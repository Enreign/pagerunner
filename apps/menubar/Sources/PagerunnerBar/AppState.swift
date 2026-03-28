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

    // MARK: - Daemon status
    var daemonStatus: DaemonStatus = .stopped
    var consecutiveFailures = 0
    var lastSuccessAt: Date?

    /// Set during intentional start/stop — suppresses poll-driven status flicker.
    enum TransitionState: Equatable {
        case none
        case starting
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

    // MARK: - Profile management (stubs — wired up in Task 7)

    /// Rename a profile's display name. Implementation in Task 7.
    func renameProfile(_ profile: Profile, newDisplayName: String) {
        // TODO: Task 7 — call ConfigEditor + restart daemon
    }

    /// Remove a profile from config. Implementation in Task 7.
    func removeProfile(_ profile: Profile) {
        // TODO: Task 7 — call ConfigEditor + restart daemon
    }

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
        if transition == .starting {
            // Start confirmed — daemon is alive
            daemonStatus = .running
            transition = .none
        } else if transition == .none {
            daemonStatus = .running
        }
        // If .stopping, ignore successes (race with dying daemon)
    }
}

