import SwiftUI
import PagerunnerCore

struct AgentIdleView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client

    @State private var goalText: String = ""
    @State private var isPressing: Bool = false

    var body: some View {
        VStack(spacing: 16) {
            Spacer().frame(height: 12)

            // Branding
            Image(systemName: "cpu")
                .font(.system(size: 36))
                .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
            Text("Pagerunner Agent")
                .font(.system(size: 16, weight: .semibold))
                .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))

            // Voice listening indicator (replaces text input when active)
            if appState.voiceActive {
                VStack(spacing: 8) {
                    VoiceStatusBadge(status: appState.voiceStatus)

                    if appState.voiceMode == .pushToTalk {
                        // Hold-to-talk button
                        Image(systemName: isPressing ? "mic.fill" : "mic.circle")
                            .font(.system(size: 32))
                            .foregroundColor(isPressing
                                ? Color(red: 0.133, green: 0.773, blue: 0.369)
                                : Color(red: 0.533, green: 0.533, blue: 0.533))
                            .gesture(
                                DragGesture(minimumDistance: 0)
                                    .onChanged { _ in
                                        if !isPressing {
                                            isPressing = true
                                            appState.voicePushToTalkStart()
                                        }
                                    }
                                    .onEnded { _ in
                                        isPressing = false
                                        appState.voicePushToTalkStop()
                                    }
                            )
                            .help("Hold to talk")

                        Text(isPressing ? "Listening..." : "Hold to talk")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    }

                    Button {
                        appState.stopVoice()
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "mic.slash.fill")
                                .font(.system(size: 10))
                            Text("Stop")
                                .font(.system(size: 12, weight: .medium))
                        }
                        .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                    }
                    .buttonStyle(.plain)

                    if !appState.agentGoal.isEmpty {
                        Text(appState.agentGoal)
                            .font(.system(size: 12))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            .italic()
                            .padding(.horizontal, 16)
                    }
                }
                .padding(.vertical, 4)
            }

            // Goal input (hidden when voice is active)
            if !appState.voiceActive {
            TextField("What should I browse?", text: $goalText, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .padding(10)
                .lineLimit(1...3)
                .background(Color.white)
                .cornerRadius(8)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(Color.primary.opacity(0.15), lineWidth: 0.5)
                )
                .padding(.horizontal, 16)
                .onSubmit {
                    if !goalText.isEmpty {
                        startRun()
                    }
                }

            // Profile + Mode pickers
            HStack(spacing: 8) {
                // Profile picker
                Picker("", selection: $appState.agentProfile) {
                    ForEach(appState.profiles) { profile in
                        Text(profile.name).tag(profile.name)
                    }
                }
                .labelsHidden()
                .frame(maxWidth: .infinity)

                // Mode picker
                Picker("", selection: $appState.agentMode) {
                    ForEach(AgentMode.allCases, id: \.self) { mode in
                        Text(mode.label).tag(mode)
                    }
                }
                .labelsHidden()
                .frame(maxWidth: .infinity)

                // Voice button
                Button {
                    appState.startVoice()
                } label: {
                    Image(systemName: "mic")
                        .font(.system(size: 12))
                        .foregroundColor(.white)
                        .frame(width: 28, height: 24)
                        .background(Color(red: 0.533, green: 0.533, blue: 0.533))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .help("Start voice input")
            }
            .padding(.horizontal, 16)

            // Voice settings (expandable)
            DisclosureGroup("Voice Settings") {
                VStack(spacing: 6) {
                    HStack {
                        Text("Listening")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                        Spacer()
                        Picker("", selection: $appState.voiceMode) {
                            ForEach(AppState.VoiceMode.allCases, id: \.self) { mode in
                                Text(mode.label).tag(mode)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 120)
                    }
                    HStack {
                        Text("Narration")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                        Spacer()
                        Picker("", selection: $appState.narrationMode) {
                            ForEach(AppState.NarrationMode.allCases, id: \.self) { mode in
                                Text(mode.label).tag(mode)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 120)
                    }
                }
                .padding(.top, 4)
            }
            .font(.system(size: 11, weight: .medium))
            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
            .padding(.horizontal, 16)

            // Run button row
            HStack {
                Spacer()
                Button(action: startRun) {
                    HStack(spacing: 4) {
                        Text("Run")
                        Image(systemName: "play.fill")
                            .font(.system(size: 9))
                    }
                    .font(.system(size: 12, weight: .medium))
                    .foregroundColor(.white)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 5)
                    .background(goalText.isEmpty
                        ? Color.gray
                        : Color(red: 0, green: 0.478, blue: 1))
                    .cornerRadius(5)
                }
                .buttonStyle(.plain)
                .disabled(goalText.isEmpty)
            }
            .padding(.horizontal, 16)
            } // end if !voiceActive

            Divider().padding(.horizontal, 16)

            // Recent goals
            if !appState.recentGoals.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Recent")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                        .padding(.horizontal, 16)

                    ForEach(appState.recentGoals.prefix(8)) { recent in
                        Button {
                            goalText = recent.goal
                        } label: {
                            HStack(spacing: 6) {
                                Image(systemName: "arrow.counterclockwise")
                                    .font(.system(size: 10))
                                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                                Text(recent.goal)
                                    .font(.system(size: 12))
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                Spacer()
                                Text(formatDuration(recent.duration))
                                    .font(.system(size: 11))
                                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            }
                            .padding(.horizontal, 16)
                            .padding(.vertical, 4)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            Spacer()

            // Model badge
            Text("Model: \(appState.agentModel)")
                .font(.system(size: 10))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                .padding(.bottom, 8)
        }
        .onAppear {
            if appState.agentProfile.isEmpty, let first = appState.profiles.first {
                appState.agentProfile = first.name
            }
            appState.loadAgentHistory(client: client)
        }
    }

    private func startRun() {
        guard !goalText.isEmpty else { return }
        let goal = goalText
        goalText = ""
        appState.startAgentRun(goal: goal, client: client)
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%d:%02d", mins, secs)
    }
}
