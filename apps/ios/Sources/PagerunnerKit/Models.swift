import Foundation

// MARK: - Error types

/// Errors thrown by PagerunnerKit operations.
public enum PagerunnerError: Error, Sendable, LocalizedError {
    case unauthorized
    case connectionFailed(String)
    case requestFailed(statusCode: Int, message: String)
    case decodingFailed(String)
    case daemonError(String)
    case websocketDisconnected
    case invalidURL(String)

    public var errorDescription: String? {
        switch self {
        case .unauthorized:
            return "Unauthorized: invalid or missing bearer token"
        case .connectionFailed(let msg):
            return "Connection failed: \(msg)"
        case .requestFailed(let code, let msg):
            return "Request failed (\(code)): \(msg)"
        case .decodingFailed(let msg):
            return "Decoding failed: \(msg)"
        case .daemonError(let msg):
            return "Daemon error: \(msg)"
        case .websocketDisconnected:
            return "WebSocket is not connected"
        case .invalidURL(let url):
            return "Invalid URL: \(url)"
        }
    }
}

// MARK: - Health

public struct HealthResponse: Codable, Sendable {
    public let status: String
    public let version: String
}

// MARK: - Auth info

public enum AuthMode: String, Codable, Sendable {
    case token
    case tailscale
}

public struct AuthInfoResponse: Codable, Sendable {
    public let mode: AuthMode
}

// MARK: - Profile

public struct Profile: Codable, Identifiable, Sendable, Hashable {
    public var id: String { name }
    public let name: String
    public let displayName: String
    public let kind: String?
    public let userDataDir: String?
    public let debugPort: Int?

    enum CodingKeys: String, CodingKey {
        case name, kind
        case displayName = "display_name"
        case userDataDir = "user_data_dir"
        case debugPort = "debug_port"
    }
}

// MARK: - Session

public enum SessionStatus: String, Codable, Sendable, Hashable {
    case alive
    case crashed
    case reconnecting
    case recovering
}

public struct Session: Codable, Identifiable, Sendable, Hashable {
    public let id: String
    public let profile: String
    public let displayName: String
    public let stealth: Bool
    public let status: SessionStatus

    public var isAlive: Bool { status == .alive }

    enum CodingKeys: String, CodingKey {
        case id, profile, stealth, status
        case displayName = "display_name"
    }

    public init(id: String, profile: String, displayName: String, stealth: Bool, status: SessionStatus) {
        self.id = id
        self.profile = profile
        self.displayName = displayName
        self.stealth = stealth
        self.status = status
    }
}

// MARK: - Tab

public struct Tab: Codable, Identifiable, Sendable {
    public var id: String { targetId }
    public let targetId: String
    public let url: String
    public let title: String

    enum CodingKeys: String, CodingKey {
        case url, title
        case targetId = "target_id"
    }
}

// MARK: - Checkpoint

public struct Checkpoint: Codable, Identifiable, Sendable {
    public var id: String { checkpointId }
    public let checkpointId: String
    public let name: String
    public let savedAt: Int
    public let profile: String
    public let tabCount: Int
    public let origins: [String]

    enum CodingKeys: String, CodingKey {
        case name, profile, origins
        case checkpointId = "checkpoint_id"
        case savedAt = "saved_at"
        case tabCount = "tab_count"
    }
}

// MARK: - Network log

public struct NetworkLogEntry: Codable, Identifiable, Sendable {
    public var id: String { "\(requestId)-\(timestampMs)" }
    public let requestId: String
    public let url: String
    public let method: String
    public let status: UInt16
    public let durationMs: UInt64
    public let timestampMs: UInt64
    public let requestHeaders: [String: String]?
    public let requestBody: String?
    public let responseBody: String?
    public let responseTruncated: Bool?
    public let tabId: String

    public init(requestId: String, url: String, method: String, status: UInt16, durationMs: UInt64, timestampMs: UInt64, requestHeaders: [String: String]? = nil, requestBody: String? = nil, responseBody: String? = nil, responseTruncated: Bool? = nil, tabId: String) {
        self.requestId = requestId; self.url = url; self.method = method; self.status = status
        self.durationMs = durationMs; self.timestampMs = timestampMs; self.requestHeaders = requestHeaders
        self.requestBody = requestBody; self.responseBody = responseBody; self.responseTruncated = responseTruncated
        self.tabId = tabId
    }

