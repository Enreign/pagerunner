import Foundation

/// A persistent chat thread. Anchors to a `Scope` (may be empty) and
/// accumulates `ChatRecord` entries representing user-visible turn
/// outcomes.
public struct ChatThread: Codable, Sendable, Identifiable, Hashable {
    public let id: UUID
    public var title: String
    public var scope: Scope
    public var records: [ChatRecord]
    public let createdAt: Date
    public var updatedAt: Date

    /// Legacy accessor: returns `scope.tabs.first` mapped to a `PinnedContext`.
    /// Kept so existing call sites in `AppState` can migrate incrementally.
    public var pinnedContext: PinnedContext? {
        get {
            guard let first = scope.tabs.first else { return nil }
            return PinnedContext(sessionId: first.sessionId, targetId: first.targetId)
        }
        set {
            if let ctx = newValue {
                scope.tabs = [ScopeTab(sessionId: ctx.sessionId, targetId: ctx.targetId, label: "")]
            } else {
                scope.tabs = []
            }
        }
    }

    public init(
        id: UUID = UUID(),
        title: String = "New thread",
        scope: Scope = Scope(),
        records: [ChatRecord] = [],
        createdAt: Date = .now,
        updatedAt: Date = .now
    ) {
        self.id = id
        self.title = title
        self.scope = scope
        self.records = records
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    // MARK: - Codable (with legacy pinnedContext migration)

    private enum CodingKeys: String, CodingKey {
        case id, title, scope, records, createdAt, updatedAt
        case pinnedContext
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        self.id = try c.decode(UUID.self, forKey: .id)
        self.title = try c.decode(String.self, forKey: .title)
        self.records = try c.decode([ChatRecord].self, forKey: .records)
        self.createdAt = try c.decode(Date.self, forKey: .createdAt)
        self.updatedAt = try c.decode(Date.self, forKey: .updatedAt)

        // Prefer new `scope` key. If absent, migrate from legacy pinnedContext.
        if let scope = try c.decodeIfPresent(Scope.self, forKey: .scope) {
            self.scope = scope
        } else if let legacy = try? c.decodeIfPresent(PinnedContext.self, forKey: .pinnedContext) {
            let tab = ScopeTab(
                sessionId: legacy.sessionId,
                targetId: legacy.targetId,
                label: ""
            )
            self.scope = Scope(tabs: [tab])
        } else {
            self.scope = Scope()
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(title, forKey: .title)
        try c.encode(scope, forKey: .scope)
        try c.encode(records, forKey: .records)
        try c.encode(createdAt, forKey: .createdAt)
        try c.encode(updatedAt, forKey: .updatedAt)
        // Never emit legacy `pinnedContext` on write — migration is one-way.
    }
}
