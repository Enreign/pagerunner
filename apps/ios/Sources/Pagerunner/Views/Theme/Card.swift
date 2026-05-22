import SwiftUI

/// Grouped container with consistent padding, radius, and surface color.
struct Card<Content: View>: View {
    var padding: CGFloat = Theme.Spacing.loose
    @ViewBuilder let content: () -> Content

    var body: some View {
        content()
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }
}

/// Section header: small caps + tint, for use above cards.
struct SectionLabel: View {
    let text: String
    var body: some View {
        Text(text)
            .font(.statLabel)
            .tracking(1.2)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 4)
    }
}
