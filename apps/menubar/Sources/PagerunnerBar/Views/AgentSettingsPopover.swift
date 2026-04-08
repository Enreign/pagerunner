import SwiftUI
import PagerunnerCore

/// Settings popover for the agent, covering profile, approval mode, voice, and model.
struct AgentSettingsPopover: View {
    @Bindable var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Agent Settings")
                .font(.system(size: 12, weight: .semibold))

            Divider()

            // Profile
            settingsRow("Profile") {
                Picker("", selection: $appState.agentProfile) {
                    ForEach(appState.profiles) { p in
                        Text(p.name).tag(p.name)
                    }
                }
                .labelsHidden()
            }

            // Approval mode
            settingsRow("Approval") {
                Picker("", selection: $appState.agentMode) {
                    ForEach(AgentMode.allCases, id: \.self) { m in
                        Text(m.label).tag(m)
                    }
                }
                .labelsHidden()
            }

            Divider()

            Text("Voice")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))

            // Listening mode
            settingsRow("Listening") {
                Picker("", selection: $appState.voiceMode) {
                    Text("Always").tag(AppState.VoiceMode.alwaysListening)
                    Text("Push to Talk").tag(AppState.VoiceMode.pushToTalk)
                }
                .labelsHidden()
            }

            // Narration
            settingsRow("Narration") {
                Picker("", selection: $appState.narrationMode) {
                    Text("Summary").tag(AppState.NarrationMode.summary)
                    Text("Full").tag(AppState.NarrationMode.full)
                    Text("Off").tag(AppState.NarrationMode.off)
                }
                .labelsHidden()
            }

            Divider()

            // Model info
            HStack {
                Text("Model")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                Spacer()
                Text(appState.agentModel)
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
            }
        }
        .padding(12)
        .frame(width: 240)
    }

    private func settingsRow<Content: View>(_ label: String, @ViewBuilder content: () -> Content) -> some View {
        HStack {
            Text(label)
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                .frame(width: 65, alignment: .leading)
            content()
                .frame(maxWidth: .infinity)
        }
    }
}
