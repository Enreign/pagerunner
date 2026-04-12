import Foundation
import PagerunnerKit

enum AppTab: String, CaseIterable, Sendable {
    case dashboard
    case sessions
    case agent
    case observe
    case settings

    var title: String {
        rawValue.capitalized
    }

    var icon: String {
        switch self {
        case .dashboard: "figure.run"
        case .sessions: "macwindow.on.rectangle"
        case .agent: "cpu"
        case .observe: "waveform"
        case .settings: "gear"
        }
    }
}

/// Wraps an `AgentEvent` with a stable identity for use in SwiftUI lists.
struct IdentifiableAgentEvent: Identifiable, Sendable {
    let id: String
    let detail: AgentEventDetail

    init(event: AgentEvent, index: Int) {
        self.id = "\(event.runId)-\(index)"
        self.detail = event.event
    }
}

@Observable @MainActor
final class AppState {
    // MARK: - Connection

    var connection = ConnectionManager()

    // MARK: - Data

    var profiles: [Profile] = []
    var sessions: [Session] = []
    var tabs: [String: [Tab]] = [:]
    var notifications: [DaemonNotification] = []
    var recordings: [Recording] = []

    // MARK: - Agent

    var agentEvents: [IdentifiableAgentEvent] = []
    var pendingApproval: AgentEventDetail?

    // MARK: - Navigation

    var selectedTab: AppTab = .dashboard
    var selectedSession: Session?

    // MARK: - Polling

    var isPolling = false
    private var pollingTask: Task<Void, Never>?

    // MARK: - Computed

    var aliveSessions: [Session] {
        sessions.filter { $0.status == .alive }
    }

    var crashedSessions: [Session] {
        sessions.filter { $0.status == .crashed }
    }

    var recentNotifications: [DaemonNotification] {
        Array(notifications.prefix(5))
    }

    func sessionsForProfile(_ profileName: String) -> [Session] {
        sessions.filter { $0.profile == profileName }
    }

    // MARK: - Polling

    func startPolling() {
        guard !isPolling else { return }
        isPolling = true
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                try? await Task.sleep(for: .seconds(3))
            }
        }
    }

    func stopPolling() {
        isPolling = false
        pollingTask?.cancel()
        pollingTask = nil
    }

    func refresh() async {
        guard connection.isConnected, let client = connection.apiClient else { return }

        do {
            let fetchedProfiles = try await client.listProfiles()
            let fetchedSessions = try await client.listSessions()
            let fetchedNotifications = try await client.notifications()

            profiles = fetchedProfiles
            sessions = fetchedSessions
            notifications = fetchedNotifications
        } catch {
            // Silently handle polling errors; connection may have dropped.
        }
    }

    // MARK: - Session Actions

    func openSession(profile: String, stealth: Bool = false) async throws {
        guard let client = connection.apiClient else { return }
        _ = try await client.openSession(profile: profile, stealth: stealth)
        await refresh()
    }

    func closeSession(_ sessionId: String) async throws {
        guard let client = connection.apiClient else { return }
        _ = try await client.closeSession(sessionId: sessionId)
        sessions.removeAll { $0.id == sessionId }
        tabs.removeValue(forKey: sessionId)
        if selectedSession?.id == sessionId {
            selectedSession = nil
        }
    }

    func fetchTabs(for sessionId: String) async {
        guard let client = connection.apiClient else { return }
        do {
            let fetchedTabs = try await client.listTabs(sessionId: sessionId)
            tabs[sessionId] = fetchedTabs
        } catch {
            // Ignore fetch errors
        }
    }

    // MARK: - Tab Actions

    func newTab(sessionId: String) async throws {
        guard let client = connection.apiClient else { return }
        _ = try await client.newTab(sessionId: sessionId)
        await fetchTabs(for: sessionId)
    }

    func closeTab(sessionId: String, targetId: String) async throws {
        guard let client = connection.apiClient else { return }
        _ = try await client.closeTab(sessionId: sessionId, targetId: targetId)
        tabs[sessionId]?.removeAll { $0.targetId == targetId }
    }

    func navigate(sessionId: String, targetId: String, url: String) async throws {
        guard let client = connection.apiClient else { return }
        _ = try await client.navigate(sessionId: sessionId, targetId: targetId, url: url)
    }

    // MARK: - Checkpoints

    func saveCheckpoint(sessionId: String, name: String?) async throws {
        guard let client = connection.apiClient else { return }
        _ = try await client.saveCheckpoint(sessionId: sessionId, name: name)
    }

    // MARK: - Recordings

    func fetchRecordings() async {
        guard let client = connection.apiClient else { return }
        do {
            recordings = try await client.recordings()
        } catch {
            // Ignore
        }
    }
}
