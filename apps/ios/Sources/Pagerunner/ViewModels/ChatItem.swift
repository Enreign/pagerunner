import Foundation
import PagerunnerKit

/// One entry in the chat transcript. Covers both user-authored messages and
/// the incremental agent events streamed from the daemon.
enum ChatItem: Identifiable, Sendable {
    case user(id: UUID, text: String, sent: Date)
    case agentThinking(id: UUID, text: String)
    case toolCall(id: UUID, name: String, args: String, sessionId: String?, targetId: String?)
    case toolResult(id: UUID, name: String, summary: String, isError: Bool)
    case screenshot(id: UUID, base64: String, sessionId: String?, targetId: String?, caption: Caption?)
    case agentDone(id: UUID, summary: String)
    case approval(id: UUID, runId: String, action: String, description: String)
    case error(id: UUID, message: String)

    var id: UUID {
        switch self {
        case .user(let id, _, _),
             .agentThinking(let id, _),
             .toolCall(let id, _, _, _, _),
             .toolResult(let id, _, _, _),
             .screenshot(let id, _, _, _, _),
             .agentDone(let id, _),
             .approval(let id, _, _, _),
             .error(let id, _):
            return id
        }
    }

    struct Caption: Sendable, Hashable {
        let title: String
        let url: String

        var host: String {
            URL(string: url)?.host() ?? url
        }
    }
}

extension ChatItem {
    /// Try to convert a streaming agent event into a chat item. Some events
    /// (progress, interrupted, budget, unknown, approvalResponse) are best
    /// rendered as compact status rows but we drop them from the transcript
    /// for now; they show up in the Inspector's event timeline.
    static func from(_ event: AgentEventDetail) -> ChatItem? {
        switch event {
        case .thinking(let text):
            return .agentThinking(id: UUID(), text: text)
        case .toolCall(let name, let args):
            let dict = args.dictValue ?? [:]
            return .toolCall(
                id: UUID(),
                name: name,
                args: renderArgs(dict),
                sessionId: sessionIdFromArgs(dict),
                targetId: targetIdFromArgs(dict)
            )
        case .toolResult(let name, let result, let isError):
            // Screenshots come back as JSON carrying base64 data — render
            // them as an inline image card instead of a text status row.
            if name.contains("screenshot"), !isError, let base64 = extractScreenshotBase64(result) {
                return .screenshot(id: UUID(), base64: base64, sessionId: nil, targetId: nil, caption: nil)
            }
            return .toolResult(id: UUID(), name: name, summary: summarise(result), isError: isError)
        case .done(let summary):
            return .agentDone(id: UUID(), summary: summary)
        case .error(let message, _):
            return .error(id: UUID(), message: message)
        case .approvalRequired(let runId, let action, let description):
            return .approval(id: UUID(), runId: runId, action: action, description: description)
        case .progress, .interrupted, .budgetExceeded, .approvalResponse, .unknown:
            return nil
        }
    }

    private static func renderArgs(_ args: [String: AnyCodableValue]) -> String {
        guard !args.isEmpty else { return "" }
        let keys: [String] = ["url", "text", "selector", "value", "profile"]
        for k in keys {
            if let v = args[k], let s = v.stringValue, !s.isEmpty {
                return s
            }
        }
        return args.keys.sorted().joined(separator: ", ")
    }

    private static func sessionIdFromArgs(_ args: [String: AnyCodableValue]) -> String? {
        args["session_id"]?.stringValue
    }

    private static func targetIdFromArgs(_ args: [String: AnyCodableValue]) -> String? {
        args["target_id"]?.stringValue
    }

    /// Pull the base64 payload out of a screenshot tool result. The daemon
    /// serialises it as either `{"data": "data:image/png;base64,…"}` or
    /// `{"base64": "…"}`.
    private static func extractScreenshotBase64(_ result: String) -> String? {
        guard let data = result.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        for key in ["data", "base64", "image"] {
            if let s = obj[key] as? String, !s.isEmpty { return s }
        }
        return nil
    }

    private static func summarise(_ result: String) -> String {
        let trimmed = result.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.count > 140 {
            return String(trimmed.prefix(140)) + "…"
        }
        return trimmed
    }
}

/// Minimal helper — extracts a string out of `AnyCodableValue` when it's a
/// string literal. Works for JSON scalars.
private extension AnyCodableValue {
    var stringValue: String? {
        switch self {
        case .string(let s): return s
        case .int(let i):    return String(i)
        case .double(let d): return String(d)
        case .bool(let b):   return String(b)
        default:             return nil
        }
    }

    var dictValue: [String: AnyCodableValue]? {
        if case .object(let d) = self { return d }
        return nil
    }
}
