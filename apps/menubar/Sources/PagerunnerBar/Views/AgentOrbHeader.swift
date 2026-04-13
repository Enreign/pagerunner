import SwiftUI

struct AgentOrbHeader: View {
    @Bindable var appState: AppState

    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)

    var body: some View {
        HStack(spacing: 14) {
            AgentOrbView(state: currentOrbState, size: 58)
                .animation(.easeInOut(duration: 0.28), value: currentOrbState)

            VStack(alignment: .leading, spacing: 3) {
                Text("Agent")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(primaryText)
                Text(statusText)
                    .font(.system(size: 12))
                    .foregroundStyle(primaryText)
                Text(metaText)
                    .font(.system(size: 10))
                    .foregroundStyle(secondaryText)
                    .lineLimit(1)
            }

            Spacer(minLength: 8)

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
                                .fill(Color.white.opacity(0.78))
                        )
                        .overlay(
                            Circle()
                                .stroke(Color.black.opacity(0.06), lineWidth: 0.5)
                        )
                }
                .buttonStyle(.plain)
                .help(appState.voiceMuted ? "Unmute narration" : "Mute narration")
            }
        }
        .padding(.horizontal, 14)
        .padding(.top, 16)
        .padding(.bottom, 10)
    }

    private var currentOrbState: AgentOrbView.OrbState {
        if appState.voiceError != nil { return .error }
        if appState.voiceStatus == .starting || appState.voiceStatus == .processing { return .working }
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
        if appState.voiceError != nil { return "Voice unavailable" }
        if appState.voiceStatus == .starting { return "Starting voice..." }
        if appState.voiceStatus == .processing { return "Transcribing..." }
        if appState.voiceStatus == .listening { return "Listening..." }
        if appState.voiceStatus == .speaking { return "Speaking..." }
        switch appState.agentState {
        case .idle: return "Ready"
        case .running: return "Working..."
        case .waitingApproval: return "Waiting for approval"
        case .completed: return "Done"
        case .error: return "Something went wrong"
        }
    }

    private var metaText: String {
        let profile = agentLabel
        let mode = appState.agentMode.label
        if appState.voiceError != nil {
            return "\(profile) · \(mode)"
        }
        if appState.voiceActive {
            return "\(profile) · \(mode) · \(voiceLabel)"
        }
        return "\(profile) · \(mode)"
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
        case .idle: return "Voice off"
        case .starting: return "Starting"
        case .listening: return "Mic on"
        case .processing: return "Thinking"
        case .speaking: return "Speaking"
        }
    }
}
