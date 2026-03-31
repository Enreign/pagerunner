import Testing
import Foundation
@testable import PagerunnerBar
@testable import PagerunnerCore

// MARK: - Multi-response mock socket server

/// Handles N sequential connections, returning one canned response per connection.
/// Any connections beyond the response list get ECONNREFUSED (server is closed).
actor MultiMockSocketServer {
    let socketPath: String
    private var serverTask: Task<Void, Never>?

    init() {
        socketPath = FileManager.default.temporaryDirectory
            .appendingPathComponent("test-pagerunner-multi-\(UUID().uuidString).sock").path
    }

    func start(responses: [String]) {
        let path = socketPath
        serverTask = Task {
            let fd = socket(AF_UNIX, SOCK_STREAM, 0)
            guard fd >= 0 else { return }
            defer { Darwin.close(fd) }

            var addr = sockaddr_un()
            addr.sun_family = sa_family_t(AF_UNIX)
            withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
                path.withCString { cstr in
                    _ = Darwin.strcpy(
                        UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self), cstr)
                }
            }
            let addrLen = socklen_t(MemoryLayout<sockaddr_un>.size)
            guard Darwin.bind(fd, withUnsafePointer(to: addr) {
                UnsafeRawPointer($0).assumingMemoryBound(to: sockaddr.self) }, addrLen) == 0 else { return }
            guard Darwin.listen(fd, Int32(responses.count) + 1) == 0 else { return }

            for response in responses {
                guard !Task.isCancelled else { break }
                let clientFd = Darwin.accept(fd, nil, nil)
                guard clientFd >= 0 else { break }
                // Drain the request line
                var buf = [UInt8](repeating: 0, count: 4096)
                Darwin.read(clientFd, &buf, buf.count)
                // Write the canned response
                let line = response + "\n"
                line.withCString { Darwin.write(clientFd, $0, strlen($0)) }
                Darwin.close(clientFd)
            }
        }
    }

    func stop() {
        serverTask?.cancel()
        try? FileManager.default.removeItem(atPath: socketPath)
    }
}

// MARK: - Helpers

/// Wraps an inner JSON dict as the double-encoded string the DaemonClient protocol requires.
private func daemonResponse(_ inner: String) -> String {
    let escaped = inner
        .replacingOccurrences(of: "\\", with: "\\\\")
        .replacingOccurrences(of: "\"", with: "\\\"")
    return "{\"id\":\"r\",\"result\":\"\(escaped)\",\"error\":null}"
}

