import SwiftUI
import PagerunnerKit

struct ChatItemView: View {
    let item: ChatItem
    var onOpenInspector: (ChatView.InspectorContext) -> Void
    var onOpenFullscreen: (FullscreenScreenshot) -> Void = { _ in }

    struct FullscreenScreenshot: Identifiable, Equatable {
        let id = UUID()
        let image: UIImage
        let caption: ChatItem.Caption?

        static func == (lhs: Self, rhs: Self) -> Bool { lhs.id == rhs.id }
    }

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
        case .screenshot(_, let base64, let sid, let tid, let caption):
            screenshotCard(base64: base64, sessionId: sid, targetId: tid, caption: caption)
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
            markdownText(text)
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
                    markdownText(summary)
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

    /// Render Markdown-ish text the way iMessage/Linear/Claude do: bold,
    /// italics, inline code, and bullet lists. Falls back to plain text if
    /// the parser rejects the string.
    private func markdownText(_ raw: String) -> some View {
        let opts = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace
        )
        let attr: AttributedString
        if let parsed = try? AttributedString(markdown: raw, options: opts) {
            attr = parsed
        } else {
            attr = AttributedString(raw)
        }
        return Text(attr)
            .font(.callout)
            .foregroundStyle(.primary)
            .textSelection(.enabled)
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

    private func screenshotCard(base64: String, sessionId: String?, targetId: String?, caption: ChatItem.Caption?) -> some View {
        let img: UIImage? = {
            guard !base64.isEmpty else { return nil }
            let stripped: String = {
                if let r = base64.range(of: ";base64,") { return String(base64[r.upperBound...]) }
                return base64
            }()
            guard let data = Data(base64Encoded: stripped) else { return nil }
            return UIImage(data: data)
        }()

        return Button {
            if let img {
                onOpenFullscreen(.init(image: img, caption: caption))
            } else if let sid = sessionId {
                onOpenInspector(.init(sessionId: sid, targetId: targetId))
            }
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                if let img {
                    Image(uiImage: img)
                        .resizable()
                        .scaledToFit()
                        .frame(maxWidth: .infinity)
                } else {
                    Rectangle()
                        .fill(.operatorCard)
                        .frame(height: 180)
                        .overlay(
                            VStack(spacing: 6) {
                                Image(systemName: "photo")
                                Text("screenshot not in history")
                                    .font(.caption2)
                            }
                            .foregroundStyle(.secondary)
                        )
                }
                if let caption {
                    captionStrip(caption)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            .shadow(color: .black.opacity(0.25), radius: 12, y: 4)
        }
        .buttonStyle(.plain)
    }

    private func captionStrip(_ caption: ChatItem.Caption) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "globe")
                .font(.caption2)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 1) {
                Text(caption.title.isEmpty ? caption.host : caption.title)
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                if !caption.title.isEmpty {
                    Text(caption.host)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.Spacing.regular)
        .padding(.vertical, 10)
        .background(.operatorCard)
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
