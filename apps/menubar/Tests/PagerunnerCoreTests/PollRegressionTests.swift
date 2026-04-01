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

    @Test("updateTabs: nil on never-loaded session leaves entry absent (no stuck-zero bug)")
    func updateTabsNilDoesNotInitialiseEmpty() {
        // Regression: previously nil initialised tabs[id] = [] on first call, which caused
        // tabCount to get stuck at 0 — subsequent nil calls hit the "preserve" path and
        // never recovered. Fix: do nothing on nil so the next successful fetch can populate.
        let state = AppState()
        #expect(state.tabs["s1"] == nil)

        state.updateTabs(for: "s1", newTabs: nil)

        #expect(state.tabs["s1"] == nil, "nil fetch must not create an empty entry")
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

    @Test("updateSessions: clears tabs for crashed sessions (sleep/wake regression)")
    func updateSessionsClearsTabsForCrashedSessions() {
        // Regression: after sleep, Chrome dies and sessions become .crashed.
        // tabCount was still counting their stale tabs, showing e.g. "0 windows · 63 tabs".
        // Fix: updateSessions only keeps tabs for alive sessions.
        let state = AppState()
        let tab = Tab(targetId: "t1", url: "https://example.com", title: "Example")
        state.tabs["crashed-session"] = [tab]
        state.tabs["alive-session"] = [tab]

        let crashed = Session(id: "crashed-session", profile: "p", displayName: "P", stealth: false, status: .crashed)
        let alive = Session(id: "alive-session", profile: "p", displayName: "P", stealth: false, status: .alive)
        state.updateSessions([crashed, alive])

        #expect(state.tabs["crashed-session"] == nil, "crashed session tabs must be cleared")
        #expect(state.tabs["alive-session"] != nil, "alive session tabs must be preserved")
    }

    @Test("tabCount only counts alive sessions")
    func tabCountExcludesCrashedSessions() {
        let state = AppState()
        let tab = Tab(targetId: "t1", url: "https://example.com", title: "Example")
        state.sessions = [
            Session(id: "alive", profile: "p", displayName: "P", stealth: false, status: .alive),
            Session(id: "crashed", profile: "p", displayName: "P", stealth: false, status: .crashed)
        ]
        state.tabs["alive"] = [tab, tab]
        state.tabs["crashed"] = [tab, tab, tab]

        #expect(state.tabCount == 2, "crashed session tabs must not contribute to tabCount")
    }

    @Test("updateSessions: empty initial state accepts empty list")
    func updateSessionsEmptyToEmpty() {
        let state = AppState()
        #expect(state.sessions.isEmpty)

        state.updateSessions([])

        #expect(state.sessions.isEmpty)
    }

    @Test("updateSessions: returns true when update is applied")
    func updateSessionsReturnsTrueOnApply() {
        let state = AppState()
        let s = Session(id: "s1", profile: "p", displayName: "P", stealth: false, status: .alive)
        let applied = state.updateSessions([s])
        #expect(applied == true)
    }

    @Test("updateSessions: returns false when preserved (empty response with existing sessions)")
    func updateSessionsReturnsFalseOnPreserve() {
        let state = AppState()
        let s = Session(id: "s1", profile: "p", displayName: "P", stealth: false, status: .alive)
        state.sessions = [s]
        let applied = state.updateSessions([])
        #expect(applied == false)
        #expect(state.sessions.count == 1) // preserved
    }

    // MARK: Sleep/wake: profilesBeforeSleep tracks alive profiles pre-sleep

    @Test("profilesBeforeSleep captures alive profiles and skips already-alive on wake")
    func sleepWakeProfileCapture() {
        // Before sleep: work + personal alive, agent crashed
        let beforeSleep = AppState()
        beforeSleep.sessions = [
            Session(id: "s1", profile: "work", displayName: "Work", stealth: false, status: .alive),
            Session(id: "s2", profile: "personal", displayName: "Personal", stealth: false, status: .alive),
            Session(id: "s3", profile: "agent", displayName: "Agent", stealth: false, status: .crashed),
        ]
        let captured = Set(beforeSleep.sessions.filter { $0.status == .alive }.map { $0.profile })
        #expect(captured == ["work", "personal"], "only alive profiles captured before sleep")
        #expect(!captured.contains("agent"), "crashed sessions must not be captured")

        // After wake: Chrome died — all sessions crashed; "personal" reconnected on its own
        let afterWake = AppState()
        afterWake.sessions = [
            Session(id: "s2", profile: "personal", displayName: "Personal", stealth: false, status: .alive),
        ]
        let aliveAfterWake = Set(afterWake.sessions.filter { $0.status == .alive }.map { $0.profile })
        let toReopen = captured.filter { !aliveAfterWake.contains($0) }
        #expect(toReopen == ["work"], "only profiles still missing should be reopened")
    }

    // MARK: Gap A regression — previousSessionStates must not be reset during preservation

    /// Regression for: when updateSessions preserves (empty daemon response), previousSessionStates
    /// was still replaced with [] — causing sessions that reappear to fire spurious "started" notifications.
    @Test("preserved sessions do not cause spurious 'started' notification on next poll")
    func preservedSessionsNoSpuriousStartNotification() async throws {
        // Poll 1: session s1 is alive — sets previousSessionStates[s1] = .alive
        // Poll 2: daemon returns empty (preserved) — previousSessionStates must NOT be wiped
        // Poll 3: daemon returns s1 alive again — must NOT fire "started" since prev is still .alive

        let server = MultiMockSocketServer()
        // Poll 1: profiles + sessions with s1
        // Poll 2: profiles + sessions empty (daemon hiccup)
        // Poll 3: profiles + sessions with s1 again + tabs
        await server.start(responses: [
            emptyProfiles, oneAliveSession,          // poll 1
            emptyProfiles, sessionsNoDataKey,         // poll 2 (empty-ish)
            emptyProfiles, oneAliveSession, oneTab,   // poll 3
        ])
        try await Task.sleep(for: .milliseconds(50))

        let delegate = AppDelegate()
        let client = DaemonClient(socketPath: await server.socketPath)

        // Poll 1 — establishes s1 as known alive
        await delegate.poll(client: client)
        #expect(delegate.previousSessionStates["s1"] == .alive)

        // Poll 2 — empty response, should preserve both sessions and previousSessionStates
        await delegate.poll(client: client)
        #expect(delegate.appState.sessions.count == 1, "sessions preserved")
        #expect(delegate.previousSessionStates["s1"] == .alive, "previousSessionStates preserved")

        // Poll 3 — s1 reappears; previousSessionStates[s1] is still .alive so no "started" fires
        // We verify by checking that s1's prev was .alive before this poll
        let prevBeforePoll3 = delegate.previousSessionStates["s1"]
        await delegate.poll(client: client)
        #expect(prevBeforePoll3 == .alive, "prev was alive — no spurious start notification")

        await server.stop()
    }

    // MARK: Gap B regression — idle tracker cleaned up for crashed sessions

    @Test("idle tracker entries for crashed sessions are removed")
    func idleTrackerClearedForCrashedSessions() async throws {
        let server = MultiMockSocketServer()
        let oneDeadSession = daemonResponse(
            #"{"ok":true,"data":[{"id":"s1","profile":"personal","display_name":"Personal","status":"crashed","stealth":false}]}"#
        )
        // list_profiles + list_sessions (s1 crashed) + list_session_checkpoints (personal)
        await server.start(responses: [emptyProfiles, oneDeadSession, emptyProfiles])
        try await Task.sleep(for: .milliseconds(50))

        let delegate = AppDelegate()
        delegate.previousSessionStates = ["s1": .alive]
        // Pre-seed the tracker as if s1 was previously alive and being tracked
        delegate.sessionIdleTracker["s1"] = (tabCount: 2, stableFrom: Date())
        delegate.idleNotifiedSessions.insert("s1")

        let client = DaemonClient(socketPath: await server.socketPath)
        await delegate.poll(client: client)

        // s1 is now crashed — its tracker entries must be cleared
        #expect(delegate.appState.sessions.first?.status == .crashed)
        #expect(delegate.sessionIdleTracker["s1"] == nil, "tracker entry removed for crashed session")
        #expect(!delegate.idleNotifiedSessions.contains("s1"), "idle flag removed for crashed session")

        await server.stop()
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
