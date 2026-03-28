import Foundation
import Observation
import ServiceManagement
import PagerunnerCore

/// Navigation state for the panel.
enum PanelNavigation: Equatable {
    case overview
    case profile(String)          // profile name
    case settings
    case addProfile
}

/// Single source of truth for all app state. @Observable triggers SwiftUI re-renders.
@MainActor
@Observable
final class AppState {
    // MARK: - Data
    var profiles: [Profile] = []
    var sessions: [Session] = []
    var tabs: [String: [Tab]] = [:]              // sessionId → tabs
    var checkpoints: [String: [Checkpoint]] = [:] // profileName → checkpoints
    /// Session IDs ever observed as alive — distinguishes dead-on-arrival from crashed-later.
    var everAliveSessions: Set<String> = []

    // MARK: - Discovery
    var discoveredInstances: [DiscoveredInstance] = []
    /// Profile labels attached from discovered (gvproxy-forwarded) instances.
    var remoteSessions: Set<String> = []

    // MARK: - Daemon status
    var daemonStatus: DaemonStatus = .stopped
    var consecutiveFailures = 0
    var lastSuccessAt: Date?

    /// Set during intentional start/stop — suppresses poll-driven status flicker.
    enum TransitionState: Equatable {
        case none
        case starting
        case restarting
        case stopping
    }
    var transition: TransitionState = .none

    // MARK: - Navigation
    var navigation: PanelNavigation = .overview

    // MARK: - Binary path
    /// Path to the pagerunner binary, or nil if not found.
    var binaryPath: String? = nil

    // MARK: - Launch at login (SMAppService, macOS 13+)
    var launchAtLogin: Bool = false {
        didSet {
            if launchAtLogin {
                try? SMAppService.mainApp.register()
            } else {
                try? SMAppService.mainApp.unregister()
            }
        }
    }

    // MARK: - Private services
    private let daemonClient = DaemonClient()
    private let discoveryService = DiscoveryService()

    // MARK: - Computed
    var sessionCount: Int { sessions.filter { $0.status == .alive }.count }
    var tabCount: Int { tabs.values.reduce(0) { $0 + $1.count } }

    var personalProfiles: [Profile] { profiles.filter { $0.kind != "agent" } }
    var agentProfiles: [Profile] { profiles.filter { $0.kind == "agent" } }

    func sessionsFor(profile: String) -> [Session] {
        sessions.filter { $0.profile == profile }
    }

    func tabsFor(session: String) -> [Tab] {
        tabs[session] ?? []
    }

    func checkpointsFor(profile: String) -> [Checkpoint] {
        checkpoints[profile] ?? []
    }

    // MARK: - Daemon restart

    func restartDaemon() async {
        guard let binary = binaryPath else { return }
        let kill = Process()
        kill.launchPath = "/usr/bin/pkill"
        kill.arguments = ["-f", "pagerunner daemon"]
        try? kill.run()
        // Don't call waitUntilExit() on main actor — use a short sleep instead
        try? await Task.sleep(for: .milliseconds(500))
        let proc = Process()
        proc.launchPath = binary
        proc.arguments = ["daemon"]
        try? proc.run()
        transition = .restarting
        // Wait for daemon to be ready (up to 5s, 500ms intervals)
        for _ in 0..<10 {
            try? await Task.sleep(for: .milliseconds(500))
            if (try? await daemonClient.call(tool: "list_profiles")) != nil { break }
        }
    }

    // MARK: - Profile refresh

    func refreshProfiles() async {
        guard let profilesRaw = try? await daemonClient.call(tool: "list_profiles") else { return }
        guard let data = profilesRaw["data"]?.arrayValue else { return }
        profiles = data.compactMap { item -> Profile? in
            guard let obj = item.objectValue else { return nil }
            var dict: [String: Any] = [
                "name": obj["name"]?.stringValue as Any,
                "display_name": obj["display_name"]?.stringValue as Any,
                "kind": obj["kind"]?.stringValue ?? "personal"
            ]
            if let udd = obj["user_data_dir"]?.stringValue {
                dict["user_data_dir"] = udd
            }
            guard let jsonData = try? JSONSerialization.data(withJSONObject: dict),
                  let profile = try? JSONDecoder().decode(Profile.self, from: jsonData) else { return nil }
            return profile
        }
    }