    enum CodingKeys: String, CodingKey {
        case url, method, status
        case requestId = "request_id"
        case durationMs = "duration_ms"
        case timestampMs = "timestamp_ms"
        case requestHeaders = "request_headers"
        case requestBody = "request_body"
        case responseBody = "response_body"
        case responseTruncated = "response_truncated"
        case tabId = "tab_id"
    }
}

public struct NetworkLogResult: Codable, Sendable {
    public let ok: Bool
    public let entries: [NetworkLogEntry]
    public let totalMatched: Int
    public let totalCaptured: Int
    public let resultTruncated: Bool

    enum CodingKeys: String, CodingKey {
        case ok, entries
        case totalMatched = "total_matched"
        case totalCaptured = "total_captured"
        case resultTruncated = "result_truncated"
    }
}

// MARK: - Console log

public struct ConsoleEntry: Codable, Identifiable, Sendable {
    public var id: String { "\(tabId)-\(timestampMs)-\(text.prefix(30))" }
    public let level: String
    public let text: String
    public let url: String?
    public let line: UInt32?
    public let timestampMs: UInt64
    public let tabId: String

    enum CodingKeys: String, CodingKey {
        case level, text, url, line
        case timestampMs = "timestamp_ms"
        case tabId = "tab_id"
    }
}

public struct ExceptionEntry: Codable, Identifiable, Sendable {
    public var id: String { "\(tabId)-\(timestampMs)-\(text.prefix(30))" }
    public let text: String
    public let url: String?
    public let line: UInt32?
    public let timestampMs: UInt64
    public let tabId: String

    enum CodingKeys: String, CodingKey {
        case text, url, line
        case timestampMs = "timestamp_ms"
        case tabId = "tab_id"
    }
}

public struct ConsoleLogResult: Codable, Sendable {
    public let ok: Bool
    public let consoleErrors: [ConsoleEntry]
    public let exceptions: [ExceptionEntry]

    enum CodingKeys: String, CodingKey {
        case ok
        case consoleErrors = "console_errors"
        case exceptions
    }
}

// MARK: - Recording

public struct Recording: Codable, Identifiable, Sendable {
    public var id: String { recordingId }
    public let recordingId: String
    public let profile: String
    public let flow: String?
    public let name: String?
    public let tags: [String]
    public let startedAt: String
    public let durationMs: UInt64?
    public let format: String

    enum CodingKeys: String, CodingKey {
        case profile, flow, name, tags, format
        case recordingId = "recording_id"
        case startedAt = "started_at"
        case durationMs = "duration_ms"
    }
}

// MARK: - Notification

public struct DaemonNotification: Codable, Identifiable, Sendable {
    public var id: String { notificationId }
    public let notificationId: String
    public let title: String
    public let body: String?
    public let level: String
    public let sessionId: String?
    public let profileName: String?
    public let createdAt: UInt64

    enum CodingKeys: String, CodingKey {
        case title, body, level
        case notificationId = "id"
        case sessionId = "session_id"
        case profileName = "profile_name"
        case createdAt = "created_at"
    }
}

// MARK: - Tool call response

public struct ToolCallResponse: Codable, Sendable {
    public let ok: Bool
    public let result: AnyCodableValue?
    public let error: String?

    public init(ok: Bool, result: AnyCodableValue?, error: String?) {
        self.ok = ok
        self.result = result
        self.error = error
    }
}

// MARK: - Response envelopes

/// List endpoints wrap results in `{"data": [...]}`.
public struct DataEnvelope<T: Codable & Sendable>: Codable, Sendable {
    public let data: T
}

/// Notification list endpoint wraps results in `{"notifications": [...]}`.
public struct NotificationsEnvelope: Codable, Sendable {
    public let notifications: [DaemonNotification]
}

// MARK: - WebSocket message types

public enum WSEventType: String, Codable, Sendable {
    case agentEvent = "agent_event"
    case notification = "notification"
    case sessionStatus = "session_status"
    case toolResult = "tool_result"
}

