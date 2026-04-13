import SwiftUI

struct VoiceHUDView: View {
    @Bindable var appState: AppState

    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)
    private let subtleFill = Color.white.opacity(0.94)

    var body: some View {
        Group {
            if appState.isVoiceHUDExpanded {
                expandedBody
            } else {
                compactBody
            }
        }
        .animation(.spring(response: 0.26, dampingFraction: 0.9), value: appState.isVoiceHUDExpanded)
    }

    private var compactBody: some View {
        HStack(spacing: 10) {
            AgentOrbView(state: orbState, size: 28)
                .animation(.easeInOut(duration: 0.2), value: orbState)

            Text(compactPrompt)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(primaryText)
                .lineLimit(1)

            Spacer(minLength: 8)

            shortcutBadge(active: false)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .frame(width: 244, height: 62)
        .background(hudBackground(cornerRadius: 20))
    }

    private var expandedBody: some View {
        HStack(spacing: 14) {
            AgentOrbView(state: orbState, size: 46)
                .animation(.easeInOut(duration: 0.2), value: orbState)

            VStack(alignment: .leading, spacing: 4) {
                Text(appState.voiceHUDTitle)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(primaryText)
                    .lineLimit(1)

                Text(appState.voiceHUDDetail)
                    .font(.system(size: 12))
                    .foregroundStyle(secondaryText)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)
            }

            Spacer(minLength: 8)

            if appState.voiceMode == .pushToTalk || appState.globalPushToTalkEnabled {
                shortcutBadge(active: appState.globalPushToTalkPressed)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .frame(width: 320, height: 104)
        .background(hudBackground(cornerRadius: 24))
    }

    @ViewBuilder
    private func shortcutBadge(active: Bool) -> some View {
        VStack(alignment: .trailing, spacing: 4) {
            Text(appState.globalHotkeyTrigger.label)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(active ? accentBlue : secondaryText)
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(
                    Capsule()
                        .fill((active ? accentBlue : Color.black).opacity(active ? 0.12 : 0.06))
                )

            if appState.globalHotkeyTrigger == .functionKey {
                Text("Cmd")
                    .font(.system(size: 9, weight: .medium))
                    .foregroundStyle(secondaryText.opacity(0.8))
            }
        }
    }

    private func hudBackground(cornerRadius: CGFloat) -> some View {
        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
            .fill(subtleFill)
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .stroke(Color.black.opacity(0.08), lineWidth: 0.5)
            )
            .shadow(color: Color.black.opacity(0.12), radius: 18, y: 12)
    }

    private var compactPrompt: String {
        if appState.globalHotkeyTrigger.continuousHint != nil, appState.globalHotkeyTrigger == .functionKey {
            return "Hold Fn to talk"
        }
        return appState.globalHotkeyTrigger.hint
    }

    private var orbState: AgentOrbView.OrbState {
        if appState.voiceError != nil { return .error }
        switch appState.voiceStatus {
        case .starting, .processing:
            return .working
        case .listening:
            return .listening
        case .speaking:
            return .speaking
        case .idle:
            return appState.globalPushToTalkPressed ? .listening : .idle
        }
    }
}
