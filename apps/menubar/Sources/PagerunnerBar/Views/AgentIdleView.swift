import SwiftUI
import PagerunnerCore

struct AgentIdleView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client

    @State private var goalText: String = ""
    @State private var isPressing: Bool = false
    @State private var showSettings: Bool = false

    var body: some View {
        VStack(spacing: 0) {
            // Compact header
            HStack(spacing: 6) {
                Text("\u{1F916} Pagerunner Agent")
                    .font(.system(size: 14, weight: .semibold))
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Divider()

            // Recent goals list filling the main area
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    if appState.recentGoals.isEmpty {
                        VStack(spacing: 8) {
                            Spacer().frame(height: 40)
                            Image(systemName: "cpu")
                                .font(.system(size: 28))
                                .foregroundColor(Color(red: 0, green: 0.478, blue: 1).opacity(0.3))
                            Text("Ask the agent to browse for you")
                                .font(.system(size: 12))
                                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                        }
                        .frame(maxWidth: .infinity)
                    } else {
                        Text("Recent")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            .padding(.horizontal, 12)
                            .padding(.top, 8)

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
                                .padding(.horizontal, 12)
                                .padding(.vertical, 4)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }

            Divider()

            // Persistent input bar + settings line at the bottom
            VStack(spacing: 6) {
                AgentInputBar(
                    text: $goalText,
                    placeholder: "Ask or speak...",
                    voiceActive: appState.voiceActive,
                    voiceStatus: appState.voiceStatus,
                    voiceMode: appState.voiceMode,
                    isRunning: false,
                    onSend: startRun,
                    onStop: {},
                    onMicTap: {
                        if appState.voiceActive {
                            appState.stopVoice()
                        } else {
                            appState.startVoice()
                        }
                    },
                    onMicHoldStart: {
                        if !isPressing {
                            isPressing = true
                            appState.voicePushToTalkStart()
                        }
                    },
                    onMicHoldEnd: {
                        isPressing = false
                        appState.voicePushToTalkStop()
                    }
                )
                .padding(.horizontal, 12)

                // Settings line
                HStack(spacing: 4) {
                    Text("\(appState.agentProfile.isEmpty ? "default" : appState.agentProfile) \u{00B7} \(appState.agentMode.label) \u{00B7} \(appState.narrationMode.label)")
                        .font(.system(size: 10))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    Spacer()
                    Button {
                        showSettings.toggle()
                    } label: {
                        Image(systemName: "gearshape")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    }
                    .buttonStyle(.plain)
                    .popover(isPresented: $showSettings) {
                        AgentSettingsPopover(appState: appState)
                    }
                }
                .padding(.horizontal, 12)
            }
            .padding(.vertical, 8)
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
