import SwiftUI

/// Animated orb that serves as the agent's visual presence indicator.
/// Each state has distinct animation behavior — breathing, pulsing, rotating, rippling.
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

    // MARK: - Animation state

    @State private var breatheScale: CGFloat = 1.0
    @State private var pulseScale: CGFloat = 1.0
    @State private var rotation: Double = 0
    @State private var ripplePhase: CGFloat = 0
    @State private var shakeOffset: CGFloat = 0

    // Color constants
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)
    private let successGreen = Color(red: 0.133, green: 0.773, blue: 0.369)
    private let errorRed = Color(red: 0.937, green: 0.267, blue: 0.267)

    var body: some View {
        ZStack {
            switch state {
            case .idle:
                idleOrb
            case .listening:
                listeningOrb
            case .working:
                workingOrb
            case .speaking:
                speakingOrb
            case .done:
                doneOrb
            case .error:
                errorOrb
            }
        }
        .frame(width: size * 1.4, height: size * 1.4)
        .onChange(of: state) { oldState, newState in
            stopAnimations()
            startAnimations(for: newState)
        }
        .onAppear {
            startAnimations(for: state)
        }
    }

    // MARK: - State views

    @ViewBuilder
    private var idleOrb: some View {
        ZStack {
            Circle()
                .fill(accentBlue.opacity(0.1))
                .frame(width: size, height: size)
                .scaleEffect(breatheScale)
            Circle()
                .fill(accentBlue.opacity(0.3))
                .frame(width: size * 0.4, height: size * 0.4)
        }
    }

    @ViewBuilder
    private var listeningOrb: some View {
        ZStack {
            // Glow ring
            Circle()
                .stroke(successGreen.opacity(0.3), lineWidth: 2)
                .frame(width: size * 1.2, height: size * 1.2)
                .scaleEffect(pulseScale)
                .opacity(Double(2.0 - pulseScale))
            // Main orb
            Circle()
                .fill(successGreen.opacity(0.15))
                .frame(width: size, height: size)
                .scaleEffect(0.95 + (pulseScale - 1.0) * 0.25)
            // Inner active indicator
            Circle()
                .fill(successGreen)
                .frame(width: size * 0.3, height: size * 0.3)
        }
    }

    @ViewBuilder
    private var workingOrb: some View {
        ZStack {
            // Rotating dashed border
            Circle()
                .stroke(style: StrokeStyle(lineWidth: 1.5, dash: [4, 4]))
                .foregroundStyle(accentBlue.opacity(0.4))
                .frame(width: size, height: size)
                .rotationEffect(.degrees(rotation))
            // Inner working dot
            Circle()
                .fill(accentBlue.opacity(0.3))
                .frame(width: size * 0.35, height: size * 0.35)
        }
    }

    @ViewBuilder
    private var speakingOrb: some View {
        ZStack {
            // Ripple rings (3 staggered)
            ForEach(0..<3, id: \.self) { i in
                Circle()
                    .stroke(accentBlue.opacity(0.2), lineWidth: 1)
                    .frame(width: size, height: size)
                    .scaleEffect(rippleScaleFor(index: i))
                    .opacity(rippleOpacityFor(index: i))
            }
            // Main orb
            Circle()
                .fill(accentBlue.opacity(0.15))
                .frame(width: size, height: size)
            // Inner active
            Circle()
                .fill(accentBlue)
                .frame(width: size * 0.3, height: size * 0.3)
        }
    }

    @ViewBuilder
    private var doneOrb: some View {
        ZStack {
            Circle()
                .fill(successGreen.opacity(0.1))
                .frame(width: size, height: size)
            Image(systemName: "checkmark")
                .font(.system(size: size * 0.35, weight: .semibold))
                .foregroundStyle(successGreen)
        }
    }

    @ViewBuilder
    private var errorOrb: some View {
        ZStack {
            Circle()
                .fill(errorRed.opacity(0.1))
                .frame(width: size, height: size)
            Image(systemName: "exclamationmark")
                .font(.system(size: size * 0.35, weight: .semibold))
                .foregroundStyle(errorRed)
        }
        .offset(x: shakeOffset)
    }

    // MARK: - Ripple helpers

    private func rippleScaleFor(index: Int) -> CGFloat {
        let offset = CGFloat(index) / 3.0
        let phase = (ripplePhase + offset).truncatingRemainder(dividingBy: 1.0)
        return 1.0 + phase * 0.6
    }

    private func rippleOpacityFor(index: Int) -> Double {
        let offset = CGFloat(index) / 3.0
        let phase = (ripplePhase + offset).truncatingRemainder(dividingBy: 1.0)
        return Double(1.0 - phase)
    }

    // MARK: - Animation control

    private func stopAnimations() {
        breatheScale = 1.0
        pulseScale = 1.0
        rotation = 0
        ripplePhase = 0
        shakeOffset = 0
    }

    private func startAnimations(for orbState: OrbState) {
        switch orbState {
        case .idle:
            withAnimation(.easeInOut(duration: 3.0).repeatForever(autoreverses: true)) {
                breatheScale = 1.03
            }
        case .listening:
            withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
                pulseScale = 1.2
            }
        case .working:
            withAnimation(.linear(duration: 4.0).repeatForever(autoreverses: false)) {
                rotation = 360
            }
        case .speaking:
            withAnimation(.linear(duration: 2.0).repeatForever(autoreverses: false)) {
                ripplePhase = 1.0
            }
        case .done:
            break
        case .error:
            // Shake animation
            withAnimation(
                .easeInOut(duration: 0.08)
                .repeatCount(5, autoreverses: true)
            ) {
                shakeOffset = 4
            }
            // Reset after shake
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                withAnimation(.easeOut(duration: 0.1)) {
                    shakeOffset = 0
                }
            }
        }
    }
}
