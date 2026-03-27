import Foundation
import Observation
import ServiceManagement
import PagerunnerCore

/// Navigation state for the panel.
enum PanelNavigation: Equatable {
    case overview
    case profile(String)          // profile name
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

    // MARK: - Daemon status
    var daemonStatus: DaemonStatus = .stopped
    var consecutiveFailures = 0
    var lastSuccessAt: Date?

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

    /// Record a poll failure and update daemonStatus.
    func recordFailure() {
        consecutiveFailures += 1
        daemonStatus = DaemonStatus.fromFailureCount(consecutiveFailures, lastSeenAt: lastSuccessAt)
    }

    /// Record a successful poll.
    func recordSuccess() {
        consecutiveFailures = 0
        lastSuccessAt = Date()
        daemonStatus = .running
    }
}
