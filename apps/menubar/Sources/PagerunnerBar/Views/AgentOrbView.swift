import SwiftUI

/// A soft, glassy orb that carries most of the visual character for the Agent
/// surface so the surrounding UI can stay minimal.
struct AgentOrbView: View {
    let state: OrbState
    var size: CGFloat = 48

    enum OrbState: Equatable {
        case idle
        case listening
        case working
        case speaking
        case done
        case error
    }

    @State private var breatheScale: CGFloat = 1.0
    @State private var glowScale: CGFloat = 1.0
    @State private var swirlOffset: CGFloat = 0
    @State private var swirlRotation: Double = 0
    @State private var ripplePhase: CGFloat = 0
    @State private var shakeOffset: CGFloat = 0

    private struct Palette {
        let shell: Color
        let core: Color
        let mist: Color
        let band: Color
        let glow: Color
        let shadow: Color
    }

    var body: some View {
        let palette = paletteForState

        ZStack {
            Ellipse()
                .fill(
                    RadialGradient(
                        colors: [
                            palette.shadow.opacity(0.2),
                            palette.shadow.opacity(0.08),
                            .clear,
                        ],
                        center: .center,
                        startRadius: 0,
                        endRadius: size * 0.75
                    )
                )
                .frame(width: size * 1.25, height: size * 0.4)
                .blur(radius: size * 0.16)
                .offset(y: size * 0.72)

            Circle()
                .fill(palette.glow.opacity(0.18))
                .frame(width: size * 1.34, height: size * 1.34)
                .blur(radius: size * 0.2)
                .scaleEffect(glowScale)

            if state == .speaking || state == .listening {
                ForEach(0..<2, id: \.self) { index in
                    Circle()
                        .stroke(palette.glow.opacity(index == 0 ? 0.2 : 0.12), lineWidth: 1)
                        .frame(width: size * 1.1, height: size * 1.1)
                        .scaleEffect(rippleScale(for: index))
                        .opacity(rippleOpacity(for: index))
                        .blur(radius: size * 0.01)
                }
            }

            orbBody(palette: palette)
                .scaleEffect(breatheScale)
                .offset(x: shakeOffset)
        }
        .frame(width: size * 1.65, height: size * 1.8)
        .onAppear {
            startAnimations(for: state)
        }
        .onChange(of: state) { _, newState in
            stopAnimations()
            startAnimations(for: newState)
        }
    }

    private func orbBody(palette: Palette) -> some View {
        ZStack {
            Circle()
                .fill(
                    RadialGradient(
                        colors: [
                            Color.white.opacity(0.9),
                            palette.mist.opacity(0.72),
                            palette.shell.opacity(0.64),
                        ],
                        center: .topLeading,
                        startRadius: size * 0.04,
                        endRadius: size * 0.9
                    )
                )
                .frame(width: size, height: size)

            ZStack {
                Circle()
                    .fill(palette.core.opacity(0.18))
                    .frame(width: size * 0.84, height: size * 0.84)
                    .offset(x: size * 0.1, y: size * 0.22)
                    .blur(radius: size * 0.06)

                Circle()
                    .fill(palette.mist.opacity(0.36))
                    .frame(width: size * 0.7, height: size * 0.7)
                    .offset(x: -size * 0.18, y: -size * 0.12)
                    .blur(radius: size * 0.12)

                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [
                                .clear,
                                palette.band.opacity(0.42),
                                Color.white.opacity(0.72),
                                palette.band.opacity(0.26),
                                .clear,
                            ],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: size * 0.92, height: size * 0.2)
                    .rotationEffect(.degrees(16 + swirlRotation))
                    .offset(x: size * 0.08, y: -size * 0.06 + swirlOffset)
                    .blur(radius: size * 0.07)

                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [
                                .clear,
                                palette.core.opacity(0.2),
                                palette.band.opacity(0.32),
                                .clear,
                            ],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .frame(width: size * 0.7, height: size * 0.14)
                    .rotationEffect(.degrees(-28 + swirlRotation * 0.6))
                    .offset(x: -size * 0.06, y: size * 0.1 - swirlOffset * 0.6)
                    .blur(radius: size * 0.08)
            }
            .clipShape(Circle())

            Circle()
                .fill(
                    RadialGradient(
                        colors: [
                            Color.white.opacity(0.78),
                            Color.white.opacity(0.22),
                            .clear,
                        ],
                        center: .center,
                        startRadius: 0,
                        endRadius: size * 0.24
                    )
                )
                .frame(width: size * 0.34, height: size * 0.34)
                .offset(x: -size * 0.12, y: -size * 0.22)
                .blur(radius: size * 0.04)

            Circle()
                .stroke(
                    LinearGradient(
                        colors: [
                            Color.white.opacity(0.48),
                            Color.white.opacity(0.16),
                            palette.shell.opacity(0.18),
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    ),
                    lineWidth: 0.9
                )
                .frame(width: size, height: size)
        }
        .background(
            Circle()
                .fill(Color.white.opacity(0.16))
                .frame(width: size * 1.08, height: size * 1.08)
                .blur(radius: size * 0.1)
        )
        .drawingGroup()
    }

