import Foundation

/// Agent-visible, thread-scoped context. Holds 0..N pinned tabs plus
/// user-written intent and agent-written observations that compound across
/// turns. Scope is a property of `ChatThread` (1:1) and is persisted via
/// `ThreadStore`.
public struct Scope: Codable, Sendable, Hashable {
    public var tabs: [ScopeTab]
    public var goal: String?
    public var notes: String?
    public var turnLog: [TurnLogEntry]

    public static let turnLogCap = 20

    public init(
        tabs: [ScopeTab] = [],
        goal: String? = nil,
        notes: String? = nil,
        turnLog: [TurnLogEntry] = []
    ) {
        self.tabs = tabs
        self.goal = goal
        self.notes = notes
        self.turnLog = turnLog
    }

    /// Append a `TurnLogEntry`; if the total exceeds the cap, drop oldest.
    public mutating func append(_ entry: TurnLogEntry) {
        turnLog.append(entry)
        while turnLog.count > Self.turnLogCap {
            turnLog.removeFirst()
        }
    }
}

public struct ScopeTab: Codable, Sendable, Hashable, Identifiable {
    public static let digestCap = 500

    public var id: String { "\(sessionId)-\(targetId ?? "first")" }
    public let sessionId: String
    public let targetId: String?
    public var label: String
    public var purpose: String?
    public var digest: String?
    public var lastTouchedAt: Date?

    public init(
        sessionId: String,
        targetId: String? = nil,
        label: String,
        purpose: String? = nil,
        digest: String? = nil,
        lastTouchedAt: Date? = nil
    ) {
        self.sessionId = sessionId
        self.targetId = targetId
        self.label = label
        self.purpose = purpose
        self.digest = digest
        self.lastTouchedAt = lastTouchedAt
    }

    /// Set `digest`, truncating to `digestCap` characters.
    public mutating func setDigest(_ raw: String) {
        digest = raw.count > Self.digestCap
            ? String(raw.prefix(Self.digestCap))
            : raw
    }
}

public struct TurnLogEntry: Codable, Sendable, Hashable {
    public let userGoal: String
    public let summary: String
    public let touchedTabIds: [String]
    public let timestamp: Date

    public init(
        userGoal: String,
        summary: String,
        touchedTabIds: [String],
        timestamp: Date
    ) {
        self.userGoal = userGoal
        self.summary = summary
        self.touchedTabIds = touchedTabIds
        self.timestamp = timestamp
    }
}
