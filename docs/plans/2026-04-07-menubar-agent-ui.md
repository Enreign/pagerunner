# Menu Bar Agent UI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Agent tab to the Pagerunner menu bar app — users type a goal, the agent browses autonomously via the daemon's `agent.run` IPC, and a narrated event feed shows progress in real-time.

**Architecture:** New Swift views in `Sources/PagerunnerBar/Views/` (AgentView, AgentFeedView, AgentIdleView), new agent state in `AppState`, and a streaming daemon client method in `PagerunnerCore/DaemonClient.swift`. No Rust changes — the daemon already supports `DaemonMessage::AgentRun` with event streaming.

**Tech Stack:** Swift 6, SwiftUI, @Observable, Unix domain sockets, UNUserNotificationCenter

---

## File Structure

```
Sources/PagerunnerCore/
  DaemonClient.swift          — MODIFY: add streamAgentRun() method
  Models.swift                — MODIFY: add agent models (AgentEvent, AgentMode, RecentGoal)

Sources/PagerunnerBar/
  AppState.swift              — MODIFY: add agent state (AgentRunState, navigation case)
  NotificationService.swift   — MODIFY: add AGENT_APPROVAL, AGENT_DONE, AGENT_ERROR categories
  Views/
    PanelView.swift           — MODIFY: add .agent navigation case + 🤖 tab in NavigationStrip
    AgentView.swift           — CREATE: top-level agent view (switches idle/running/completed/error)
    AgentIdleView.swift       — CREATE: centered input, profile picker, mode picker, recent list
    AgentFeedView.swift       — CREATE: narrated event feed with auto-scroll
    AgentApprovalCard.swift   — CREATE: inline approval card (approve/deny buttons)
```

---

### Task 1: Agent Models in PagerunnerCore

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerCore/Models.swift`

- [ ] **Step 1: Add agent event and mode types**

Append to `Models.swift`:

```swift
// MARK: - Agent models

/// Event streamed from the daemon during an agent run.
/// Matches the Rust `AgentEvent` enum serialization (tagged with "type").
public struct AgentEventWire: Codable, Sendable {
    public let type: String
    // Optional fields — present depending on type
    public let text: String?
    public let name: String?
    public let args: AnyCodable?
    public let result: String?
    public let isError: Bool?
    public let message: String?
    public let recoverable: Bool?
    public let summary: String?
    public let runId: String?
    public let action: String?
    public let description: String?
    public let reason: String?
    public let artifacts: [AnyCodable]?

    enum CodingKeys: String, CodingKey {
        case type, text, name, args, result, message, recoverable
        case summary, action, description, reason, artifacts
        case isError = "is_error"
        case runId = "run_id"
    }
}

/// Wrapper for DaemonEvent JSON lines: {"run_id":"...","event":{...}}
public struct DaemonEventWire: Codable, Sendable {
    public let runId: String
    public let event: AgentEventWire

    enum CodingKeys: String, CodingKey {
        case runId = "run_id"
        case event
    }
}

/// Agent run result (final DaemonResponse inner JSON).
public struct AgentRunResult: Codable, Sendable {
    public let outcome: String
    public let summary: String?
    public let totalSteps: Int?
    public let inputTokens: Int?
    public let outputTokens: Int?

    enum CodingKeys: String, CodingKey {
        case outcome, summary
        case totalSteps = "total_steps"
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
    }
}

/// Minimal Codable wrapper for heterogeneous JSON values.
public struct AnyCodable: Codable, Sendable {
    public let value: Any?

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let s = try? container.decode(String.self) { value = s }
        else if let i = try? container.decode(Int.self) { value = i }
        else if let d = try? container.decode(Double.self) { value = d }
        else if let b = try? container.decode(Bool.self) { value = b }
        else { value = nil }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        if let s = value as? String { try container.encode(s) }
        else if let i = value as? Int { try container.encode(i) }
        else if let d = value as? Double { try container.encode(d) }
        else if let b = value as? Bool { try container.encode(b) }
        else { try container.encodeNil() }
    }
}

/// Approval mode — maps to AutonomyPolicy in Rust.
public enum AgentMode: String, Codable, CaseIterable, Sendable {
    case fullAuto = "full_auto"
    case supervised = "supervised"
    case stepByStep = "step_by_step"

    public var label: String {
        switch self {
        case .fullAuto: return "Full Auto"
        case .supervised: return "Supervised"
        case .stepByStep: return "Step-by-Step"
        }
    }

    public var description: String {
        switch self {
        case .fullAuto: return "Agent acts freely, no approvals"
        case .supervised: return "Approve clicks, fills, and code execution"
        case .stepByStep: return "Approve every action"
        }
    }

