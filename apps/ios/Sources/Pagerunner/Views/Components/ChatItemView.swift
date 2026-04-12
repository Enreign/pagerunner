import SwiftUI
import PagerunnerKit

struct ChatItemView: View {
    let item: ChatItem
    var onOpenInspector: (ChatView.InspectorContext) -> Void

    var body: some View {
        switch item {
        case .user(_, let text, _):
            userBubble(text)
        case .agentThinking(_, let text):
            thinkingRow(text)
        case .toolCall(_, let name, let args, let sid, let tid):
            toolCallRow(name: name, args: args, sessionId: sid, targetId: tid)
        case .toolResult(_, _, let summary, let isError):
            toolResultRow(summary: summary, isError: isError)
        case .agentDone(_, let summary):
            doneRow(summary)
        case .approval(_, _, let action, let description):
            approvalRow(action: action, description: description)
        case .error(_, let message):
            errorRow(message)
        }
    }

    // MARK: User

    private func userBubble(_ text: String) -> some View {
        HStack {
            Spacer(minLength: 40)
            Text(text)
                .font(.callout)
                .foregroundStyle(.white)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(.accent, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
    }

    // MARK: Agent

    private func thinkingRow(_ text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            agentAvatar
            Text(text)
                .font(.callout)
                .italic()
                .foregroundStyle(.secondary)
            Spacer(minLength: 40)
        }
    }

    private func doneRow(_ summary: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            agentAvatar
            VStack(alignment: .leading, spacing: 6) {
                if !summary.isEmpty {
                    Text(summary)
                        .font(.callout)
                        .foregroundStyle(.primary)
                }
                HStack(spacing: 6) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.caption)
                        .foregroundStyle(.accent)
                    Text("done")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 40)
        }
    }

    // MARK: Tool

    private func toolCallRow(name: String, args: String, sessionId: String?, targetId: String?) -> some View {
        Button {
            if let sid = sessionId {
                onOpenInspector(.init(sessionId: sid, targetId: targetId))
            }
        } label: {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: toolIcon(name))
                    .font(.caption)
                    .foregroundStyle(.accent)
                    .frame(width: 20)
                    .padding(.top, 3)
                VStack(alignment: .leading, spacing: 2) {
                    Text(name)
                        .font(.monoCaption.weight(.semibold))
                        .foregroundStyle(.primary)
                    if !args.isEmpty {
                        Text(args)
                            .font(.monoFootnote)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }
                }
                Spacer(minLength: 0)
                if sessionId != nil {
                    Image(systemName: "chevron.right")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(Theme.Spacing.regular)
            .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    private func toolResultRow(summary: String, isError: Bool) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: isError ? "xmark.circle.fill" : "checkmark")
                .font(.caption)
                .foregroundStyle(isError ? .red : .accent)
                .frame(width: 20)
                .padding(.top, 3)
            Text(summary.isEmpty ? (isError ? "error" : "ok") : summary)
                .font(.monoFootnote)
                .foregroundStyle(.secondary)
                .lineLimit(3)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.Spacing.regular)
    }

    private func errorRow(_ message: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
            Text(message)
                .font(.footnote)
                .foregroundStyle(.primary)
        }
        .padding(Theme.Spacing.regular)
        .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
    }

    private func approvalRow(action: String, description: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "hand.raised.fill")
                .foregroundStyle(.yellow)
            VStack(alignment: .leading, spacing: 2) {
                Text("Agent wants to \(action)")
                    .font(.subheadline.weight(.semibold))
                Text(description)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }
            Spacer(minLength: 0)
        }
        .padding(Theme.Spacing.regular)
        .background(Color.yellow.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
    }

    // MARK: Helpers

    private var agentAvatar: some View {
        Image(systemName: "figure.run")
            .font(.caption)
            .foregroundStyle(.accent)
            .frame(width: 24, height: 24)
            .background(.operatorCard, in: Circle())
    }

    private func toolIcon(_ name: String) -> String {
        switch name {
        case let n where n.contains("screenshot"):  return "camera"
        case let n where n.contains("navigate"):    return "arrow.up.right"
        case let n where n.contains("click"):       return "hand.tap"
        case let n where n.contains("fill"):        return "text.cursor"
        case let n where n.contains("evaluate"):    return "terminal"
        case let n where n.contains("new_tab"):     return "plus.rectangle"
        case let n where n.contains("close"):       return "xmark.rectangle"
        case let n where n.contains("open_session"):return "macwindow.on.rectangle"
        case let n where n.contains("network"):     return "network"
        case let n where n.contains("console"):     return "terminal"
        default:                                    return "wrench.and.screwdriver"
        }
    }
}
