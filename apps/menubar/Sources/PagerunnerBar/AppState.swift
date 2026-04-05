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
    var sessionCount: Int { sessions.filter { $0.status == .alive || $0.status == .reconnecting || $0.status == .recovering }.count }
    var tabCount: Int {
        let aliveIds = Set(sessions.filter { $0.status == .alive || $0.status == .reconnecting || $0.status == .recovering }.map { $0.id })
        return tabs.filter { aliveIds.contains($0.key) }.values.reduce(0) { $0 + $1.count }
    }

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

    /// Update the sessions list. Preserves existing sessions when the new list is suspiciously
    /// empty (daemon hiccup) — only clears when we had no sessions to begin with.
    /// Also removes tabs for sessions that are no longer present.
    /// Returns true if the update was applied, false if the old state was preserved.
    @discardableResult
    func updateSessions(_ newSessions: [Session]) -> Bool {
        if !newSessions.isEmpty || sessions.isEmpty {
            sessions = newSessions
            // Keep tabs only for sessions that still exist AND are alive.
            // Crashed/gone sessions have no live tabs — clearing them prevents stale counts.
            let aliveIds = Set(newSessions.filter { $0.status == .alive || $0.status == .reconnecting || $0.status == .recovering }.map { $0.id })
            tabs = tabs.filter { aliveIds.contains($0.key) }
            return true
        }
        // Transient empty response — preserve existing sessions and their tabs
        return false
    }

    /// Update tabs for a session. Pass nil on a failed fetch to preserve existing tabs;
    /// only initialises to empty if no tabs have ever been loaded for this session.
    func updateTabs(for sessionId: String, newTabs: [Tab]?) {
        if let tabs = newTabs {
            self.tabs[sessionId] = tabs
        }
        // nil: transient failure — keep whatever was there, or nothing if not yet loaded.
        // Do NOT initialise to [] here: a failed first fetch would permanently show 0 tabs
        // because subsequent nil calls hit the "preserve existing" path and never recover.
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
        transition = .none  // Always clear — next successful poll will confirm daemon is up
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

    /// Remove debug_port from a profile config and restart daemon (permanent detach).
    func detachProfile(_ profile: Profile) async throws {
        // Close any active sessions first
        let activeSessions = sessions.filter { $0.profile == profile.name }
        for session in activeSessions {
            _ = try? await daemonClient.call(tool: "close_session", args: ["session_id": session.id])
        }
        try ConfigEditor.removeDebugPortFromProfile(name: profile.name)
        remoteSessions.remove(profile.name)
        await restartDaemon()
        await refreshProfiles()
        navigation = .profile(profile.name)
    }

    /// Merge a discovered port into an existing profile (adds debug_port to its config entry).
    func mergeDiscovered(_ instance: DiscoveredInstance, intoProfile profileName: String) {
        Task {
            guard let idx = discoveredInstances.firstIndex(where: { $0.id == instance.id }) else { return }
            discoveredInstances[idx].attachState = .attaching
            do {
                try ConfigEditor.addDebugPortToProfile(name: profileName, port: instance.port)
                await discoveryService.invalidateCache()
                await restartDaemon()
                await refreshProfiles()
                _ = try? await daemonClient.call(tool: "open_session", args: ["profile": profileName])
                let displayName = profiles.first(where: { $0.name == profileName })?.displayName ?? profileName
                discoveredInstances[idx].attachState = .attached(profileDisplayName: displayName)
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
                await discoveryService.invalidateCache()
                await restartDaemon()
                await refreshProfiles()
                _ = try? await daemonClient.call(tool: "open_session", args: ["profile": label])

                discoveredInstances[idx].attachState = .attached(profileDisplayName: displayName)
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
            consecutiveFailures = 0
            transition = .none
        } else if transition == .none {
            // Already intentionally stopped — don't oscillate back to stale
            guard daemonStatus != .stopped else { return }
            daemonStatus = DaemonStatus.fromFailureCount(consecutiveFailures, lastSeenAt: lastSuccessAt)
        }
        // If .starting or .restarting, ignore failures (daemon still booting)
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