    /// Convert to the autonomy policy JSON for the daemon.
    public var autonomyArgs: [String: Any] {
        switch self {
        case .fullAuto:
            return ["auto_approve": ["*"], "require_approval": [] as [String], "block": [] as [String]]
        case .supervised:
            return [
                "auto_approve": ["navigate", "get_content", "screenshot", "scroll",
                                 "list_tabs", "list_sessions", "list_profiles", "new_tab", "close_tab"],
                "require_approval": ["click", "fill", "type_text", "select", "evaluate",
                                     "open_session", "close_session"],
                "block": [] as [String]
            ]
        case .stepByStep:
            return ["auto_approve": [] as [String], "require_approval": ["*"], "block": [] as [String]]
        }
    }
}

/// A recent goal entry for history display.
public struct RecentGoal: Codable, Identifiable, Sendable {
    public var id: String { "\(timestamp)-\(goal.prefix(20))" }
    public let goal: String
    public let profile: String
    public let timestamp: Date
    public let duration: TimeInterval
    public let steps: Int
    public let outcome: String

    public init(goal: String, profile: String, timestamp: Date, duration: TimeInterval, steps: Int, outcome: String) {
        self.goal = goal
        self.profile = profile
        self.timestamp = timestamp
        self.duration = duration
        self.steps = steps
        self.outcome = outcome
    }
}
```

- [ ] **Step 2: Build to verify**

Run: `cd apps/menubar && swift build -c release 2>&1 | tail -3`
Expected: builds successfully.

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerCore/Models.swift
git commit -m "feat(menubar): add agent event, mode, and history models"
```

---

### Task 2: Streaming Daemon Client

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerCore/DaemonClient.swift`

- [ ] **Step 1: Add streaming agent run method**

Add to `DaemonClient`:

```swift
    /// Enum for events received during an agent stream.
    public enum AgentStreamEvent: Sendable {
        case event(DaemonEventWire)
        case result(AgentRunResult)
        case error(String)
    }

    /// Start an agent run and stream events back.
    ///
    /// Opens a long-lived socket connection, sends the AgentRun message,
    /// then reads lines until the final DaemonResponse arrives.
    /// The caller receives an AsyncThrowingStream of AgentStreamEvent.
    public func streamAgentRun(
        goal: String,
        profile: String?,
        model: String?,
        maxSteps: Int?,
        mode: AgentMode
    ) -> AsyncThrowingStream<AgentStreamEvent, Error> {
        let socketPath = self.socketPath
        let requestId = UUID().uuidString

        // Build the agent config
        var config: [String: Any] = [:]
        config["autonomy"] = mode.autonomyArgs
        if let profile { config["session_profile"] = profile }
        if let model { config["model"] = model }
        if let maxSteps { config["budget"] = ["max_steps": maxSteps] }

        let message: [String: Any] = [
            "type": "agent_run",
            "id": requestId,
            "goal": goal,
            "config": config
        ]

        return AsyncThrowingStream { continuation in
            Task.detached(priority: .utility) {
                do {
                    try Self.performStreamingRun(
                        socketPath: socketPath,
                        message: message,
                        continuation: continuation
                    )
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    /// Send an approval response on a fresh socket connection.
    public func sendApproval(runId: String, approved: Bool) async throws {
        let socketPath = self.socketPath
        let requestId = UUID().uuidString
        let message: [String: Any] = [
            "type": "agent_approve",
            "id": requestId,
            "run_id": runId,
            "approved": approved
        ]
        let messageData = try JSONSerialization.data(withJSONObject: message)
        var line = messageData
        line.append(0x0A)

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .utility) {
                do {
                    // Open socket, send, read response (don't need to parse)
                    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
                    guard fd >= 0 else { throw DaemonError.daemonStopped }
                    defer { Darwin.close(fd) }

                    var addr = sockaddr_un()
                    addr.sun_family = sa_family_t(AF_UNIX)
                    withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
                        socketPath.withCString { cstr in
                            _ = Darwin.strcpy(
                                UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self),
                                cstr
                            )
                        }
                    }
                    let connectResult = withUnsafePointer(to: addr) { ptr in
                        Darwin.connect(fd, UnsafeRawPointer(ptr).assumingMemoryBound(to: sockaddr.self), socklen_t(MemoryLayout<sockaddr_un>.size))
                    }
                    guard connectResult == 0 else { throw DaemonError.daemonStopped }
                    _ = line.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Send an interrupt on a fresh socket connection.
    public func sendInterrupt(runId: String) async throws {
        let socketPath = self.socketPath
        let requestId = UUID().uuidString
        let message: [String: Any] = [
            "type": "agent_interrupt",
            "id": requestId,
            "run_id": runId
        ]
        let messageData = try JSONSerialization.data(withJSONObject: message)
        var line = messageData
        line.append(0x0A)

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .utility) {
                do {
                    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
                    guard fd >= 0 else { throw DaemonError.daemonStopped }
                    defer { Darwin.close(fd) }

                    var addr = sockaddr_un()
                    addr.sun_family = sa_family_t(AF_UNIX)
                    withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
                        socketPath.withCString { cstr in
                            _ = Darwin.strcpy(
                                UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self),
                                cstr
                            )
                        }
                    }
                    let connectResult = withUnsafePointer(to: addr) { ptr in
                        Darwin.connect(fd, UnsafeRawPointer(ptr).assumingMemoryBound(to: sockaddr.self), socklen_t(MemoryLayout<sockaddr_un>.size))
                    }
                    guard connectResult == 0 else { throw DaemonError.daemonStopped }
                    _ = line.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    // MARK: - Streaming internals

    private static func performStreamingRun(
        socketPath: String,
        message: [String: Any],
        continuation: AsyncThrowingStream<AgentStreamEvent, Error>.Continuation
    ) throws {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonError.daemonStopped }
        defer { Darwin.close(fd) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            socketPath.withCString { cstr in
                _ = Darwin.strcpy(
                    UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self),
                    cstr
                )
            }
        }
        let connectResult = withUnsafePointer(to: addr) { ptr in
            Darwin.connect(fd, UnsafeRawPointer(ptr).assumingMemoryBound(to: sockaddr.self), socklen_t(MemoryLayout<sockaddr_un>.size))
        }
        guard connectResult == 0 else { throw DaemonError.daemonStopped }

        // Send the agent_run message
        let messageData = try JSONSerialization.data(withJSONObject: message)
        var messageLine = messageData
        messageLine.append(0x0A)
        _ = messageLine.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }

        // Read lines until socket closes or we get a DaemonResponse
        var lineBuffer = [UInt8]()
        var byte = UInt8(0)
        while Darwin.read(fd, &byte, 1) > 0 {
            if byte == 0x0A {
                guard !lineBuffer.isEmpty else { continue }
                let data = Data(lineBuffer)
                lineBuffer.removeAll(keepingCapacity: true)

                // Try as DaemonEventWire first
                if let event = try? JSONDecoder().decode(DaemonEventWire.self, from: data) {
                    continuation.yield(.event(event))
                    continue
                }

                // Try as final DaemonResponse (has "id" + "result"/"error" fields)
                if let outerJSON = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                    if let errorStr = outerJSON["error"] as? String, !errorStr.isEmpty {
                        continuation.yield(.error(errorStr))
                        continuation.finish()
                        return
                    }
                    if let resultStr = outerJSON["result"] as? String,
                       let resultData = resultStr.data(using: .utf8),
                       let result = try? JSONDecoder().decode(AgentRunResult.self, from: resultData) {
                        continuation.yield(.result(result))
                        continuation.finish()
                        return
                    }
                }

                // Unknown line — skip
            } else {
                lineBuffer.append(byte)
            }
        }
        continuation.finish()
    }