    private var paletteForState: Palette {
        switch state {
        case .idle:
            return Palette(
                shell: Color(red: 0.77, green: 0.75, blue: 0.98),
                core: Color(red: 0.53, green: 0.73, blue: 1.0),
                mist: Color(red: 0.89, green: 0.92, blue: 1.0),
                band: Color(red: 0.38, green: 0.8, blue: 1.0),
                glow: Color(red: 0.76, green: 0.78, blue: 1.0),
                shadow: Color(red: 0.73, green: 0.74, blue: 0.95)
            )
        case .listening:
            return Palette(
                shell: Color(red: 0.63, green: 0.93, blue: 0.87),
                core: Color(red: 0.35, green: 0.88, blue: 0.78),
                mist: Color(red: 0.9, green: 1.0, blue: 0.97),
                band: Color(red: 0.62, green: 1.0, blue: 0.92),
                glow: Color(red: 0.56, green: 1.0, blue: 0.88),
                shadow: Color(red: 0.55, green: 0.88, blue: 0.8)
            )
        case .working:
            return Palette(
                shell: Color(red: 0.77, green: 0.75, blue: 0.98),
                core: Color(red: 0.47, green: 0.69, blue: 1.0),
                mist: Color(red: 0.87, green: 0.92, blue: 1.0),
                band: Color(red: 0.54, green: 0.84, blue: 1.0),
                glow: Color(red: 0.64, green: 0.72, blue: 1.0),
                shadow: Color(red: 0.61, green: 0.68, blue: 0.96)
            )
        case .speaking:
            return Palette(
                shell: Color(red: 0.76, green: 0.75, blue: 0.98),
                core: Color(red: 0.55, green: 0.73, blue: 1.0),
                mist: Color(red: 0.9, green: 0.93, blue: 1.0),
                band: Color(red: 0.39, green: 0.83, blue: 1.0),
                glow: Color(red: 0.67, green: 0.75, blue: 1.0),
                shadow: Color(red: 0.64, green: 0.69, blue: 0.96)
            )
        case .done:
            return Palette(
                shell: Color(red: 0.71, green: 0.95, blue: 0.84),
                core: Color(red: 0.34, green: 0.82, blue: 0.56),
                mist: Color(red: 0.92, green: 1.0, blue: 0.96),
                band: Color(red: 0.56, green: 0.96, blue: 0.78),
                glow: Color(red: 0.64, green: 0.95, blue: 0.79),
                shadow: Color(red: 0.53, green: 0.83, blue: 0.68)
            )
        case .error:
            return Palette(
                shell: Color(red: 0.98, green: 0.75, blue: 0.78),
                core: Color(red: 0.97, green: 0.39, blue: 0.44),
                mist: Color(red: 1.0, green: 0.92, blue: 0.93),
                band: Color(red: 1.0, green: 0.62, blue: 0.68),
                glow: Color(red: 1.0, green: 0.7, blue: 0.72),
                shadow: Color(red: 0.93, green: 0.55, blue: 0.59)
            )
        }
    }

    private func rippleScale(for index: Int) -> CGFloat {
        let offset = CGFloat(index) * 0.35
        let phase = (ripplePhase + offset).truncatingRemainder(dividingBy: 1.0)
        return 1.0 + phase * 0.45
    }

    private func rippleOpacity(for index: Int) -> Double {
        let offset = CGFloat(index) * 0.35
        let phase = (ripplePhase + offset).truncatingRemainder(dividingBy: 1.0)
        return Double(0.95 - phase)
    }

    private func stopAnimations() {
        breatheScale = 1.0
        glowScale = 1.0
        swirlOffset = 0
        swirlRotation = 0
        ripplePhase = 0
        shakeOffset = 0
    }

    private func startAnimations(for orbState: OrbState) {
        withAnimation(.easeInOut(duration: 3.4).repeatForever(autoreverses: true)) {
            breatheScale = 1.03
        }
        withAnimation(.easeInOut(duration: 4.4).repeatForever(autoreverses: true)) {
            swirlOffset = size * 0.08
            swirlRotation = 7
        }

        switch orbState {
        case .idle:
            withAnimation(.easeInOut(duration: 3.0).repeatForever(autoreverses: true)) {
                glowScale = 1.04
            }
        case .listening:
            withAnimation(.easeInOut(duration: 1.0).repeatForever(autoreverses: true)) {
                glowScale = 1.16
            }
            withAnimation(.linear(duration: 1.8).repeatForever(autoreverses: false)) {
                ripplePhase = 1.0
            }
        case .working:
            withAnimation(.linear(duration: 4.8).repeatForever(autoreverses: false)) {
                swirlRotation = 20
            }
            withAnimation(.easeInOut(duration: 1.2).repeatForever(autoreverses: true)) {
                glowScale = 1.1
            }
        case .speaking:
            withAnimation(.easeInOut(duration: 1.1).repeatForever(autoreverses: true)) {
                glowScale = 1.14
            }
            withAnimation(.linear(duration: 1.6).repeatForever(autoreverses: false)) {
                ripplePhase = 1.0
            }
        case .done:
            withAnimation(.easeInOut(duration: 1.8).repeatForever(autoreverses: true)) {
                glowScale = 1.08
            }
        case .error:
            withAnimation(.easeInOut(duration: 0.08).repeatCount(5, autoreverses: true)) {
                shakeOffset = 4
            }
        }
    }
}
