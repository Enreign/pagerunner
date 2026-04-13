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
    case agent
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

    // MARK: - Voice state

    enum VoiceStatus: Equatable {
        case idle
        case starting
        case listening
        case processing
        case speaking
    }

    enum VoiceMode: String, CaseIterable, Sendable {
        case alwaysListening = "always"
        case pushToTalk = "ptt"

        var label: String {
            switch self {
            case .alwaysListening: return "Always"
            case .pushToTalk: return "Push-to-Talk"
            }
        }
    }

    enum NarrationMode: String, CaseIterable, Sendable {
        case full = "full"
        case summary = "summary"
        case off = "off"

        var label: String {
            switch self {
            case .full: return "Full"
            case .summary: return "Summary"
            case .off: return "Off"
            }
        }
    }

    var voiceActive: Bool = false
    var voiceProcess: Process?
    var voiceStatus: VoiceStatus = .idle
    var voiceMode: VoiceMode = .alwaysListening
    var narrationMode: NarrationMode = .summary
    var voiceMuted: Bool = false {
        didSet {
            guard voiceActive, let pipe = voiceInputPipe else { return }
            let cmd = voiceMuted ? "{\"type\":\"mute\"}\n" : "{\"type\":\"unmute\"}\n"
            if let data = cmd.data(using: .utf8) {
                pipe.fileHandleForWriting.write(data)
            }
        }
    }
    /// Stdin pipe to the voice sidecar (for PTT commands).
    var voiceInputPipe: Pipe?
    /// Background task reading voice sidecar stdout.
    var voiceReadTask: Task<Void, Never>?

    func startVoice() {
        guard !voiceActive, let binary = binaryPath else { return }
        voiceActive = true
        voiceStatus = .starting

        let voiceBinaryPath: String
        if binary.hasSuffix("/pagerunner") {
            voiceBinaryPath = String(binary.dropLast("/pagerunner".count)) + "/pagerunner-voice"
        } else {
            voiceBinaryPath = binary + "-voice"
        }

        let profile = agentProfile.isEmpty ? profiles.first?.name ?? "personal" : agentProfile

        let process = Process()
        process.executableURL = URL(fileURLWithPath: voiceBinaryPath)
        process.arguments = [
            "--profile", profile,
            "--json",
            "--mode", voiceMode.rawValue,
            "--narration", narrationMode.rawValue,
        ]

        let outPipe = Pipe()
        process.standardOutput = outPipe
        process.standardError = FileHandle.nullDevice

        let inPipe = Pipe()
        process.standardInput = inPipe
        voiceInputPipe = inPipe

        voiceProcess = process

        voiceReadTask = Task { [weak self] in
            do {
                try process.run()
            } catch {
                await MainActor.run {
                    self?.voiceActive = false
                    self?.voiceStatus = .idle
                }
                return
            }

            let handle = outPipe.fileHandleForReading
            do {
                for try await line in handle.bytes.lines {
                    guard let self, !Task.isCancelled else { break }
                    await MainActor.run {
                        self.handleVoiceEvent(line)
                    }
                }
            } catch {
                // Stream ended or read error — fall through to cleanup
            }

            // Process ended
            await MainActor.run {
                self?.voiceActive = false
                self?.voiceStatus = .idle
            }
        }
    }

    func stopVoice() {
        voiceReadTask?.cancel()
        voiceReadTask = nil
        voiceProcess?.terminate()
        voiceProcess = nil
        voiceInputPipe = nil
        voiceActive = false
        voiceStatus = .idle
    }

    /// PTT: send start_listening command to sidecar stdin.
    func voicePushToTalkStart() {
        guard voiceActive, let pipe = voiceInputPipe else { return }
        if let data = "{\"type\":\"start_listening\"}\n".data(using: .utf8) {
            pipe.fileHandleForWriting.write(data)
        }
    }

    /// PTT: send stop_listening command to sidecar stdin.
    func voicePushToTalkStop() {
        guard voiceActive, let pipe = voiceInputPipe else { return }
        if let data = "{\"type\":\"stop_listening\"}\n".data(using: .utf8) {
            pipe.fileHandleForWriting.write(data)
        }
    }

    /// Send text to the voice sidecar for TTS playback (e.g. replay result).
    func voiceReplay(text: String) {
        guard voiceActive, let pipe = voiceInputPipe else { return }
        // JSON-escape the text by encoding it as a JSON string value
        if let textData = try? JSONSerialization.data(withJSONObject: text),
           let escapedText = String(data: textData, encoding: .utf8) {
            let cmd = "{\"type\":\"speak\",\"text\":\(escapedText)}\n"
            if let data = cmd.data(using: .utf8) {
                pipe.fileHandleForWriting.write(data)
            }
        }
    }

    private func handleVoiceEvent(_ line: String) {
        guard let data = line.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let eventType = json["type"] as? String else { return }

        let eventData = json["data"] as? [String: Any]

        switch eventType {
        case "listening":
            voiceStatus = .listening
        case "utterance":
            if let text = eventData?["text"] as? String {
                agentGoal = text
                voiceStatus = .processing
            }
        case "agent_event":
            if let innerEvent = eventData?["event"] as? [String: Any] {
                // Decode the inner event as AgentEventWire
                if let wireData = try? JSONSerialization.data(withJSONObject: innerEvent),
                   let wire = try? JSONDecoder().decode(AgentEventWire.self, from: wireData) {
                    // Auto-transition to running on first event
                    if agentState == .idle {
                        agentState = .running
                        agentStartTime = Date()
                        agentEvents = []
                        agentSteps = 0
                        agentTokens = 0
                        agentSummary = nil
                        agentError = nil
                        agentApproval = nil
                    }
                    handleAgentEvent(wire)
                }
            }
        case "speaking":
            voiceStatus = .speaking
        case "idle":
            voiceStatus = voiceMode == .pushToTalk ? .idle : .listening
            // If agent was running, mark completed
            if agentState == .running || agentState == .completed {
                if agentState != .completed && agentState != .error {
                    agentState = .completed
                }
            }
        case "approval":
            voiceStatus = .listening
        case "approval_response":
            voiceStatus = .processing
        case "error":
            if let message = eventData?["message"] as? String {
                agentState = .error
                agentError = message
            }
            stopVoice()
        default:
            break
        }
    }

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
            let argsSummary = event.args?.stringValue ?? ""
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
        stopVoice()
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

    // MARK: - Agent history

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
            let client = DaemonClient()
            Task {
                _ = try? await client.call(tool: "kv_set", args: [
                    "namespace": "agent-history",
                    "key": "recent",
                    "value": json
                ])
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