```

- [ ] **Step 2: Build to verify**

Run: `cd apps/menubar && swift build -c release 2>&1 | tail -3`

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerCore/DaemonClient.swift
git commit -m "feat(menubar): add streaming agent run + approval/interrupt to DaemonClient"
```

---

### Task 3: Agent State in AppState

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/AppState.swift`

- [ ] **Step 1: Add agent state types and properties**

Add the navigation case first — find the `PanelNavigation` enum at the top and add `.agent`:

```swift
enum PanelNavigation: Equatable {
    case overview
    case profile(String)
    case settings
    case addProfile
    case agent           // NEW
}
```

Add agent state types and properties to `AppState`. Insert these after the existing properties:

```swift
    // MARK: - Agent state

    /// UI-facing event item for the feed.
    struct AgentEventItem: Identifiable {
        let id = UUID()
        let timestamp = Date()
        let kind: AgentEventKind
    }

    enum AgentEventKind {
        case thinking(String)
        case toolCall(name: String, argsSummary: String)
        case toolResult(name: String, ok: Bool, summary: String)
        case progress(String)
        case done(String)
        case error(String)
    }

    struct ApprovalRequest {
        let runId: String
        let action: String
        let description: String
    }

    enum AgentRunState: Equatable {
        case idle
        case running
        case waitingApproval
        case completed
        case error

        static func == (lhs: AgentRunState, rhs: AgentRunState) -> Bool {
            switch (lhs, rhs) {
            case (.idle, .idle), (.running, .running),
                 (.waitingApproval, .waitingApproval),
                 (.completed, .completed), (.error, .error):
                return true
            default: return false
            }
        }
    }

    var agentState: AgentRunState = .idle
    var agentGoal: String = ""
    var agentProfile: String = ""
    var agentMode: AgentMode = .supervised
    var agentModel: String = "claude-haiku-4-5-20251001"
    var agentRunId: String?
    var agentEvents: [AgentEventItem] = []
    var agentSteps: Int = 0
    var agentTokens: Int = 0
    var agentSummary: String?
    var agentError: String?
    var agentApproval: ApprovalRequest?
    var agentStartTime: Date?
    var recentGoals: [RecentGoal] = []

    /// Active streaming task — cancelled on stop/new run.
    var agentStreamTask: Task<Void, Never>?
