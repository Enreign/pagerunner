import Foundation

/// Durable subset of a chat transcript entry. Persisted to disk as part of a
/// `ChatThread`. Lossy on purpose: tool calls, results, and live thinking are
/// dropped; screenshots persist only their metadata, not the base64 image.
public enum ChatRecord: Codable, Sendable, Identifiable, Hashable {
    case user(id: UUID, text: String, sentAt: Date)
    case agentDone(id: UUID, summary: String, at: Date)
    case screenshot(id: UUID, sessionId: String, targetId: String, tabTitle: String, tabUrl: String, at: Date)
    case error(id: UUID, message: String, at: Date)

    public var id: UUID {
        switch self {
        case .user(let id, _, _),
             .agentDone(let id, _, _),
             .screenshot(let id, _, _, _, _, _),
             .error(let id, _, _):
            return id
        }
    }

    public var timestamp: Date {
        switch self {
        case .user(_, _, let d),
             .agentDone(_, _, let d),
             .screenshot(_, _, _, _, _, let d),
             .error(_, _, let d):
            return d
        }
    }
}
