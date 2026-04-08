import SwiftUI
import PagerunnerCore

struct AgentFeedView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client
    @State private var isPressing: Bool = false
    @State private var followUpText: String = ""
    @State private var showSettings: Bool = false

    var body: some View {
        VStack(spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text("\u{1F916} Pagerunner Agent")
                        .font(.system(size: 14, weight: .semibold))
                    Spacer()

                    // Mute/unmute narration toggle (only when voice active)
                    if appState.voiceActive {
                        Button {
                            appState.voiceMuted.toggle()
                        } label: {
                            Image(systemName: appState.voiceMuted ? "speaker.slash" : "speaker.wave.2")
                                .font(.system(size: 12))
                                .foregroundColor(appState.voiceMuted
                                    ? Color(red: 0.533, green: 0.533, blue: 0.533)
                                    : Color(red: 0, green: 0.478, blue: 1))
                        }
                        .buttonStyle(.plain)
                        .help(appState.voiceMuted ? "Unmute narration" : "Mute narration")
                    }

                    // Mic toggle
                    if appState.voiceActive && appState.voiceMode == .pushToTalk {
                        Image(systemName: isPressing ? "mic.fill" : "mic")
                            .foregroundColor(isPressing
                                ? Color(red: 0.133, green: 0.773, blue: 0.369)
                                : Color(red: 0.533, green: 0.533, blue: 0.533))
                            .font(.system(size: 14))
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
                    } else {
                        Button {
                            if appState.voiceActive {
                                appState.stopVoice()
                            } else {
                                appState.startVoice()
                            }
                        } label: {
                            Image(systemName: appState.voiceActive ? "mic.fill" : "mic")
                                .foregroundColor(appState.voiceActive
                                    ? Color(red: 0.937, green: 0.267, blue: 0.267)
                                    : Color(red: 0.533, green: 0.533, blue: 0.533))
                                .font(.system(size: 14))
                        }
                        .buttonStyle(.plain)
                        .help(appState.voiceActive ? "Stop voice" : "Start voice")
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Divider()

            // Event feed
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(appState.agentEvents) { event in
                            AgentEventRow(kind: event.kind)
                                .id(event.id)
                        }

                        // Approval card (inline)
                        if let approval = appState.agentApproval {
                            AgentApprovalCard(
                                action: approval.action,
                                description: approval.description,
                                onApprove: { appState.approveAgent(approved: true, client: client) },
                                onDeny: { appState.approveAgent(approved: false, client: client) }
                            )
                            .id("approval")
                        }

                        // Spinner for running state
                        if appState.agentState == .running {
                            HStack(spacing: 6) {
                                ProgressView()
                                    .scaleEffect(0.6)
                                    .frame(width: 12, height: 12)
                                Text("Working...")
                                    .font(.system(size: 11))
                                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            }
                            .padding(.horizontal, 12)
                            .id("spinner")
                        }

                        // Result card when completed
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
                    .padding(.vertical, 8)
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

            Divider()

            // Bottom bar: unified input + status
            bottomBar
        }
    }

    @ViewBuilder
    private var bottomBar: some View {
        VStack(spacing: 4) {
            switch appState.agentState {
            case .running, .waitingApproval:
                AgentInputBar(
                    text: $followUpText,
                    placeholder: "Follow up...",
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
                .padding(.horizontal, 12)

                // Status line
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
                        // Waveform indicator when speaking
                        waveformIndicator
                    } else {
                        Text("Step \(appState.agentSteps)/15 \u{00B7} \(formatTokens(appState.agentTokens))")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    }
                    Spacer()
                }
                .padding(.horizontal, 12)

            case .completed:
                AgentInputBar(
                    text: $followUpText,
                    placeholder: "Follow up...",
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
                .padding(.horizontal, 12)

                // Settings line
                HStack(spacing: 4) {
                    Text("\u{2713} Done \u{00B7} \(appState.agentSteps) steps \u{00B7} \(formatTokens(appState.agentTokens))")
                        .font(.system(size: 10))
                        .foregroundColor(Color(red: 0.133, green: 0.773, blue: 0.369))
                    Spacer()
                    Button { appState.resetAgent() } label: {
                        Text("New")
                            .font(.system(size: 10, weight: .medium))
                            .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                    }
                    .buttonStyle(.plain)

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
                .padding(.horizontal, 12)

            case .idle:
                EmptyView()
            }
        }
        .padding(.vertical, 8)
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

        case .progress(let msg):
            Text(msg)
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                .padding(.horizontal, 12)

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
        }
    }
}