public struct WSMessage: Codable, Sendable {
    public let type: WSEventType
    public let data: AnyCodableValue?
    public let ok: Bool?
    public let result: AnyCodableValue?
    public let error: String?

    enum CodingKeys: String, CodingKey {
        case type, data, ok, result, error
    }
}

// MARK: - Agent events

public struct AgentEvent: Codable, Sendable {
    public let runId: String
    public let event: AgentEventDetail

    enum CodingKeys: String, CodingKey {
        case event
        case runId = "run_id"
    }
}

/// Matches the Rust `AgentEvent` enum which uses `#[serde(tag = "type", rename_all = "snake_case")]`.
public enum AgentEventDetail: Codable, Sendable {
    case thinking(text: String)
    case toolCall(name: String, args: AnyCodableValue)
    case toolResult(name: String, result: String, isError: Bool)
    case progress(message: String)
    case approvalRequired(runId: String, action: String, description: String)
    case approvalResponse(runId: String, approved: Bool)
    case done(summary: String)
    case error(message: String, recoverable: Bool)
    case interrupted
    case budgetExceeded(reason: String)
    case scopeDigest(sessionId: String, targetId: String?, digest: String)
    case turnSummary(summary: String, touchedTabIds: [String])
    case unknown(type: String)

    private enum TypeKey: String, CodingKey {
        case type
    }

    private enum EventType: String, Codable {
        case thinking
        case toolCall = "tool_call"
        case toolResult = "tool_result"
        case progress
        case approvalRequired = "approval_required"
        case approvalResponse = "approval_response"
        case done
        case error
        case interrupted
        case budgetExceeded = "budget_exceeded"
        case scopeDigest = "scope_digest"
        case turnSummary = "turn_summary"
    }

    // -- Payload keys per variant --

    private enum ThinkingKeys: String, CodingKey {
        case type, text
    }

    private enum ToolCallKeys: String, CodingKey {
        case type, name, args
    }

    private enum ToolResultKeys: String, CodingKey {
        case type, name, result
        case isError = "is_error"
    }

    private enum ProgressKeys: String, CodingKey {
        case type, message
    }

    private enum ApprovalRequiredKeys: String, CodingKey {
        case type, action, description
        case runId = "run_id"
    }

    private enum ApprovalResponseKeys: String, CodingKey {
        case type, approved
        case runId = "run_id"
    }

    private enum DoneKeys: String, CodingKey {
        case type, summary
    }

    private enum ErrorKeys: String, CodingKey {
        case type, message, recoverable
    }

    private enum BudgetExceededKeys: String, CodingKey {
        case type, reason
    }

    private enum ScopeDigestKeys: String, CodingKey {
        case type, digest
        case sessionId = "session_id"
        case targetId = "target_id"
    }

    private enum TurnSummaryKeys: String, CodingKey {
        case type, summary
        case touchedTabIds = "touched_tab_ids"
    }

