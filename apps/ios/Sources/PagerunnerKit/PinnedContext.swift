import Foundation

/// What the agent operates on for a given turn. `nil` = agent picks freely.
public struct PinnedContext: Codable, Sendable, Hashable {
    public let sessionId: String
    public let targetId: String?

    public init(sessionId: String, targetId: String? = nil) {
        self.sessionId = sessionId
        self.targetId = targetId
    }
}
