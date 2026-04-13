import SwiftUI
import PagerunnerCore

struct AgentFeedView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client
    @State private var isPressing: Bool = false
    @State private var followUpText: String = ""

    private let secondaryText = Color(red: 0.533, green: 0.533, blue: 0.533)
    private let primaryText = Color(red: 0.114, green: 0.114, blue: 0.122)
    private let accentBlue = Color(red: 0, green: 0.478, blue: 1)

    var body: some View {
        VStack(spacing: 0) {
            AgentOrbHeader(appState: appState)

            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        if let voiceError = appState.voiceError {
                            voiceErrorBanner(message: voiceError)
                        }

                        if !appState.agentGoal.isEmpty {
                            Text(appState.agentGoal)
                                .font(.system(size: 12))
                                .foregroundStyle(secondaryText)
                                .lineLimit(3)
                                .padding(.horizontal, 2)
                        }

                        LazyVStack(alignment: .leading, spacing: 8) {
                            ForEach(appState.agentEvents) { event in
                                AgentEventRow(kind: event.kind)
                                    .id(event.id)
                            }

                            if let approval = appState.agentApproval {
                                AgentApprovalCard(
                                    action: approval.action,
                                    description: approval.description,
                                    onApprove: { appState.approveAgent(approved: true, client: client) },
                                    onDeny: { appState.approveAgent(approved: false, client: client) }
                                )
                                .id("approval")
                            }

                            if appState.agentState == .running {
                                HStack(spacing: 6) {
                                    ProgressView()
                                        .scaleEffect(0.65)
                                        .frame(width: 12, height: 12)
                                    Text("Working…")
                                        .font(.system(size: 11))
                                        .foregroundStyle(secondaryText)
                                }
                                .padding(.horizontal, 12)
                                .id("spinner")
                            }

                            if appState.agentState == .completed, let summary = appState.agentSummary {
                                AgentResultCard(
                                    summary: summary,
                                    steps: appState.agentSteps,
                                    tokens: appState.agentTokens,
                                    voiceActive: appState.voiceActive,
                                    onReplay: {
                                        appState.voiceReplay(text: summary)
                                    },
                                    onCopy: {
                                        NSPasteboard.general.clearContents()
                                        NSPasteboard.general.setString(summary, forType: .string)
                                    }
                                )
                                .id("result-card")
                            }
                        }
                        .padding(12)
                        .background(
                            RoundedRectangle(cornerRadius: 14)
                                .fill(Color.white.opacity(0.9))
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 14)
                                .stroke(Color.black.opacity(0.07), lineWidth: 0.5)
                        )

                        bottomBar
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 12)
                }
                .onChange(of: appState.agentEvents.count) { _, _ in
                    withAnimation(.easeOut(duration: 0.2)) {
                        if let last = appState.agentEvents.last {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        } else if appState.agentState == .running {
                            proxy.scrollTo("spinner", anchor: .bottom)
                        }
                    }
                }
                .onChange(of: appState.agentState) { _, newState in
                    if newState == .completed {
                        withAnimation(.easeOut(duration: 0.2)) {
                            proxy.scrollTo("result-card", anchor: .bottom)
                        }
                    }
                }
            }
        }
    }

    private func voiceErrorBanner(message: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "mic.slash.fill")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Color(red: 0.937, green: 0.267, blue: 0.267))
                .frame(width: 20, height: 20)
                .background(
                    Circle()
                        .fill(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.1))
                )

            VStack(alignment: .leading, spacing: 4) {
                Text("Voice is offline")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(primaryText)
                Text(message)
                    .font(.system(size: 11))
                    .foregroundStyle(secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Spacer(minLength: 8)

            Button("Retry") {
                appState.retryVoice()
            }
            .buttonStyle(.plain)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(accentBlue)
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.07))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.14), lineWidth: 0.5)
        )
    }

    @ViewBuilder
    private var bottomBar: some View {
        VStack(spacing: 4) {
            switch appState.agentState {
            case .running, .waitingApproval:
                VStack(alignment: .leading, spacing: 8) {
                    AgentInputBar(
                        text: $followUpText,
                        placeholder: "Follow up after this run…",
                        voiceActive: appState.voiceActive,
                        voiceStatus: appState.voiceStatus,
                        voiceMode: appState.voiceMode,
                        isRunning: true,
                        onSend: {},
                        onStop: { appState.stopAgent(client: client) },
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

                    HStack(spacing: 6) {
                        if appState.agentState == .waitingApproval {
                            HStack(spacing: 4) {
                                Image(systemName: "pause.fill")
                                    .font(.system(size: 9))
                                Text("Waiting for approval...")
                            }
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))
                        } else if appState.voiceStatus == .speaking && appState.voiceActive {
                            waveformIndicator
                        } else {
                            Text("Step \(appState.agentSteps)/15 · \(formatTokens(appState.agentTokens))")
                                .font(.system(size: 11))
                                .foregroundStyle(secondaryText)
                        }
                        Spacer()
                    }
                }

            case .completed:
                VStack(alignment: .leading, spacing: 8) {
                    AgentInputBar(
                        text: $followUpText,
                        placeholder: "Ask a follow-up or start another task…",
                        voiceActive: appState.voiceActive,
                        voiceStatus: appState.voiceStatus,
                        voiceMode: appState.voiceMode,
                        isRunning: false,
                        onSend: startFollowUp,
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

                    HStack(spacing: 6) {
                        Text("Done · \(appState.agentSteps) steps · \(formatTokens(appState.agentTokens))")
                            .font(.system(size: 10, weight: .medium))
                            .foregroundColor(Color(red: 0.133, green: 0.773, blue: 0.369))
                        Spacer()
                        Button("New Run") { appState.resetAgent() }
                            .font(.system(size: 11, weight: .medium))
                            .buttonStyle(.plain)
                            .foregroundStyle(accentBlue)
                    }
                }

            case .error:
                VStack(spacing: 6) {
                    HStack {
                        HStack(spacing: 4) {
                            Image(systemName: "xmark")
                                .font(.system(size: 10, weight: .bold))
                            Text("Failed \u{00B7} \(appState.agentSteps) steps")
                        }
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                        Spacer()
                    }
                    HStack(spacing: 8) {
                        Button("Retry") {
                            let goal = appState.agentGoal
                            appState.startAgentRun(goal: goal, client: client)
                        }
                        .font(.system(size: 12, weight: .medium))
                        .buttonStyle(.plain)
                        Spacer()
                        Button("New Goal") { appState.resetAgent() }
                            .font(.system(size: 12, weight: .medium))
                            .buttonStyle(.plain)
                    }
                }

            case .idle:
                EmptyView()
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 14)
                .fill(Color.white.opacity(0.9))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14)
                .stroke(Color.black.opacity(0.07), lineWidth: 0.5)
        )
    }

    @ViewBuilder
    private var waveformIndicator: some View {
        HStack(spacing: 2) {
            ForEach(0..<5, id: \.self) { i in
                RoundedRectangle(cornerRadius: 1)
                    .fill(Color(red: 0, green: 0.478, blue: 1))
                    .frame(width: 3, height: waveformHeight(for: i))
            }
            Text("Speaking...")
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
        }
    }

    /// Deterministic height per bar index to avoid SwiftUI re-render jitter.
    private func waveformHeight(for index: Int) -> CGFloat {
        let heights: [CGFloat] = [6, 12, 8, 14, 5]
        return heights[index % heights.count]
    }

    private func startFollowUp() {
        guard !followUpText.isEmpty else { return }
        let goal = followUpText
        followUpText = ""
        appState.startAgentRun(goal: goal, client: client)
    }

    private func formatTokens(_ tokens: Int) -> String {
        if tokens >= 1000 {
            return "\(tokens / 1000)K tk"
        }
        return "\(tokens) tk"
    }
}