    public init(from decoder: Decoder) throws {
        let typeContainer = try decoder.container(keyedBy: TypeKey.self)
        let typeString = try typeContainer.decode(String.self, forKey: .type)

        guard let eventType = EventType(rawValue: typeString) else {
            self = .unknown(type: typeString)
            return
        }

        switch eventType {
        case .thinking:
            let c = try decoder.container(keyedBy: ThinkingKeys.self)
            self = .thinking(text: try c.decode(String.self, forKey: .text))

        case .toolCall:
            let c = try decoder.container(keyedBy: ToolCallKeys.self)
            self = .toolCall(
                name: try c.decode(String.self, forKey: .name),
                args: try c.decode(AnyCodableValue.self, forKey: .args)
            )

        case .toolResult:
            let c = try decoder.container(keyedBy: ToolResultKeys.self)
            self = .toolResult(
                name: try c.decode(String.self, forKey: .name),
                result: try c.decode(String.self, forKey: .result),
                isError: try c.decode(Bool.self, forKey: .isError)
            )

        case .progress:
            let c = try decoder.container(keyedBy: ProgressKeys.self)
            self = .progress(message: try c.decode(String.self, forKey: .message))

        case .approvalRequired:
            let c = try decoder.container(keyedBy: ApprovalRequiredKeys.self)
            self = .approvalRequired(
                runId: try c.decode(String.self, forKey: .runId),
                action: try c.decode(String.self, forKey: .action),
                description: try c.decode(String.self, forKey: .description)
            )

        case .approvalResponse:
            let c = try decoder.container(keyedBy: ApprovalResponseKeys.self)
            self = .approvalResponse(
                runId: try c.decode(String.self, forKey: .runId),
                approved: try c.decode(Bool.self, forKey: .approved)
            )

        case .done:
            let c = try decoder.container(keyedBy: DoneKeys.self)
            self = .done(summary: try c.decode(String.self, forKey: .summary))

        case .error:
            let c = try decoder.container(keyedBy: ErrorKeys.self)
            self = .error(
                message: try c.decode(String.self, forKey: .message),
                recoverable: try c.decode(Bool.self, forKey: .recoverable)
            )

        case .interrupted:
            self = .interrupted

        case .budgetExceeded:
            let c = try decoder.container(keyedBy: BudgetExceededKeys.self)
            self = .budgetExceeded(reason: try c.decode(String.self, forKey: .reason))

        case .scopeDigest:
            let c = try decoder.container(keyedBy: ScopeDigestKeys.self)
            self = .scopeDigest(
                sessionId: try c.decode(String.self, forKey: .sessionId),
                targetId: try c.decodeIfPresent(String.self, forKey: .targetId),
                digest: try c.decode(String.self, forKey: .digest)
            )

        case .turnSummary:
            let c = try decoder.container(keyedBy: TurnSummaryKeys.self)
            self = .turnSummary(
                summary: try c.decode(String.self, forKey: .summary),
                touchedTabIds: try c.decode([String].self, forKey: .touchedTabIds)
            )
        }
    }

    public func encode(to encoder: Encoder) throws {
        switch self {
        case .thinking(let text):
            var c = encoder.container(keyedBy: ThinkingKeys.self)
            try c.encode("thinking", forKey: .type)
            try c.encode(text, forKey: .text)

        case .toolCall(let name, let args):
            var c = encoder.container(keyedBy: ToolCallKeys.self)
            try c.encode("tool_call", forKey: .type)
            try c.encode(name, forKey: .name)
            try c.encode(args, forKey: .args)

        case .toolResult(let name, let result, let isError):
            var c = encoder.container(keyedBy: ToolResultKeys.self)
            try c.encode("tool_result", forKey: .type)
            try c.encode(name, forKey: .name)
            try c.encode(result, forKey: .result)
            try c.encode(isError, forKey: .isError)

        case .progress(let message):
            var c = encoder.container(keyedBy: ProgressKeys.self)
            try c.encode("progress", forKey: .type)
            try c.encode(message, forKey: .message)

        case .approvalRequired(let runId, let action, let description):
            var c = encoder.container(keyedBy: ApprovalRequiredKeys.self)
            try c.encode("approval_required", forKey: .type)
            try c.encode(runId, forKey: .runId)
            try c.encode(action, forKey: .action)
            try c.encode(description, forKey: .description)

        case .approvalResponse(let runId, let approved):
            var c = encoder.container(keyedBy: ApprovalResponseKeys.self)
            try c.encode("approval_response", forKey: .type)
            try c.encode(runId, forKey: .runId)
            try c.encode(approved, forKey: .approved)

        case .done(let summary):
            var c = encoder.container(keyedBy: DoneKeys.self)
            try c.encode("done", forKey: .type)
            try c.encode(summary, forKey: .summary)

        case .error(let message, let recoverable):
            var c = encoder.container(keyedBy: ErrorKeys.self)
            try c.encode("error", forKey: .type)
            try c.encode(message, forKey: .message)
            try c.encode(recoverable, forKey: .recoverable)

        case .interrupted:
            var c = encoder.container(keyedBy: TypeKey.self)
            try c.encode("interrupted", forKey: .type)

        case .budgetExceeded(let reason):
            var c = encoder.container(keyedBy: BudgetExceededKeys.self)
            try c.encode("budget_exceeded", forKey: .type)
            try c.encode(reason, forKey: .reason)

        case .scopeDigest(let sessionId, let targetId, let digest):
            var c = encoder.container(keyedBy: ScopeDigestKeys.self)
            try c.encode("scope_digest", forKey: .type)
            try c.encode(sessionId, forKey: .sessionId)
            try c.encodeIfPresent(targetId, forKey: .targetId)
            try c.encode(digest, forKey: .digest)

        case .turnSummary(let summary, let touchedTabIds):
            var c = encoder.container(keyedBy: TurnSummaryKeys.self)
            try c.encode("turn_summary", forKey: .type)
            try c.encode(summary, forKey: .summary)
            try c.encode(touchedTabIds, forKey: .touchedTabIds)

        case .unknown(let type):
            var c = encoder.container(keyedBy: TypeKey.self)
            try c.encode(type, forKey: .type)
        }
    }
}