```

- [ ] **Step 2: Add agent methods**

Add these methods to AppState:

```swift
    // MARK: - Agent actions

    func startAgentRun(goal: String, client: DaemonClient) {
        // Cancel any existing run
        agentStreamTask?.cancel()

        // Reset state
        agentState = .running
        agentGoal = goal
        agentEvents = []
        agentSteps = 0
        agentTokens = 0
        agentSummary = nil
        agentError = nil
        agentApproval = nil
        agentStartTime = Date()
        agentRunId = nil

        let profile = agentProfile.isEmpty ? profiles.first?.name : agentProfile
        let mode = agentMode

        agentStreamTask = Task { @MainActor [weak self] in
            guard let self else { return }
            let stream = client.streamAgentRun(
                goal: goal,
                profile: profile,
                model: nil,
                maxSteps: 15,
                mode: mode
            )
            do {
                for try await item in stream {
                    guard !Task.isCancelled else { break }
                    switch item {
                    case .event(let wire):
                        self.handleAgentEvent(wire.event)
                        self.agentRunId = wire.runId
                    case .result(let result):
                        self.agentSteps = result.totalSteps ?? self.agentSteps
                        self.agentTokens = (result.inputTokens ?? 0) + (result.outputTokens ?? 0)
                        if result.outcome == "completed" {
                            self.agentState = .completed
                            self.agentSummary = result.summary
                        } else {
                            self.agentState = .error
                            self.agentError = result.summary ?? result.outcome
                        }
                        self.saveToHistory(outcome: result.outcome)
                    case .error(let msg):
                        self.agentState = .error
                        self.agentError = msg
                        self.saveToHistory(outcome: "error")
                    }
                }
            } catch {
                if !Task.isCancelled {
                    self.agentState = .error
                    self.agentError = error.localizedDescription
                    self.saveToHistory(outcome: "error")
                }
            }
        }
    }

    private func handleAgentEvent(_ event: AgentEventWire) {
        switch event.type {
        case "thinking":
            if let text = event.text, !text.isEmpty {
                agentEvents.append(AgentEventItem(kind: .thinking(text)))
            }
        case "tool_call":
            agentSteps += 1
            let name = event.name ?? "unknown"
            let argsSummary = event.args?.value as? String ?? ""
            agentEvents.append(AgentEventItem(kind: .toolCall(name: name, argsSummary: argsSummary)))
        case "tool_result":
            let name = event.name ?? "unknown"
            let ok = !(event.isError ?? false)
            let summary = event.result.map { s in
                s.count > 120 ? String(s.prefix(117)) + "..." : s
            } ?? ""
            agentEvents.append(AgentEventItem(kind: .toolResult(name: name, ok: ok, summary: summary)))
        case "progress":
            if let msg = event.message {
                agentEvents.append(AgentEventItem(kind: .progress(msg)))
            }
        case "approval_required":
            agentState = .waitingApproval
            agentApproval = ApprovalRequest(
                runId: event.runId ?? agentRunId ?? "",
                action: event.action ?? "unknown",
                description: event.description ?? ""
            )
        case "done":
            agentSummary = event.summary
            agentEvents.append(AgentEventItem(kind: .done(event.summary ?? "Done")))
        case "error":
            agentEvents.append(AgentEventItem(kind: .error(event.message ?? "Unknown error")))
        case "budget_exceeded":
            agentEvents.append(AgentEventItem(kind: .error("Budget: \(event.reason ?? "exceeded")")))
        case "interrupted":
            agentEvents.append(AgentEventItem(kind: .error("Interrupted")))
        default:
            break
        }
    }

    func approveAgent(approved: Bool, client: DaemonClient) {
        guard let approval = agentApproval else { return }
        agentApproval = nil
        agentState = .running
        Task {
            try? await client.sendApproval(runId: approval.runId, approved: approved)
        }
    }

    func stopAgent(client: DaemonClient) {
        if let runId = agentRunId {
            Task { try? await client.sendInterrupt(runId: runId) }
        }
        agentStreamTask?.cancel()
        agentStreamTask = nil
        agentState = .idle
    }

    func resetAgent() {
        agentStreamTask?.cancel()
        agentStreamTask = nil
        agentState = .idle
        agentGoal = ""
        agentEvents = []
        agentSteps = 0
        agentTokens = 0
        agentSummary = nil
        agentError = nil
        agentApproval = nil
        agentRunId = nil
    }

    // MARK: - History

    func loadAgentHistory(client: DaemonClient) {
        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                let result = try await client.call(tool: "kv_get", args: ["namespace": "agent-history", "key": "recent"])
                if let valueStr = result["value"]?.stringValue,
                   let data = valueStr.data(using: .utf8),
                   let goals = try? JSONDecoder().decode([RecentGoal].self, from: data) {
                    self.recentGoals = goals
                }
            } catch {
                // No history yet — that's fine
            }
        }
    }

    private func saveToHistory(outcome: String) {
        let duration = agentStartTime.map { Date().timeIntervalSince($0) } ?? 0
        let entry = RecentGoal(
            goal: agentGoal,
            profile: agentProfile,
            timestamp: Date(),
            duration: duration,
            steps: agentSteps,
            outcome: outcome
        )
        recentGoals.insert(entry, at: 0)
        if recentGoals.count > 20 { recentGoals = Array(recentGoals.prefix(20)) }

        // Persist to KV store (fire-and-forget)
        if let data = try? JSONEncoder().encode(recentGoals),
           let json = String(data: data, encoding: .utf8) {
            let home = FileManager.default.homeDirectoryForCurrentUser.path
            let client = DaemonClient(socketPath: "\(home)/.pagerunner/daemon.sock")
            Task {
                _ = try? await client.call(tool: "kv_set", args: [
                    "namespace": "agent-history",
                    "key": "recent",
                    "value": json
                ])
            }
        }
    }
