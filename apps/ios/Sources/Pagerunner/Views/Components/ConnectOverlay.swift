import SwiftUI

struct ConnectOverlay: View {
    var onOpenSettings: () -> Void

    @State private var appeared = false

    var body: some View {
        ZStack {
            // Dim underlying content so the overlay reads clearly
            Color.black.opacity(0.45)
                .ignoresSafeArea()

            VStack(spacing: Theme.Spacing.loose) {
                Spacer(minLength: 0)

                VStack(spacing: Theme.Spacing.loose) {
                    // Monogram
                    ZStack {
                        RoundedRectangle(cornerRadius: 22, style: .continuous)
                            .fill(.operatorSubtle)
                            .frame(width: 84, height: 84)
                        Image(systemName: "figure.run")
                            .font(.system(size: 38, weight: .semibold))
                            .foregroundStyle(.accent)
                    }
                    .shadow(color: .black.opacity(0.35), radius: 18, x: 0, y: 10)

                    VStack(spacing: Theme.Spacing.tight) {
                        Text("Pagerunner")
                            .font(.title.bold())
                        Text("Not connected to a daemon")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    Button(action: onOpenSettings) {
                        HStack(spacing: 8) {
                            Image(systemName: "gear")
                            Text("Open Settings")
                        }
                        .font(.body.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .tint(.accent)

                    HStack(spacing: 6) {
                        Image(systemName: "lock.shield")
                            .foregroundStyle(.secondary)
                        Text("Use your Tailscale IP for remote access")
                            .foregroundStyle(.secondary)
                    }
                    .font(.footnote)
                }
                .padding(.horizontal, Theme.Spacing.loose + 8)
                .padding(.vertical, Theme.Spacing.section + 8)
                .frame(maxWidth: .infinity)
                .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card + 4, style: .continuous))
                .padding(.horizontal, Theme.Spacing.loose)
                .scaleEffect(appeared ? 1 : 0.95)
                .opacity(appeared ? 1 : 0)
                .animation(.spring(duration: 0.45, bounce: 0.2), value: appeared)

                Spacer(minLength: 0)
            }
        }
        .onAppear { appeared = true }
    }
}

#Preview {
    ZStack {
        Color.operatorBackground.ignoresSafeArea()
        ConnectOverlay(onOpenSettings: {})
    }
}
