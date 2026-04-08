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
        HStack(spacing: 12) {
            AgentOrbView(state: currentOrbState, size: 44)
                .animation(.easeInOut(duration: 0.3), value: currentOrbState)

            VStack(alignment: .leading, spacing: 2) {
                Text("Pagerunner Agent")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(primaryText)
                Text(statusText)
                    .font(.system(size: 11))
                    .foregroundStyle(secondaryText)
            }

            Spacer()

            // Mute toggle (only when voice active + running/speaking)
            if appState.voiceActive && (appState.agentState == .running || appState.voiceStatus == .speaking) {
                Button {
                    appState.voiceMuted.toggle()
                } label: {
                    Image(systemName: appState.voiceMuted ? "speaker.slash" : "speaker.wave.2")
                        .font(.system(size: 12))
                        .foregroundStyle(appState.voiceMuted ? secondaryText : accentBlue)
                }
                .buttonStyle(.plain)
                .help(appState.voiceMuted ? "Unmute narration" : "Mute narration")
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
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
}
