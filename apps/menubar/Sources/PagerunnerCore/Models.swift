import Foundation

// MARK: - Wire protocol

// Note: DaemonClient.call() constructs requests as [String: Any] using
// JSONSerialization — no DaemonRequest struct is needed.

/// Response from daemon (JSON-lines) — result is double-serialized JSON string
public struct DaemonResponse: Codable, Sendable {
    public let id: String
    public let result: String?
    public let error: String?
}

// MARK: - Response envelopes

public struct ListProfilesResponse: Codable, Sendable {
    public let ok: Bool
    public let data: [Profile]
}

public struct ListSessionsResponse: Codable, Sendable {
    public let ok: Bool
    public let data: [Session]
}

public struct ListTabsResponse: Codable, Sendable {
    public let ok: Bool
    public let data: [Tab]
}

public struct ListCheckpointsResponse: Codable, Sendable {
    public let ok: Bool
    public let data: [Checkpoint]
}

// MARK: - Domain models

public struct Profile: Codable, Identifiable, Sendable {
    public var id: String { name }
    public let name: String
    public let displayName: String
    public let kind: String?      // "personal" | "agent" | "attached" | nil
    public let userDataDir: String?
    public let debugPort: Int?

    enum CodingKeys: String, CodingKey {
        case name, kind
        case displayName = "display_name"
        case userDataDir = "user_data_dir"
        case debugPort = "debug_port"
    }
}

public struct Session: Codable, Identifiable, Sendable {
    public let id: String
    public let profile: String
    public let displayName: String
    public let stealth: Bool
    public let status: SessionStatus

    enum CodingKeys: String, CodingKey {
        case id, profile, stealth, status
        case displayName = "display_name"
    }
}

public enum SessionStatus: String, Codable, Sendable {
    case alive
    case crashed
    case reconnecting
    case recovering
}

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

public struct Checkpoint: Codable, Identifiable, Sendable {
    public var id: String { checkpointId }
    public let checkpointId: String
    public let name: String
    public let savedAt: Int   // Unix SECONDS (not microseconds — matches daemon response)
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

// MARK: - Discovery models (not Codable — purely in-memory UI state)

public enum AttachState: Equatable, Sendable {
    case idle
    case attaching
    case attached(profileDisplayName: String)
    case failed(String)
}

public struct DiscoveredInstance: Identifiable, Sendable {
    public let id: String        // "port-9225"
    public let port: Int
    public let tabCount: Int
    public let isVM: Bool        // true if owning process is gvproxy
    public var attachState: AttachState

    public init(id: String, port: Int, tabCount: Int, isVM: Bool, attachState: AttachState) {
        self.id = id
        self.port = port
        self.tabCount = tabCount
        self.isVM = isVM
        self.attachState = attachState
    }
}

// Note: DaemonClient.call() takes args as [String: Any] and passes them directly
// to JSONSerialization — no AnyCodable wrapper needed. Args are simple primitives
// (String, Bool, Int) at all call sites.

// MARK: - Agent models

/// Minimal Codable wrapper for heterogeneous JSON values (Sendable-safe).
public enum AnyCodable: Codable, Sendable {
    case string(String)
    case int(Int)
    case double(Double)
    case bool(Bool)
    case null

    public var stringValue: String? {
        if case .string(let s) = self { return s }
        return nil
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let s = try? container.decode(String.self) { self = .string(s) }
        else if let i = try? container.decode(Int.self) { self = .int(i) }
        else if let d = try? container.decode(Double.self) { self = .double(d) }
        else if let b = try? container.decode(Bool.self) { self = .bool(b) }
        else { self = .null }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let s): try container.encode(s)
        case .int(let i): try container.encode(i)
        case .double(let d): try container.encode(d)
        case .bool(let b): try container.encode(b)
        case .null: try container.encodeNil()
        }
    }
}

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

    public var modeDescription: String {
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
    public var id: String { "\(timestamp.timeIntervalSince1970)-\(goal.prefix(20))" }
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
