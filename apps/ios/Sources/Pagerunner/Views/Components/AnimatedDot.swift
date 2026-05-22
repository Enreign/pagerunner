import SwiftUI

private struct AnimatedDotModifier: ViewModifier {
    let offset: Double
    @State private var up = false

    func body(content: Content) -> some View {
        content
            .opacity(up ? 1.0 : 0.3)
            .animation(
                .easeInOut(duration: 0.55)
                    .repeatForever()
                    .delay(offset),
                value: up
            )
            .onAppear { up = true }
    }
}

extension View {
    func animatedDot(offset: Double) -> some View {
        modifier(AnimatedDotModifier(offset: offset))
    }
}