// MARK: - AnyCodableValue

/// A type-erased Codable value that can represent any JSON value.
/// Used where the daemon returns dynamic/untyped JSON payloads.
public enum AnyCodableValue: Codable, Sendable, Hashable {
    case null
    case bool(Bool)
    case int(Int)
    case double(Double)
    case string(String)
    case array([AnyCodableValue])
    case object([String: AnyCodableValue])

    // MARK: Convenience accessors

    public var boolValue: Bool? {
        if case .bool(let b) = self { return b }
        return nil
    }

    public var intValue: Int? {
        if case .int(let i) = self { return i }
        return nil
    }

    public var doubleValue: Double? {
        switch self {
        case .double(let d): return d
        case .int(let i): return Double(i)
        default: return nil
        }
    }

    public var stringValue: String? {
        if case .string(let s) = self { return s }
        return nil
    }

    public var arrayValue: [AnyCodableValue]? {
        if case .array(let a) = self { return a }
        return nil
    }

    public var objectValue: [String: AnyCodableValue]? {
        if case .object(let o) = self { return o }
        return nil
    }

    public var isNull: Bool {
        if case .null = self { return true }
        return false
    }

    public subscript(key: String) -> AnyCodableValue? {
        if case .object(let dict) = self { return dict[key] }
        return nil
    }

    public subscript(index: Int) -> AnyCodableValue? {
        if case .array(let arr) = self, arr.indices.contains(index) { return arr[index] }
        return nil
    }

    // MARK: Codable

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()

        if container.decodeNil() {
            self = .null
            return
        }
        // Bool must be checked before Int/Double because JSONDecoder can decode
        // true/false as Int 1/0.
        if let b = try? container.decode(Bool.self) {
            self = .bool(b)
            return
        }
        if let i = try? container.decode(Int.self) {
            self = .int(i)
            return
        }
        if let d = try? container.decode(Double.self) {
            self = .double(d)
            return
        }
        if let s = try? container.decode(String.self) {
            self = .string(s)
            return
        }
        if let arr = try? container.decode([AnyCodableValue].self) {
            self = .array(arr)
            return
        }
        if let obj = try? container.decode([String: AnyCodableValue].self) {
            self = .object(obj)
            return
        }
        self = .null
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null:
            try container.encodeNil()
        case .bool(let b):
            try container.encode(b)
        case .int(let i):
            try container.encode(i)
        case .double(let d):
            try container.encode(d)
        case .string(let s):
            try container.encode(s)
        case .array(let a):
            try container.encode(a)
        case .object(let o):
            try container.encode(o)
        }
    }

    // MARK: Init from Any (for bridging [String: Any] dicts)

    /// Create an `AnyCodableValue` from a loosely-typed value (e.g. from JSONSerialization).
    public init(_ value: Any) {
        switch value {
        case let b as Bool:
            self = .bool(b)
        case let i as Int:
            self = .int(i)
        case let d as Double:
            self = .double(d)
        case let s as String:
            self = .string(s)
        case let arr as [Any]:
            self = .array(arr.map { AnyCodableValue($0) })
        case let dict as [String: Any]:
            self = .object(dict.mapValues { AnyCodableValue($0) })
        default:
            self = .null
        }
    }
}