// MARK: - Event row

struct AgentEventRow: View {
    let kind: AppState.AgentEventKind

    var body: some View {
        switch kind {
        case .thinking(let text):
            HStack(alignment: .top, spacing: 6) {
                Image(systemName: "bubble.left.fill")
                    .font(.system(size: 10))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                Text(text)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.black.opacity(0.03)))

        case .toolCall(let name, _):
            HStack(spacing: 6) {
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))
                Text(name)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color(red: 0, green: 0.478, blue: 1).opacity(0.04)))

        case .toolResult(_, let ok, let summary):
            HStack(alignment: .top, spacing: 6) {
                Image(systemName: ok ? "checkmark" : "xmark")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundColor(ok
                        ? Color(red: 0.133, green: 0.773, blue: 0.369)
                        : Color(red: 0.937, green: 0.267, blue: 0.267))
                Text(summary)
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    .lineLimit(2)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.black.opacity(0.025)))

        case .progress(let msg):
            Text(msg)
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(RoundedRectangle(cornerRadius: 10).fill(Color.black.opacity(0.02)))

        case .done(let summary):
            HStack(alignment: .top, spacing: 6) {
                Image(systemName: "bubble.left.fill")
                    .font(.system(size: 10))
                    .foregroundColor(Color(red: 0.133, green: 0.773, blue: 0.369))
                Text(summary)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color(red: 0.133, green: 0.773, blue: 0.369).opacity(0.08)))

        case .error(let msg):
            HStack(alignment: .top, spacing: 6) {
                Image(systemName: "xmark.circle.fill")
                    .font(.system(size: 10))
                    .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                Text(msg)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.08)))
        }
    }
}
