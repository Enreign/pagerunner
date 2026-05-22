import SwiftUI

/// A 8pt colored dot with an optional pulsing halo for "live" states.
struct StatusDot: View {
    enum Kind {
        case live
        case idle
        case error
        case muted

        var color: Color {
            switch self {
            case .live:  .accentColor
            case .idle:  .yellow
            case .error: .red
            case .muted: .secondary
            }
        }

        var pulses: Bool { self == .live }
    }

    let state: Kind
    var size: CGFloat = 8

    @State private var animate = false

    var body: some View {
        ZStack {
            if state.pulses {
                Circle()
                    .fill(state.color.opacity(0.35))
                    .frame(width: size * 2.25, height: size * 2.25)
                    .scaleEffect(animate ? 1.0 : 0.5)
                    .opacity(animate ? 0 : 1)
                    .animation(
                        .easeOut(duration: 1.6).repeatForever(autoreverses: false),
                        value: animate
                    )
            }
            Circle()
                .fill(state.color)
                .frame(width: size, height: size)
        }
        .onAppear { animate = true }
        .accessibilityHidden(true)
    }
}

#Preview {
    VStack(spacing: 24) {
        HStack { StatusDot(state: .live);  Text("live") }
        HStack { StatusDot(state: .idle);  Text("idle") }
        HStack { StatusDot(state: .error); Text("error") }
        HStack { StatusDot(state: .muted); Text("muted") }
    }
    .padding()
}