private let emptyProfiles = daemonResponse(#"{"ok":true,"data":[]}"#)

/// One alive session with id "s1".
private let oneAliveSession = daemonResponse(
    #"{"ok":true,"data":[{"id":"s1","profile":"personal","display_name":"Personal","status":"alive","stealth":false}]}"#
)

/// A list_sessions response that has "ok" but no "data" key.
private let sessionsNoDataKey = daemonResponse(#"{"ok":true}"#)

/// One tab for session "s1".
private let oneTab = daemonResponse(
    #"{"ok":true,"data":[{"target_id":"t1","url":"https://example.com","title":"Example"}]}"#
)

// MARK: - Tests

@Suite("Poll regression: session/tab continuity")
@MainActor
struct PollRegressionTests {

    // MARK: Bug 1

    /// Regression for: list_sessions returns a valid response but without a "data" array.
    /// Before the fix, poll returned early without calling recordSuccess(), so the
    /// consecutiveFailures counter kept incrementing until the daemon appeared stopped.
    @Test("list_sessions missing 'data' key does not increment failure count")
    func listSessionsMissingDataKeyDoesNotIncrementFailures() async throws {
        let server = MultiMockSocketServer()
        // Provide responses for: list_profiles, list_sessions (no "data" key)
        // list_tabs is never reached because guard triggers early return
        await server.start(responses: [emptyProfiles, sessionsNoDataKey])
        try await Task.sleep(for: .milliseconds(50))

        let delegate = AppDelegate()
        let client = DaemonClient(socketPath: await server.socketPath)

        // Simulate 2 previous failures (daemon appears stale)
        delegate.appState.recordFailure()
        delegate.appState.recordFailure()
        #expect(delegate.appState.consecutiveFailures == 2)

        await delegate.poll(client: client)

        // After a "successful" poll (daemon responded, just no sessions),
        // failures must be reset to 0 — daemon is alive.
        #expect(delegate.appState.consecutiveFailures == 0)
        #expect(delegate.appState.sessions.isEmpty)

        await server.stop()
    }

    // MARK: Bug 2

    /// Regression for: list_tabs call fails → tabs immediately wiped to [].
    /// Before the fix, any transient failure cleared visible tabs.
    @Test("list_tabs failure preserves existing tabs")
    func listTabsFailurePreservesExistingTabs() async throws {
        let server = MultiMockSocketServer()
        // list_profiles + list_sessions succeed; list_tabs gets no response (server closes)
        await server.start(responses: [emptyProfiles, oneAliveSession])
        try await Task.sleep(for: .milliseconds(50))

        let delegate = AppDelegate()
        // Pre-seed previousSessionStates so no "session started" notification fires
        delegate.previousSessionStates = ["s1": .alive]

        // Pre-load tabs that must survive the poll
        let existingTab = Tab(targetId: "t1", url: "https://example.com", title: "Example")
        delegate.appState.tabs["s1"] = [existingTab]

        let client = DaemonClient(socketPath: await server.socketPath)
        await delegate.poll(client: client)

        // list_tabs had no server to connect to → should preserve existing tab
        let tabs = delegate.appState.tabs["s1"]
        #expect(tabs?.count == 1)
        #expect(tabs?.first?.targetId == "t1")

        await server.stop()
    }

    /// Complementary: when list_tabs succeeds, tabs are updated normally.
    @Test("list_tabs success updates tabs")
    func listTabsSuccessUpdatesTabs() async throws {
        let server = MultiMockSocketServer()
        await server.start(responses: [emptyProfiles, oneAliveSession, oneTab])
        try await Task.sleep(for: .milliseconds(50))

        let delegate = AppDelegate()
        delegate.previousSessionStates = ["s1": .alive]

        let client = DaemonClient(socketPath: await server.socketPath)
        await delegate.poll(client: client)

        let tabs = delegate.appState.tabs["s1"]
        #expect(tabs?.count == 1)
        #expect(tabs?.first?.url == "https://example.com")

        await server.stop()
    }

    // MARK: AppState.updateTabs unit tests

    @Test("updateTabs: nil preserves existing tabs")
    func updateTabsNilPreservesExisting() {
        let state = AppState()
        let tab = Tab(targetId: "t1", url: "https://example.com", title: "Example")
        state.tabs["s1"] = [tab]

        state.updateTabs(for: "s1", newTabs: nil)

        #expect(state.tabs["s1"]?.count == 1)
        #expect(state.tabs["s1"]?.first?.targetId == "t1")
    }

    @Test("updateTabs: nil initialises to empty when session never loaded")
    func updateTabsNilInitialisesEmpty() {
        let state = AppState()
        #expect(state.tabs["s1"] == nil)

        state.updateTabs(for: "s1", newTabs: nil)

        #expect(state.tabs["s1"] != nil)
        #expect(state.tabs["s1"]?.isEmpty == true)
    }

    @Test("updateTabs: non-nil replaces tabs")
    func updateTabsReplacesTabs() {
        let state = AppState()
        let old = Tab(targetId: "old", url: "https://old.com", title: "Old")
        let new = Tab(targetId: "new", url: "https://new.com", title: "New")
        state.tabs["s1"] = [old]

        state.updateTabs(for: "s1", newTabs: [new])

        #expect(state.tabs["s1"]?.count == 1)
        #expect(state.tabs["s1"]?.first?.targetId == "new")
    }

    // MARK: AppState.updateSessions unit tests

    @Test("updateSessions: empty list preserves existing sessions")
    func updateSessionsEmptyPreservesExisting() {
        let state = AppState()
        let session = Session(id: "s1", profile: "personal", displayName: "Personal", stealth: false, status: .alive)
        state.sessions = [session]

        state.updateSessions([])

        #expect(state.sessions.count == 1)
        #expect(state.sessions.first?.id == "s1")
    }

    @Test("updateSessions: non-empty list replaces sessions")
    func updateSessionsNonEmptyReplaces() {
        let state = AppState()
        let old = Session(id: "s1", profile: "personal", displayName: "Personal", stealth: false, status: .alive)
        let new = Session(id: "s2", profile: "work", displayName: "Work", stealth: false, status: .alive)
        state.sessions = [old]

        state.updateSessions([new])

        #expect(state.sessions.count == 1)
        #expect(state.sessions.first?.id == "s2")
    }

    @Test("updateSessions: removes tabs for sessions that disappeared")
    func updateSessionsCleansUpDeadTabs() {
        let state = AppState()
        let tab = Tab(targetId: "t1", url: "https://example.com", title: "Example")
        state.tabs["dead-session"] = [tab]
        state.tabs["live-session"] = [tab]

        let live = Session(id: "live-session", profile: "p", displayName: "P", stealth: false, status: .alive)
        state.updateSessions([live])

        #expect(state.tabs["dead-session"] == nil)
        #expect(state.tabs["live-session"] != nil)
    }

    @Test("updateSessions: empty initial state accepts empty list")
    func updateSessionsEmptyToEmpty() {
        let state = AppState()
        #expect(state.sessions.isEmpty)

        state.updateSessions([])

        #expect(state.sessions.isEmpty)
    }
}

// MARK: - PollingService concurrency tests

@Suite("PollingService concurrency")
@MainActor
struct PollingServiceConcurrencyTests {

    @Test("concurrent poll guard: second call skipped while first is in flight")
    func concurrentPollGuardSkipsSecondCall() async {
        // Use a counter to track how many times the handler actually ran to completion
        actor Counter { var n = 0; func inc() { n += 1 } }
        let counter = Counter()

        // A poll handler that takes 200ms (simulates a slow poll)
        let service = PollingService { @MainActor in
            try? await Task.sleep(for: .milliseconds(200))
            await counter.inc()
        }
        service.start()  // starts with immediate: true — first poll fires immediately

        // Immediately trigger a second loop (simulates panel open while first poll in flight)
        service.panelDidOpen()

        // Wait enough time for at most 1 poll to complete (300ms > 200ms handler but < 2 polls)
        try? await Task.sleep(for: .milliseconds(300))
        service.stop()

        // Only 1 completion — the in-flight poll was not interrupted or doubled
        #expect(await counter.n == 1)
    }
}
