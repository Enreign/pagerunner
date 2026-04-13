import SwiftUI
import PagerunnerCore

struct AgentIdleView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client

    @State private var goalText: String = ""
    @State private var isPressing: Bool = false
    @State private var showSettings: Bool = false

    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)

    var body: some View {
        VStack(spacing: 0) {
            AgentOrbHeader(appState: appState)
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    suggestionSection

                    if !appState.recentGoals.isEmpty {
                        recentRunsSection
                    }

                    composerSection

                    if showSettings {
                        AgentSettingsPopover(appState: appState)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 12)
            }
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

    private var suggestionSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader("Start with a voice-first task")

            VStack(spacing: 8) {
                ForEach(samplePrompts, id: \.title) { prompt in
                    Button {
                        goalText = prompt.command
                    } label: {
                        HStack(alignment: .top, spacing: 10) {
                            Image(systemName: prompt.icon)
                                .font(.system(size: 12, weight: .medium))
                                .foregroundStyle(accentBlue)
                                .frame(width: 28, height: 28)
                                .background(Circle().fill(accentBlue.opacity(0.1)))
                            VStack(alignment: .leading, spacing: 2) {
                                Text(prompt.title)
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(primaryText)
                                Text(prompt.command)
                                    .font(.system(size: 11))
                                    .foregroundStyle(secondaryText)
                                    .lineLimit(2)
                            }
                            Spacer()
                            Image(systemName: "arrow.up.left")
                                .font(.system(size: 10, weight: .medium))
                                .foregroundStyle(secondaryText)
                        }
                        .padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(
                            RoundedRectangle(cornerRadius: 12)
                                .fill(Color.white.opacity(0.86))
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 12)
                                .stroke(Color.black.opacity(0.06), lineWidth: 0.5)
                        )
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var recentRunsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader("Recent")

            VStack(spacing: 8) {
                ForEach(appState.recentGoals.prefix(6)) { recent in
                    Button {
                        goalText = recent.goal
                    } label: {
                        HStack(spacing: 10) {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(recent.goal)
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(primaryText)
                                    .lineLimit(2)
                                Text("\(recent.profile.isEmpty ? "Default" : recent.profile) · \(recent.steps) steps")
                                    .font(.system(size: 10))
                                    .foregroundStyle(secondaryText)
                            }
                            Spacer()
                            Text(formatDuration(recent.duration))
                                .font(.system(size: 11, weight: .medium))
                                .foregroundStyle(secondaryText)
                        }
                        .padding(12)
                        .background(
                            RoundedRectangle(cornerRadius: 12)
                                .fill(Color.black.opacity(0.035))
                        )
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var composerSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionHeader("Ask or speak")

            VStack(spacing: 8) {
                AgentInputBar(
                    text: $goalText,
                    placeholder: "Ask the agent to browse, inspect, or summarize…",
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

                HStack(spacing: 8) {
                    textChip(appState.agentProfile.isEmpty ? "Default profile" : appState.agentProfile)
                    textChip(appState.agentMode.label)
                    textChip(appState.narrationMode.label)
                    Spacer()
                    Button {
                        withAnimation(.easeInOut(duration: 0.18)) {
                            showSettings.toggle()
                        }
                    } label: {
                        Label(showSettings ? "Hide controls" : "Controls", systemImage: "slider.horizontal.3")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(accentBlue)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(12)
            .background(
                RoundedRectangle(cornerRadius: 16)
                    .fill(Color(red: 0, green: 0.478, blue: 1).opacity(0.05))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 16)
                    .stroke(Color(red: 0, green: 0.478, blue: 1).opacity(0.12), lineWidth: 0.5)
            )
        }
    }

    private func sectionHeader(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(secondaryText)
            .textCase(.uppercase)
            .tracking(0.5)
    }

    private func textChip(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(secondaryText)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(
                Capsule()
                    .fill(Color.black.opacity(0.05))
            )
    }

    private var samplePrompts: [(title: String, command: String, icon: String)] {
        [
            ("Daily web brief", "Go to Hacker News and summarize the top stories", "newspaper"),
            ("Inbox triage", "Open Linear and summarize my urgent issues", "tray.full"),
            ("Research run", "Find the latest docs for redb 4 migration and summarize them", "magnifyingglass")
        ]
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%d:%02d", mins, secs)
    }
}