```

- [ ] **Step 3: Build to verify**

Run: `cd apps/menubar && swift build -c release 2>&1 | tail -5`

- [ ] **Step 4: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/AppState.swift
git commit -m "feat(menubar): add agent run state, event handling, and history to AppState"
```

---

### Task 4: Agent Tab in Navigation Strip

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/PanelView.swift`

- [ ] **Step 1: Add 🤖 tab to NavigationStrip**

In the `NavigationStrip` struct, after the `ScrollView` with profile tabs and before the closing `}` of the outer `HStack`, add the agent tab:

```swift
            // Agent tab — always last
            Rectangle().fill(Color.primary.opacity(0.1)).frame(width: 0.5)
                .padding(.vertical, 6)

            Button {
                appState.navigation = .agent
            } label: {
                VStack(spacing: 3) {
                    ZStack {
                        Text("🤖")
                            .font(.system(size: 18))
                        // Pulsing dot when running
                        if appState.agentState == .running {
                            Circle()
                                .fill(Color(red: 0, green: 0.478, blue: 1))
                                .frame(width: 6, height: 6)
                                .offset(x: 10, y: -8)
                                .opacity(0.9)
                        } else if appState.agentState == .waitingApproval {
                            Circle()
                                .fill(Color(red: 0.961, green: 0.620, blue: 0.043))
                                .frame(width: 6, height: 6)
                                .offset(x: 10, y: -8)
                        }
                    }
                    Text("Agent")
                        .font(.system(size: 10))
                        .foregroundColor(appState.navigation == .agent
                                         ? Color(red: 0, green: 0.478, blue: 1)
                                         : Color(red: 0.4, green: 0.4, blue: 0.4))
                        .fontWeight(appState.navigation == .agent ? .medium : .regular)
                }
                .frame(minWidth: 50)
                .padding(.vertical, 5)
                .frame(maxHeight: .infinity)
                .contentShape(Rectangle())
                .overlay(alignment: .bottom) {
                    if appState.navigation == .agent {
                        Rectangle()
                            .fill(Color(red: 0, green: 0.478, blue: 1))
                            .frame(height: 2)
                    }
                }
            }
            .buttonStyle(.plain)
            .help("Pagerunner Agent")
```

- [ ] **Step 2: Add .agent case to PanelView content switch**

In `PanelView`, find the `switch appState.navigation` block inside the `ScrollView` and add:

```swift
                        case .agent:
                            AgentView(appState: appState)
```

- [ ] **Step 3: Build to verify** (will fail — AgentView doesn't exist yet, that's expected)

- [ ] **Step 4: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/PanelView.swift
git commit -m "feat(menubar): add 🤖 Agent tab to navigation strip"
```

---

### Task 5: AgentIdleView

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/Views/AgentIdleView.swift`

- [ ] **Step 1: Create the idle view**

```swift
import SwiftUI
import PagerunnerCore

