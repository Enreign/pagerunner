import SwiftUI
import PagerunnerCore

/// Top-level agent view — routes between idle, running, completed, and error states.
struct AgentView: View {
    @Bindable var appState: AppState

    var body: some View {
        switch appState.agentState {
        case .idle:
            AgentIdleView(appState: appState)
        case .running, .waitingApproval, .completed, .error:
            AgentFeedView(appState: appState)
        }
    }
}
