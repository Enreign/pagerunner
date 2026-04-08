import SwiftUI
import PagerunnerCore

struct AgentFeedView: View {
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var client
    @State private var isPressing: Bool = false

    var body: some View {
        VStack(spacing: 0) {
            // Header
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Image(systemName: "cpu")
                        .font(.system(size: 14))
                        .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                    Text("Pagerunner Agent")
                        .font(.system(size: 14, weight: .semibold))
                    Spacer()

                    // Mic toggle — PTT uses hold gesture, always-listening uses tap
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
                                .foregroundColor(appState.voiceActive ? Color(red: 0.937, green: 0.267, blue: 0.267) : Color(red: 0.533, green: 0.533, blue: 0.533))
                                .font(.system(size: 14))
                        }
                        .buttonStyle(.plain)
                        .help(appState.voiceActive ? "Stop voice" : "Start voice")
                    }
                }

                HStack(spacing: 4) {
                    Text("Using \(appState.agentModel) \u{00B7} \(appState.agentProfile)")
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    Spacer()
                    if appState.voiceActive {
                        VoiceStatusBadge(status: appState.voiceStatus)
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
            }

            Divider()

            // Bottom bar
            bottomBar
        }
    }

    @ViewBuilder
    private var bottomBar: some View {
        switch appState.agentState {
        case .running:
            HStack {
                Text("Step \(appState.agentSteps)/15 \u{00B7} \(formatTokens(appState.agentTokens))")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                Spacer()
                Button("Stop") {
                    appState.stopAgent(client: client)
                }
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

        case .waitingApproval:
            HStack {
                HStack(spacing: 4) {
                    Image(systemName: "pause.fill")
                        .font(.system(size: 9))
                    Text("Waiting for approval...")
                }
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))
                Spacer()
                Button("Stop") {
                    appState.stopAgent(client: client)
                }
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

        case .completed:
            VStack(spacing: 6) {
                HStack {
                    HStack(spacing: 4) {
                        Image(systemName: "checkmark")
                            .font(.system(size: 10, weight: .bold))
                        Text("Done \u{00B7} \(appState.agentSteps) steps \u{00B7} \(formatTokens(appState.agentTokens))")
                    }
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.133, green: 0.773, blue: 0.369))
                    Spacer()
                }

                HStack(spacing: 8) {
                    Button("New Goal") { appState.resetAgent() }
                        .font(.system(size: 12, weight: .medium))
                        .buttonStyle(.plain)
                    Spacer()
                    if let summary = appState.agentSummary {
                        Button("Copy Result") {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(summary, forType: .string)
                        }
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                        .buttonStyle(.plain)
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

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
            .padding(.vertical, 8)

        case .idle:
            EmptyView()
        }
    }

    private func formatTokens(_ tokens: Int) -> String {
        if tokens >= 1000 {
            return "\(tokens / 1000)K tokens"
        }
        return "\(tokens) tokens"
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