struct AgentIdleView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client

    @State private var goalText: String = ""

    var body: some View {
        VStack(spacing: 16) {
            Spacer().frame(height: 12)

            // Branding
            Text("🤖")
                .font(.system(size: 36))
            Text("Pagerunner Agent")
                .font(.system(size: 16, weight: .semibold))
                .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))

            // Goal input
            TextField("What should I browse?", text: $goalText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .padding(10)
                .lineLimit(1...3)
                .background(Color.white)
                .cornerRadius(8)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color.primary.opacity(0.15), lineWidth: 0.5)
                )
                .padding(.horizontal, 16)
                .onSubmit {
                    if !goalText.isEmpty {
                        startRun()
                    }
                }

            // Profile + Mode pickers
            HStack(spacing: 8) {
                // Profile picker
                Picker("", selection: $appState.agentProfile) {
                    ForEach(appState.profiles) { profile in
                        Text(profile.name).tag(profile.name)
                    }
                }
                .labelsHidden()
                .frame(maxWidth: .infinity)

                // Mode picker
                Picker("", selection: $appState.agentMode) {
                    ForEach(AgentMode.allCases, id: \.self) { mode in
                        Text(mode.label).tag(mode)
                    }
                }
                .labelsHidden()
                .frame(maxWidth: .infinity)

                // Run button
                Button(action: startRun) {
                    Text("Run ▶")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(.white)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 5)
                        .background(goalText.isEmpty
                            ? Color.gray
                            : Color(red: 0, green: 0.478, blue: 1))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .disabled(goalText.isEmpty)
            }
            .padding(.horizontal, 16)

            Divider().padding(.horizontal, 16)

            // Recent goals
            if !appState.recentGoals.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Recent")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                        .padding(.horizontal, 16)

                    ForEach(appState.recentGoals.prefix(8)) { recent in
                        Button {
                            goalText = recent.goal
                        } label: {
                            HStack(spacing: 6) {
                                Text("↻")
                                    .font(.system(size: 11))
                                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                                Text(recent.goal)
                                    .font(.system(size: 12))
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                Spacer()
                                Text(formatDuration(recent.duration))
                                    .font(.system(size: 11))
                                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            }
                            .padding(.horizontal, 16)
                            .padding(.vertical, 4)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            Spacer()

            // Model badge
            Text("Model: \(appState.agentModel)")
                .font(.system(size: 10))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                .padding(.bottom, 8)
        }
        .onAppear {
            if appState.agentProfile.isEmpty, let first = appState.profiles.first {
                appState.agentProfile = first.name
            }
            if let client {
                appState.loadAgentHistory(client: client)
            }
        }
    }

    private func startRun() {
        guard !goalText.isEmpty, let client else { return }
        let goal = goalText
        goalText = ""
        appState.startAgentRun(goal: goal, client: client)
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%d:%02d", mins, secs)
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/AgentIdleView.swift
git commit -m "feat(menubar): add AgentIdleView — goal input, profile/mode pickers, recent history"
```

---

### Task 6: AgentFeedView + AgentApprovalCard

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/Views/AgentFeedView.swift`
- Create: `apps/menubar/Sources/PagerunnerBar/Views/AgentApprovalCard.swift`

- [ ] **Step 1: Create AgentApprovalCard**

```swift
import SwiftUI

struct AgentApprovalCard: View {
    let action: String
    let description: String
    let onApprove: () -> Void
    let onDeny: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 4) {
                Text("⚠")
                    .font(.system(size: 13))
                Text("Approval Required")
                    .font(.system(size: 12, weight: .semibold))
            }
            .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))

            Text(description)
                .font(.system(size: 12))
                .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
                .lineLimit(3)

            HStack(spacing: 12) {
                Button(action: onApprove) {
                    Text("Approve")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(.white)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 5)
                        .background(Color(red: 0.133, green: 0.773, blue: 0.369))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)

                Button(action: onDeny) {
                    Text("Deny")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                        .padding(.horizontal, 16)
                        .padding(.vertical, 5)
                        .background(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.1))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(12)
        .background(Color(red: 0.961, green: 0.620, blue: 0.043).opacity(0.08))
        .cornerRadius(8)
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color(red: 0.961, green: 0.620, blue: 0.043).opacity(0.3), lineWidth: 1)
        )
        .padding(.horizontal, 12)
    }
}
```

- [ ] **Step 2: Create AgentFeedView**

```swift
import SwiftUI
import PagerunnerCore

struct AgentFeedView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client

    var body: some View {
        VStack(spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text("🤖 Pagerunner Agent")
                        .font(.system(size: 14, weight: .semibold))
                    Spacer()
                }
                Text("Using \(appState.agentModel) · \(appState.agentProfile)")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Divider()

            // Event feed
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(appState.agentEvents) { event in
                            AgentEventRow(kind: event.kind)
                                .id(event.id)
                        }

                        // Approval card (inline)
                        if let approval = appState.agentApproval {
                            AgentApprovalCard(
                                action: approval.action,
                                description: approval.description,
                                onApprove: { if let client { appState.approveAgent(approved: true, client: client) } },
                                onDeny: { if let client { appState.approveAgent(approved: false, client: client) } }
                            )
                            .id("approval")
                        }

                        // Spinner for running state
                        if appState.agentState == .running {
                            HStack(spacing: 6) {
                                ProgressView()
                                    .scaleEffect(0.6)
                                    .frame(width: 12, height: 12)
                                Text("Working...")
                                    .font(.system(size: 11))
                                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            }
                            .padding(.horizontal, 12)
                            .id("spinner")
                        }
                    }
                    .padding(.vertical, 8)
                }
                .onChange(of: appState.agentEvents.count) { _, _ in
                    withAnimation(.easeOut(duration: 0.2)) {
                        if let last = appState.agentEvents.last {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        } else if appState.agentState == .running {
                            proxy.scrollTo("spinner", anchor: .bottom)
                        }
                    }
                }
            }

            Divider()

            // Bottom bar
            bottomBar
        }
    }

    @ViewBuilder
    private var bottomBar: some View {
        switch appState.agentState {
        case .running:
            HStack {
                Text("Step \(appState.agentSteps)/15 · \(formatTokens(appState.agentTokens))")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                Spacer()
                Button("Stop") {
                    if let client { appState.stopAgent(client: client) }
                }
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

        case .waitingApproval:
            HStack {
                Text("⏸ Waiting for approval...")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))
                Spacer()
                Button("Stop") {
                    if let client { appState.stopAgent(client: client) }
                }
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

        case .completed:
            VStack(spacing: 6) {
                HStack {
                    Text("✓ Done · \(appState.agentSteps) steps · \(formatTokens(appState.agentTokens))")
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0.133, green: 0.773, blue: 0.369))
                    Spacer()
                }

                HStack(spacing: 8) {
                    Button("New Goal") { appState.resetAgent() }
                        .font(.system(size: 12, weight: .medium))
                        .buttonStyle(.plain)
                    Spacer()
                    if let summary = appState.agentSummary {
                        Button("Copy Result") {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(summary, forType: .string)
                        }
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                        .buttonStyle(.plain)
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

        case .error:
            VStack(spacing: 6) {
                HStack {
                    Text("✗ Failed · \(appState.agentSteps) steps")
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                    Spacer()
                }
                HStack(spacing: 8) {
                    Button("Retry") {
                        if let client {
                            let goal = appState.agentGoal
                            appState.startAgentRun(goal: goal, client: client)
                        }
                    }
                    .font(.system(size: 12, weight: .medium))
                    .buttonStyle(.plain)
                    Spacer()
                    Button("New Goal") { appState.resetAgent() }
                        .font(.system(size: 12, weight: .medium))
                        .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

        case .idle:
            EmptyView()
        }
    }

    private func formatTokens(_ tokens: Int) -> String {
        if tokens >= 1000 {
            return "\(tokens / 1000)K tokens"
        }
        return "\(tokens) tokens"
    }
}

// MARK: - Event row

struct AgentEventRow: View {
    let kind: AppState.AgentEventKind

    var body: some View {
        switch kind {
        case .thinking(let text):
            HStack(alignment: .top, spacing: 6) {
                Text("💭")
                    .font(.system(size: 12))
                Text(text)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
            }
            .padding(.horizontal, 12)

        case .toolCall(let name, _):
            HStack(spacing: 6) {
                Text("▸")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))
                Text(name)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
            }
            .padding(.horizontal, 12)

        case .toolResult(_, let ok, let summary):
            HStack(alignment: .top, spacing: 6) {
                Text(ok ? "✓" : "✗")
                    .font(.system(size: 12))
                    .foregroundColor(ok
                        ? Color(red: 0.133, green: 0.773, blue: 0.369)
                        : Color(red: 0.937, green: 0.267, blue: 0.267))
                Text(summary)
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    .lineLimit(2)
            }
            .padding(.horizontal, 12)

        case .progress(let msg):
            Text(msg)
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                .padding(.horizontal, 12)

        case .done(let summary):
            HStack(alignment: .top, spacing: 6) {
                Text("💭")
                    .font(.system(size: 12))
                Text(summary)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
            }
            .padding(.horizontal, 12)

        case .error(let msg):
            HStack(alignment: .top, spacing: 6) {
                Text("✗")
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                Text(msg)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
            }
            .padding(.horizontal, 12)
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/AgentFeedView.swift apps/menubar/Sources/PagerunnerBar/Views/AgentApprovalCard.swift
git commit -m "feat(menubar): add AgentFeedView with event feed + AgentApprovalCard"
```

---

### Task 7: AgentView (Top-Level Router)

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/Views/AgentView.swift`

- [ ] **Step 1: Create AgentView**

```swift
import SwiftUI
import PagerunnerCore

/// Top-level agent view — routes between idle, running, completed, and error states.
struct AgentView: View {
    @Bindable var appState: AppState

    var body: some View {
        switch appState.agentState {
        case .idle:
            AgentIdleView(appState: appState)
        case .running, .waitingApproval, .completed, .error:
            AgentFeedView(appState: appState)
        }
    }
}
```

- [ ] **Step 2: Build the full app**

Run: `cd apps/menubar && swift build -c release 2>&1 | tail -5`
Expected: builds successfully.

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/AgentView.swift
git commit -m "feat(menubar): add AgentView router — idle/running/completed/error states"
```

---

### Task 8: Agent Notifications

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/NotificationService.swift`

- [ ] **Step 1: Add agent notification categories and methods**

In `registerCategories()`, add after the existing categories:

```swift
            let approve = UNNotificationAction(identifier: "APPROVE", title: "Approve", options: [])
            let deny = UNNotificationAction(identifier: "DENY", title: "Deny", options: [.destructive])
```

And add these category registrations:

```swift
            UNNotificationCategory(identifier: "AGENT_APPROVAL", actions: [approve, deny], intentIdentifiers: []),
            UNNotificationCategory(identifier: "AGENT_DONE",     actions: [view],            intentIdentifiers: []),
            UNNotificationCategory(identifier: "AGENT_ERROR",    actions: [view],            intentIdentifiers: []),
```

Add notification methods:

```swift
    func notifyAgentApproval(action: String, description: String, runId: String) {
        let content = UNMutableNotificationContent()
        content.title = "🤖 Pagerunner Agent"
        content.body = "Wants to: \(description)"
        content.sound = .default
        content.categoryIdentifier = "AGENT_APPROVAL"
        content.userInfo = ["run_id": runId, "action": action]
        schedule(content, id: "agent-approval-\(runId)")
    }

    func notifyAgentDone(summary: String) {
        let content = UNMutableNotificationContent()
        content.title = "🤖 Agent completed"
        content.body = summary.prefix(200).description
        content.sound = .default
        content.categoryIdentifier = "AGENT_DONE"
        schedule(content, id: "agent-done-\(UUID().uuidString)")
    }

    func notifyAgentError(message: String) {
        let content = UNMutableNotificationContent()
        content.title = "🤖 Agent failed"
        content.body = message.prefix(200).description
        content.sound = .default
        content.categoryIdentifier = "AGENT_ERROR"
        schedule(content, id: "agent-error-\(UUID().uuidString)")
    }
```

In the notification response handler (`userNotificationCenter(_:didReceive:)`), add handling for AGENT_APPROVAL:

```swift
        case "AGENT_APPROVAL":
            if let runId = response.notification.request.content.userInfo["run_id"] as? String {
                let approved = response.actionIdentifier == "APPROVE"
                let client = DaemonClient()
                Task { try? await client.sendApproval(runId: runId, approved: approved) }
            }
```

- [ ] **Step 2: Build to verify**

Run: `cd apps/menubar && swift build -c release 2>&1 | tail -3`

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/NotificationService.swift
git commit -m "feat(menubar): add agent notification categories — approval, done, error"
```

---

### Task 9: Package, Test, and Polish

**Files:** None new — integration testing and fixes.

- [ ] **Step 1: Build release**

```bash
cd apps/menubar && swift build -c release
```

- [ ] **Step 2: Package the app**

```bash
cd apps/menubar/scripts && ./package.sh
```

- [ ] **Step 3: Launch and test manually**

```bash
open Pagerunner.app
```

Test flow:
1. Click menu bar icon → see navigation strip with 🤖 Agent tab
2. Click Agent → see idle state with input, profile picker, mode picker
3. Type "Go to example.com and describe it" → click Run
4. Watch event feed populate in real-time
5. Verify completed state shows summary, copy button, new goal
6. Click New Goal → back to idle, verify recent history shows the run
7. Re-run from recent list
8. Test Stop button during a run
9. Test with Supervised mode — verify approval card appears

- [ ] **Step 4: Fix any build or runtime issues discovered during testing**

- [ ] **Step 5: Commit all fixes**

```bash
git add apps/menubar/
git commit -m "feat(menubar): agent UI complete — idle, feed, approval, notifications"
```
