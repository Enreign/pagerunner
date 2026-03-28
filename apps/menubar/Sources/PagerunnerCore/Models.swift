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
    public let kind: String       // "personal" | "agent"

    enum CodingKeys: String, CodingKey {
        case name, kind
        case displayName = "display_name"
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

// Note: DaemonClient.call() takes args as [String: Any] and passes them directly
// to JSONSerialization — no AnyCodable wrapper needed. Args are simple primitives
// (String, Bool, Int) at all call sites.
