import SwiftUI

/// Combines the animated orb with status text and action buttons.
/// Replaces the flat text header in both idle and feed views.
struct AgentOrbHeader: View {
    @Bindable var appState: AppState

    // Color constants
    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                AgentOrbView(state: currentOrbState, size: 52)
                    .animation(.easeInOut(duration: 0.3), value: currentOrbState)

                VStack(alignment: .leading, spacing: 4) {
                    Text("Pagerunner Agent")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(primaryText)
                    Text(statusText)
                        .font(.system(size: 11))
                        .foregroundStyle(secondaryText)
                    HStack(spacing: 6) {
                        infoChip(agentLabel, tint: accentBlue)
                        infoChip(appState.agentMode.label, tint: statusAccent)
                        if appState.voiceActive {
                            infoChip(voiceLabel, tint: statusAccent.opacity(0.85))
                        }
                    }
                }

                Spacer()

                VStack(alignment: .trailing, spacing: 6) {
                    if appState.voiceActive {
                        Button {
                            appState.voiceMuted.toggle()
                        } label: {
                            Image(systemName: appState.voiceMuted ? "speaker.slash.fill" : "speaker.wave.2.fill")
                                .font(.system(size: 12))
                                .foregroundStyle(appState.voiceMuted ? secondaryText : accentBlue)
                                .frame(width: 28, height: 28)
                                .background(
                                    Circle()
                                        .fill(Color.white.opacity(0.72))
                                )
                        }
                        .buttonStyle(.plain)
                        .help(appState.voiceMuted ? "Unmute narration" : "Mute narration")
                    }

                    Circle()
                        .fill(statusAccent)
                        .frame(width: 10, height: 10)
                        .overlay(Circle().stroke(Color.white, lineWidth: 2))
                }
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 16)
                .fill(
                    LinearGradient(
                        colors: [
                            Color.white.opacity(0.95),
                            accentBlue.opacity(0.06)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
        )
        .overlay(
            RoundedRectangle(cornerRadius: 16)
                .stroke(Color.black.opacity(0.08), lineWidth: 0.5)
        )
        .padding(.horizontal, 12)
        .padding(.top, 12)
    }

    private var currentOrbState: AgentOrbView.OrbState {
        if appState.voiceStatus == .speaking { return .speaking }
        if appState.voiceStatus == .listening { return .listening }
        switch appState.agentState {
        case .running: return .working
        case .completed: return .done
        case .error: return .error
        default: return .idle
        }
    }

    private var statusText: String {
        if appState.voiceStatus == .listening { return "Listening..." }
        if appState.voiceStatus == .speaking { return "Speaking..." }
        switch appState.agentState {
        case .idle: return "Ready"
        case .running: return "Working... \u{00B7} Step \(appState.agentSteps)"
        case .waitingApproval: return "Needs approval"
        case .completed: return "Done \u{00B7} \(appState.agentSteps) steps"
        case .error: return "Error"
        }
    }

    private var agentLabel: String {
        let fallback = appState.profiles.first?.displayName ?? "Default profile"
        let raw = appState.agentProfile.isEmpty
            ? fallback
            : appState.profiles.first(where: { $0.name == appState.agentProfile })?.displayName ?? appState.agentProfile
        if let paren = raw.firstIndex(of: "(") {
            return String(raw[..<paren]).trimmingCharacters(in: .whitespaces)
        }
        return raw
    }

    private var voiceLabel: String {
        switch appState.voiceStatus {
        case .idle: return "Voice idle"
        case .starting: return "Starting voice"
        case .listening: return "Mic hot"
        case .processing: return "Transcribing"
        case .speaking: return "Narrating"
        }
    }

    private var statusAccent: Color {
        if appState.voiceStatus == .listening {
            return Color(red: 0.133, green: 0.773, blue: 0.369)
        }
        if appState.voiceStatus == .speaking {
            return accentBlue
        }
        switch appState.agentState {
        case .waitingApproval:
            return Color(red: 0.961, green: 0.620, blue: 0.043)
        case .completed:
            return Color(red: 0.133, green: 0.773, blue: 0.369)
        case .error:
            return Color(red: 0.937, green: 0.267, blue: 0.267)
        case .running:
            return accentBlue
        case .idle:
            return Color(red: 0.533, green: 0.533, blue: 0.533)
        }
    }

    private func infoChip(_ text: String, tint: Color) -> some View {
        Text(text)
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(tint)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(
                Capsule()
                    .fill(tint.opacity(0.12))
            )
    }
}