    // MARK: - Profile management

    func renameProfile(_ profile: Profile, newDisplayName: String) async throws {
        try ConfigEditor.renameProfile(name: profile.name, newDisplayName: newDisplayName)
        await restartDaemon()
        await refreshProfiles()
    }

    func removeProfile(_ profile: Profile) async throws {
        // 1. Close any active sessions for this profile
        let sessionsToClose = sessions.filter { $0.profile == profile.name }
        for session in sessionsToClose {
            do {
                _ = try await daemonClient.call(tool: "close_session",
                                                args: ["session_id": session.id])
            } catch {
                print("closeSession error (continuing): \(error)")
                // non-fatal: continue even if close fails
            }
        }

        // 2. Remove from config
        try ConfigEditor.removeProfile(name: profile.name)

        // 3. Restart daemon
        await restartDaemon()

        // 4. Refresh profiles
        await refreshProfiles()
    }

    // MARK: - Discovery

    func triggerDiscovery() {
        Task {
            let found = await discoveryService.probe()
            // Filter out ports already covered by an attached-kind profile
            let managedPorts = Set(profiles.compactMap { p -> Int? in
                guard p.kind == "attached", let port = p.debugPort else { return nil }
                return port
            })
            let unmanaged = found.filter { !managedPorts.contains($0.port) }
            // Preserve non-idle attach states for existing instances
            discoveredInstances = unmanaged.map { instance in
                if let existing = discoveredInstances.first(where: { $0.id == instance.id }),
                   existing.attachState != .idle {
                    var updated = instance
                    updated.attachState = existing.attachState
                    return updated
                }
                return instance
            }
        }
    }

    /// Merge a discovered port into an existing profile (adds debug_port to its config entry).
    func mergeDiscovered(_ instance: DiscoveredInstance, intoProfile profileName: String) {
        Task {
            guard let idx = discoveredInstances.firstIndex(where: { $0.id == instance.id }) else { return }
            discoveredInstances[idx].attachState = .attaching
            do {
                try ConfigEditor.addDebugPortToProfile(name: profileName, port: instance.port)
                await restartDaemon()
                await refreshProfiles()
                _ = try? await daemonClient.call(tool: "open_session", args: ["profile": profileName])
                discoveredInstances[idx].attachState = .attached
                navigation = .profile(profileName)
            } catch {
                discoveredInstances[idx].attachState = .failed(error.localizedDescription)
            }
        }
    }

    func attachDiscovered(_ instance: DiscoveredInstance, displayName: String) {
        Task {
            guard let idx = discoveredInstances.firstIndex(where: { $0.id == instance.id }) else { return }
            discoveredInstances[idx].attachState = .attaching

            let label = "chrome-\(instance.port)"
            do {
                // Save profile to config so it appears permanently in Overview
                try ConfigEditor.addAttachedProfile(name: label, displayName: displayName, port: instance.port)

                // Restart daemon to pick up the new profile entry
                await restartDaemon()
                await refreshProfiles()
                _ = try? await daemonClient.call(tool: "open_session", args: ["profile": label])

                discoveredInstances[idx].attachState = .attached
                if instance.isVM {
                    remoteSessions.insert(label)
                }
                // Navigate to the new profile so user sees the attached session
                navigation = .profile(label)
            } catch {
                discoveredInstances[idx].attachState = .failed(error.localizedDescription)
            }
        }
    }

    // MARK: - Daemon status tracking

    /// Record a poll failure and update daemonStatus.
    func recordFailure() {
        consecutiveFailures += 1
        if transition == .stopping {
            // Stopping confirmed — daemon is dead
            daemonStatus = .stopped
            transition = .none
        } else if transition == .none {
            daemonStatus = DaemonStatus.fromFailureCount(consecutiveFailures, lastSeenAt: lastSuccessAt)
        }
        // If .starting, ignore failures (daemon still booting)
    }

    /// Record a successful poll.
    func recordSuccess() {
        consecutiveFailures = 0
        lastSuccessAt = Date()
        if transition == .starting || transition == .restarting {
            // Start/restart confirmed — daemon is alive
            daemonStatus = .running
            transition = .none
        } else if transition == .none {
            daemonStatus = .running
        }
        // If .stopping, ignore successes (race with dying daemon)
    }
}
