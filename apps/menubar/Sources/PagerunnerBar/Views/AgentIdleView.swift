import SwiftUI
import PagerunnerCore

struct AgentIdleView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client

    @State private var goalText: String = ""
    @State private var isPressing: Bool = false

    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)
    private let dangerRed = Color(red: 0.937, green: 0.267, blue: 0.267)

    var body: some View {
        VStack(spacing: 0) {
            AgentOrbHeader(appState: appState)

            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    composerSection

                    if let voiceError = appState.voiceError {
                        voiceErrorSection(message: voiceError)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 4)
                .padding(.bottom, 12)
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
        let trimmed = goalText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        goalText = ""
        appState.startAgentRun(goal: trimmed, client: client)
    }

    private var composerSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            AgentInputBar(
                text: $goalText,
                placeholder: "Ask or speak…",
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
                Text(statusLine)
                    .font(.system(size: 11))
                    .foregroundStyle(secondaryText)
                    .lineLimit(2)
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color.white.opacity(0.9))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(Color.black.opacity(0.07), lineWidth: 0.5)
        )
    }

    private func voiceErrorSection(message: String) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Voice couldn’t start")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(primaryText)

            Text(message)
                .font(.system(size: 11))
                .foregroundStyle(secondaryText)
                .fixedSize(horizontal: false, vertical: true)

            HStack {
                Button("Retry voice") {
                    appState.retryVoice()
                }
                .buttonStyle(.plain)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(accentBlue)

                Spacer()

                Button("Use text only") {
                    appState.stopVoice(clearError: true)
                }
                .buttonStyle(.plain)
                .font(.system(size: 11))
                .foregroundStyle(secondaryText)
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(dangerRed.opacity(0.06))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(dangerRed.opacity(0.14), lineWidth: 0.5)
        )
    }

    private var statusLine: String {
        if appState.voiceError != nil {
            return "Voice is unavailable right now. You can still run tasks by typing."
        }
        if appState.voiceMode == .pushToTalk {
            return appState.voiceActive
                ? "\(appState.globalHotkeyTrigger.hint). Release to send."
                : "Press the mic or \(appState.voiceShortcutHint.lowercased())."
        }
        if appState.voiceActive {
            switch appState.voiceStatus {
            case .starting:
                return "Starting the voice sidecar."
            case .processing:
                return "Transcribing the current utterance."
            case .speaking:
                return "Narration is playing."
            case .listening, .idle:
                return "The mic is live."
            }
        }
        return "Type a goal or turn on the mic."
    }
}
