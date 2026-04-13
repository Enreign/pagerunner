import SwiftUI
import PagerunnerCore

/// Inline settings card for the agent. Despite the historic file name, this is
/// now rendered inside the panel so it matches the rest of the app visually.
struct AgentSettingsPopover: View {
    @Bindable var appState: AppState

    private let cardFill = Color.white.opacity(0.92)
    private let cardStroke = Color.black.opacity(0.08)
    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Agent Controls")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(primaryText)
                    Text("Profile, approvals, and voice behavior")
                        .font(.system(size: 11))
                        .foregroundStyle(secondaryText)
                }
                Spacer()
                if appState.voiceActive {
                    statusChip("Voice live", tint: accentBlue)
                }
            }

            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 10) {
                controlCard("Profile") {
                    Picker("Profile", selection: $appState.agentProfile) {
                        ForEach(appState.profiles) { profile in
                            Text(profile.displayName).tag(profile.name)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                controlCard("Approval") {
                    Picker("Approval", selection: $appState.agentMode) {
                        ForEach(AgentMode.allCases, id: \.self) { mode in
                            Text(mode.label).tag(mode)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                controlCard("Listening") {
                    Picker("Listening", selection: $appState.voiceMode) {
                        Text("Always").tag(AppState.VoiceMode.alwaysListening)
                        Text("Push to Talk").tag(AppState.VoiceMode.pushToTalk)
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                controlCard("Narration") {
                    Picker("Narration", selection: $appState.narrationMode) {
                        Text("Summary").tag(AppState.NarrationMode.summary)
                        Text("Full").tag(AppState.NarrationMode.full)
                        Text("Off").tag(AppState.NarrationMode.off)
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                controlCard("Hold to Talk") {
                    Toggle(isOn: $appState.globalPushToTalkEnabled) {
                        Text("Global shortcut")
                            .font(.system(size: 11))
                            .foregroundStyle(primaryText)
                    }
                    .toggleStyle(.switch)
                    .controlSize(.small)
                }

                controlCard("Hold Key") {
                    Picker("Hold Key", selection: $appState.globalHotkeyTrigger) {
                        ForEach(AppState.GlobalHotkeyTrigger.allCases, id: \.self) { trigger in
                            Text(trigger.label).tag(trigger)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    statusChip(appState.agentMode.label, tint: accentBlue)
                    statusChip(appState.narrationMode.label, tint: Color(red: 0.533, green: 0.533, blue: 0.533))
                    if appState.voiceMode == .pushToTalk {
                        statusChip("Push to Talk", tint: Color(red: 0.133, green: 0.773, blue: 0.369))
                    }
                    if appState.globalPushToTalkEnabled {
                        statusChip(appState.globalHotkeyTrigger.label, tint: Color(red: 0.114, green: 0.114, blue: 0.122))
                    }
                }

                HStack(alignment: .top) {
                    Text("Model")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(secondaryText)
                    Spacer()
                    Text(appState.agentModel)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(primaryText.opacity(0.75))
                        .multilineTextAlignment(.trailing)
                }

                if appState.globalPushToTalkEnabled {
                    Text("\(sentenceCase(appState.voiceShortcutHint)). The floating HUD appears while listening, transcribing, and speaking.")
                        .font(.system(size: 10))
                        .foregroundStyle(secondaryText)
                }

                if appState.voiceActive {
                    Text("Changes to profile, listening, or narration apply to the active voice session immediately.")
                        .font(.system(size: 10))
                        .foregroundStyle(secondaryText)
                }
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(cardFill)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(cardStroke, lineWidth: 0.5)
        )
    }

    @ViewBuilder
    private func controlCard<Content: View>(_ label: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label)
                .font(.system(size: 10, weight: .medium))
                .foregroundStyle(secondaryText)
            content()
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(Color.black.opacity(0.035))
        )
    }

    private func statusChip(_ text: String, tint: Color) -> some View {
        Text(text)
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(tint)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(
                Capsule()
                    .fill(tint.opacity(0.1))
            )
    }

    private func sentenceCase(_ text: String) -> String {
        guard let first = text.first else { return text }
        return first.uppercased() + text.dropFirst()
    }
}
