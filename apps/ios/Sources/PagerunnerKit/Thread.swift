import Foundation

/// A persistent chat. Anchors to a `PinnedContext` (or none) and accumulates
/// `ChatRecord` entries representing the user-visible turn outcomes.
public struct Thread: Codable, Sendable, Identifiable, Hashable {
    public let id: UUID
    public var title: String
    public var pinnedContext: PinnedContext?
    public var records: [ChatRecord]
    public let createdAt: Date
    public var updatedAt: Date

    public init(
        id: UUID = UUID(),
        title: String = "New thread",
        pinnedContext: PinnedContext? = nil,
        records: [ChatRecord] = [],
        createdAt: Date = .now,
        updatedAt: Date = .now
    ) {
        self.id = id
        self.title = title
        self.pinnedContext = pinnedContext
        self.records = records
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}
