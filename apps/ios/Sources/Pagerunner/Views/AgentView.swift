import SwiftUI
import PagerunnerKit

struct AgentView: View {
    @Environment(AppState.self) private var appState

    @State private var goalText = ""
    @State private var isRunning = false

    var body: some View {
        VStack(spacing: 0) {
            eventFeed
            Divider()
            statusBar
            Divider()
            goalInput
        }
        .navigationTitle("Agent")
    }

    // MARK: - Event Feed

    private var eventFeed: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 8) {
                    if appState.agentEvents.isEmpty {
                        ContentUnavailableView(
                            "No Events",
                            systemImage: "cpu",
                            description: Text("Enter a goal and tap Run to start the agent.")
                        )
                        .padding(.top, 60)
                    } else {
                        ForEach(appState.agentEvents) { event in
                            EventRow(event: event.detail) {
                                approveEvent(event)
                            } onDeny: {
                                denyEvent(event)
                            }
                            .id(event.id)
                        }
                    }
                }
                .padding()
            }
            .onChange(of: appState.agentEvents.count) {
                if let lastEvent = appState.agentEvents.last {
                    withAnimation {
                        proxy.scrollTo(lastEvent.id, anchor: .bottom)
                    }
                }
            }
        }
    }

    // MARK: - Status Bar

    private var statusBar: some View {
        HStack {
            if isRunning {
                ProgressView()
                    .scaleEffect(0.8)
                Text("Running...")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "cpu")
                    .foregroundStyle(.secondary)
                Text("Idle")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Text("\(appState.agentEvents.count) events")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color(.secondarySystemBackground))
    }

    // MARK: - Goal Input

    private var goalInput: some View {
        HStack(spacing: 12) {
            TextField("Describe a goal...", text: $goalText, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...4)
                .padding(10)
                .background(Color(.tertiarySystemFill))
                .clipShape(RoundedRectangle(cornerRadius: 12))

            Button {
                runAgent()
            } label: {
                Image(systemName: "play.fill")
                    .font(.title3)
                    .frame(width: 44, height: 44)
                    .background(goalText.isEmpty || isRunning ? Color.gray : Color.accentColor)
                    .foregroundStyle(.white)
                    .clipShape(Circle())
            }
            .disabled(goalText.isEmpty || isRunning)
        }
        .padding()
        .background(Color(.systemBackground))
    }

    // MARK: - Actions

    private func runAgent() {
        guard !goalText.isEmpty, let client = appState.connection.apiClient else { return }
        isRunning = true
        let goal = goalText
        goalText = ""

        Task {
            do {
                _ = try await client.callTool("agent_run", args: ["goal": goal])
            } catch {
                // Handle error
            }
            isRunning = false
        }
    }

    private func approveEvent(_ event: IdentifiableAgentEvent) {
        guard let client = appState.connection.apiClient else { return }
        if case .approvalRequired(let runId, _, _) = event.detail {
            Task {
                _ = try? await client.callTool(
                    "agent_approve",
                    args: ["run_id": runId, "approved": true]
                )
                appState.pendingApproval = nil
            }
        }
    }

    private func denyEvent(_ event: IdentifiableAgentEvent) {
        guard let client = appState.connection.apiClient else { return }
        if case .approvalRequired(let runId, _, _) = event.detail {
            Task {
                _ = try? await client.callTool(
                    "agent_approve",
                    args: ["run_id": runId, "approved": false]
                )
                appState.pendingApproval = nil
            }
        }
    }
}

#Preview {
    NavigationStack {
        AgentView()
    }
    .environment(AppState())
}
