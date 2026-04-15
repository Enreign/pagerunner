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

    // MARK: - Chat / Threads

    var threads: [ChatThread] = []
    var currentThreadId: UUID?

    /// Live transcript for the current thread (base64 screenshots etc.). This
    /// is rebuilt from the thread's persisted `ChatRecord`s on switch and grows
    /// as live agent events stream in.
    var chatItems: [ChatItem] = []
    var activeRunId: String?

    /// Convenience: pinned context for the current thread, or nil.
    var pinnedContext: PinnedContext? {
        currentThread?.pinnedContext
    }

    var currentThread: ChatThread? {
        guard let id = currentThreadId else { return nil }
        return threads.first(where: { $0.id == id })
    }

    private let threadStore = ThreadStore()

    // MARK: - Navigation

    var selectedTab: AppTab = .agent
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

    // MARK: - Threads

    /// Load threads from disk. If none exist, create a starter thread.
    func loadThreads() {
        do {
            let loaded = try threadStore.load()
            threads = loaded.sorted(by: { $0.updatedAt > $1.updatedAt })
        } catch {
            PgrLog.app.error("loadThreads: \(error.localizedDescription, privacy: .public)")
            threads = []
        }
        if threads.isEmpty {
            let starter = ChatThread()
            threads = [starter]
        }
        // Restore the most recently updated thread on launch.
        switchTo(threadId: threads[0].id)
    }

    /// Switch the active thread. Rebuilds `chatItems` from persisted records.
    func switchTo(threadId: UUID) {
        currentThreadId = threadId
        guard let thread = threads.first(where: { $0.id == threadId }) else {
            chatItems = []
            return
        }
        chatItems = thread.records.map(Self.live(from:))
    }

    /// Create a new thread, switch to it, and persist.
    func createThread(pinnedContext: PinnedContext? = nil) {
        let thread = ChatThread(pinnedContext: pinnedContext)
        threads.insert(thread, at: 0)
        switchTo(threadId: thread.id)
        persistThreads()
    }

    /// Update the pinned context on the current thread.
    func setPinnedContext(_ context: PinnedContext?) {
        guard let id = currentThreadId,
              let idx = threads.firstIndex(where: { $0.id == id }) else { return }
        threads[idx].pinnedContext = context
        threads[idx].updatedAt = .now
        persistThreads()
    }

    /// Delete a thread. If it was the current one, switch to the next or
    /// create a new starter.
    func deleteThread(_ id: UUID) {
        threads.removeAll(where: { $0.id == id })
        persistThreads()
        if currentThreadId == id {
            if let next = threads.first {
                switchTo(threadId: next.id)
            } else {
                createThread()
            }
        }
    }

    /// Append a `ChatRecord` to the current thread and save.
    private func appendRecord(_ record: ChatRecord) {
        guard let id = currentThreadId,
              let idx = threads.firstIndex(where: { $0.id == id }) else { return }
        threads[idx].records.append(record)
        threads[idx].updatedAt = .now
        // Auto-title from the first user message if still default.
        if threads[idx].title == "New thread", case .user(_, let text, _) = record {
            let firstLine = text
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .components(separatedBy: .newlines)
                .first ?? text
            threads[idx].title = String(firstLine.prefix(40))
        }
        persistThreads()
    }

    private func persistThreads() {
        do {
            try threadStore.save(threads)
        } catch {
            PgrLog.app.error("persistThreads: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Build a live `ChatItem` from a persisted `ChatRecord`. Screenshots
    /// recover their metadata caption but lose the base64 image (rendered
    /// as a placeholder).
    private static func live(from record: ChatRecord) -> ChatItem {
        switch record {
        case .user(let id, let text, let sent):
            return .user(id: id, text: text, sent: sent)
        case .agentDone(let id, let summary, _):
            return .agentDone(id: id, summary: summary)
        case .screenshot(let id, let sid, let tid, let title, let url, _):
            return .screenshot(id: id, base64: "", sessionId: sid, targetId: tid, caption: ChatItem.Caption(title: title, url: url))
        case .error(let id, let message, _):
            return .error(id: id, message: message)
        }
    }

    // MARK: - Polling

    func startPolling() {
        guard !isPolling else { return }
        isPolling = true
        attachWebSocketCallbacks()
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                try? await Task.sleep(for: .seconds(3))
            }
        }
    }

    /// Wire live event streams from the WebSocket into the chat transcript
    /// and the per-tab event log. Called once a connection is established.
    private func attachWebSocketCallbacks() {
        guard let ws = connection.wsClient else { return }

        ws.onAgentEvent = { [weak self] event in
            Task { @MainActor in
                guard let self else { return }
                PgrLog.agent.debug("event run=\(event.runId, privacy: .public) \(String(describing: event.event), privacy: .public)")
                let wrapped = IdentifiableAgentEvent(event: event, index: self.agentEvents.count)
                self.agentEvents.append(wrapped)
                if let item = ChatItem.from(event.event) {
                    self.chatItems.append(item)
                }
                // Persist user-visible turn outcomes only.
                switch event.event {
                case .done(let summary):
                    self.appendRecord(.agentDone(id: UUID(), summary: summary, at: .now))
                case .error(let message, _):
                    self.appendRecord(.error(id: UUID(), message: message, at: .now))
                default:
                    break
                }
                if case .approvalRequired = event.event {
                    self.pendingApproval = event.event
                }
                self.activeRunId = event.runId

                // After a visual action completes, fetch a screenshot of the
                // affected tab and surface it inline — so the user sees the
                // state of the browser at each step, not just tool metadata.
                if case .toolResult(let name, _, let isError) = event.event, !isError {
                    self.maybeAutoScreenshot(afterTool: name)
                }
            }
        }
    }

    /// Tools that visually change the page. We screenshot after these so the
    /// chat reflects the state the user would see if they opened the tab.
    private static let autoScreenshotTools: Set<String> = [
        "navigate", "new_tab", "click", "fill", "open_session",
    ]

    private func maybeAutoScreenshot(afterTool name: String) {
        guard Self.autoScreenshotTools.contains(name),
              let client = connection.apiClient else { return }
        // Pick the most recent tool_call of the same name with a session id.
        let sid: String? = {
            for e in agentEvents.reversed() {
                if case .toolCall(let ename, let args) = e.detail,
                   ename == name,
                   case .object(let dict) = args,
                   case .string(let s) = dict["session_id"] ?? .null {
                    return s
                }
            }
            return nil
        }()
        // Fall back to the first alive session for tools like open_session
        // that produce a new session_id we haven't observed yet.
        let sessionId = sid ?? aliveSessions.first?.id
        guard let sessionId else { return }

        Task {
            // Fetch the first tab for the session and screenshot it.
            let targetId: String? = await (try? client.listTabs(sessionId: sessionId))?.first?.targetId
            guard let targetId else {
                PgrLog.chat.notice("auto-screenshot skipped: no tabs for session \(sessionId, privacy: .public)")
                return
            }
            do {
                let base64 = try await client.screenshot(sessionId: sessionId, targetId: targetId)
                chatItems.append(.screenshot(id: UUID(), base64: base64, sessionId: sessionId, targetId: targetId, caption: nil))
                PgrLog.chat.info("auto-screenshot appended (tool=\(name, privacy: .public))")
            } catch {
                PgrLog.chat.error("auto-screenshot failed: \(error.localizedDescription, privacy: .public)")
            }
        }
    }

    /// Add a user-authored message to the transcript and kick off the agent.
    /// The HTTP call blocks until the agent finishes; live events continue to
    /// stream in via the WebSocket. The HTTP summary is always appended as
    /// the canonical "done" for this turn — duplicate-suppression happened
    /// before by looking at the last few items, but that incorrectly
    /// swallowed every follow-up message whose previous turn had a done.
    func sendUserMessage(_ text: String) async {
        guard let client = connection.apiClient else {
            PgrLog.chat.error("sendUserMessage with no apiClient")
            return
        }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        PgrLog.chat.info("send: \(trimmed.count) chars")
        let turnMarker = chatItems.count
        let userId = UUID()
        let now = Date.now
        chatItems.append(.user(id: userId, text: trimmed, sent: now))
        appendRecord(.user(id: userId, text: trimmed, at: now))
        isAgentRunning = true
        defer {
            isAgentRunning = false
            PgrLog.chat.info("turn ended")
        }

        do {
            var args: [String: Any] = ["goal": trimmed]
            if let ctx = pinnedContext {
                args["session_id"] = ctx.sessionId
                if let tid = ctx.targetId {
                    args["target_id"] = tid
                }
            }
            let response = try await client.callTool("agent_run", args: args)
            PgrLog.chat.info("agent_run returned ok=\(response.ok)")

            // The HTTP response often beats the final WebSocket .done event
            // by a few hundred ms — give the WebSocket a moment to catch up
            // before deciding whether to fall back.
            try? await Task.sleep(for: .milliseconds(600))

            let wsDoneThisTurn = chatItems[turnMarker...].contains { item in
                if case .agentDone = item { return true }
                return false
            }
            let wsErrorThisTurn = chatItems[turnMarker...].contains { item in
                if case .error = item { return true }
                return false
            }

            if let err = response.error, !err.isEmpty {
                let eid = UUID()
                chatItems.append(.error(id: eid, message: err))
                if !wsErrorThisTurn {
                    appendRecord(.error(id: eid, message: err, at: .now))
                }
            } else if !wsDoneThisTurn {
                // WebSocket never delivered a done — fall back to the HTTP
                // summary so the user doesn't see a silent turn.
                let summary = response.result?["summary"]?.stringValue ?? ""
                let did = UUID()
                chatItems.append(.agentDone(id: did, summary: summary))
                appendRecord(.agentDone(id: did, summary: summary, at: .now))
            }
        } catch {
            let eid = UUID()
            chatItems.append(.error(id: eid, message: error.localizedDescription))
            let wsErrorThisTurn = chatItems[turnMarker...].contains { item in
                if case .error = item { return true }
                return false
            }
            if !wsErrorThisTurn {
                appendRecord(.error(id: eid, message: error.localizedDescription, at: .now))
            }
        }
    }

    var isAgentRunning = false

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
