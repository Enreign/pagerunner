import SwiftUI
import PagerunnerKit

/// Push-in view for a single session (optionally pinned to a specific tab).
/// Shows a live screenshot, URL, and the recent event timeline.
struct SessionInspectorView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    let sessionId: String
    let targetId: String?

    @State private var screenshot: UIImage?
    @State private var screenshotError: String?
    @State private var isLoadingScreenshot = false
    @State private var refreshTask: Task<Void, Never>?
    @State private var selectedTab: PagerunnerKit.Tab?

    private var session: Session? {
        appState.sessions.first { $0.id == sessionId }
    }

    private var tabs: [PagerunnerKit.Tab] {
        appState.tabs[sessionId] ?? []
    }

    private var activeTab: PagerunnerKit.Tab? {
        selectedTab
            ?? tabs.first(where: { $0.targetId == targetId })
            ?? tabs.first
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.section) {
                header
                screenshotCard
                tabStrip
                eventsSection
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.vertical, Theme.Spacing.regular)
        }
        .background(Color.operatorBackground)
        .navigationTitle(session?.profile ?? "Session")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    Task { await takeScreenshot(force: true) }
                } label: {
                    if isLoadingScreenshot {
                        ProgressView()
                    } else {
                        Image(systemName: "arrow.clockwise")
                    }
                }
                .accessibilityLabel("Refresh screenshot")
            }
        }
        .task {
            await appState.fetchTabs(for: sessionId)
            await takeScreenshot(force: false)
        }
        .onDisappear { refreshTask?.cancel() }
    }

    // MARK: Header

    private var header: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.tight) {
            HStack(spacing: 10) {
                StatusDot(state: session?.status == .alive ? .live : .error)
                Text(session?.displayName ?? sessionId)
                    .font(.headline)
                Spacer()
            }
            if let url = activeTab?.url {
                Text(url)
                    .font(.monoCaption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }

    // MARK: Screenshot

    private var screenshotCard: some View {
        Card(padding: Theme.Spacing.tight) {
            ZStack {
                if let img = screenshot {
                    Image(uiImage: img)
                        .resizable()
                        .scaledToFit()
                        .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
                } else if isLoadingScreenshot {
                    ProgressView()
                        .padding(60)
                } else if let err = screenshotError {
                    VStack(spacing: 10) {
                        Image(systemName: "photo.on.rectangle.angled")
                            .font(.largeTitle)
                            .foregroundStyle(.secondary)
                        Text(err)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                    }
                    .padding(Theme.Spacing.loose)
                } else {
                    VStack(spacing: 10) {
                        Image(systemName: "photo")
                            .font(.largeTitle)
                            .foregroundStyle(.secondary)
                        Text("No screenshot yet")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    .padding(Theme.Spacing.loose)
                }
            }
            .frame(maxWidth: .infinity)
        }
    }

    // MARK: Tabs

    @ViewBuilder
    private var tabStrip: some View {
        if !tabs.isEmpty {
            VStack(alignment: .leading, spacing: Theme.Spacing.tight) {
                SectionLabel(text: "TABS · \(tabs.count)")
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(tabs) { tab in
                            tabChip(tab)
                        }
                    }
                }
            }
        }
    }

    private func tabChip(_ tab: PagerunnerKit.Tab) -> some View {
        let isSelected = activeTab?.targetId == tab.targetId
        return Button {
            selectedTab = tab
            Task { await takeScreenshot(force: true) }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "globe")
                    .font(.caption2)
                Text(tab.title.isEmpty ? tab.url : tab.title)
                    .font(.caption)
                    .lineLimit(1)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                isSelected ? Color.accent.opacity(0.2) : Color.operatorCard,
                in: Capsule()
            )
            .foregroundStyle(isSelected ? Color.accent : .primary)
        }
        .buttonStyle(.plain)
    }

    // MARK: Events

    private var eventsSection: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.tight) {
            SectionLabel(text: "RECENT EVENTS")
            let events = recentEvents()
            if events.isEmpty {
                Card(padding: Theme.Spacing.regular) {
                    Text("No agent events yet.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            } else {
                Card(padding: Theme.Spacing.regular) {
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(events) { event in
                            eventLine(event)
                        }
                    }
                }
            }
        }
    }

    private func recentEvents() -> [IdentifiableAgentEvent] {
        Array(appState.agentEvents.suffix(20))
    }

    private func eventLine(_ wrapped: IdentifiableAgentEvent) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: iconFor(wrapped.detail))
                .font(.caption)
                .foregroundStyle(colorFor(wrapped.detail))
                .frame(width: 16)
                .padding(.top, 3)
            Text(textFor(wrapped.detail))
                .font(.monoCaption)
                .foregroundStyle(.primary)
                .lineLimit(2)
            Spacer(minLength: 0)
        }
    }

    private func iconFor(_ e: AgentEventDetail) -> String {
        switch e {
        case .thinking: "text.bubble"
        case .toolCall: "play.fill"
        case .toolResult: "checkmark"
        case .approvalRequired: "hand.raised"
        case .done: "checkmark.seal"
        case .error: "exclamationmark.triangle"
        case .progress: "ellipsis"
        case .interrupted: "pause.fill"
        case .budgetExceeded: "gauge.with.dots.needle.bottom.50percent"
        case .approvalResponse: "checkmark.message"
        case .scopeDigest: "doc.text.magnifyingglass"
        case .turnSummary: "list.bullet.clipboard"
        case .unknown: "questionmark"
        }
    }

    private func colorFor(_ e: AgentEventDetail) -> Color {
        switch e {
        case .error, .interrupted, .budgetExceeded: .red
        case .approvalRequired:                     .yellow
        case .done, .toolResult:                    .accent
        default:                                    .secondary
        }
    }

    private func textFor(_ e: AgentEventDetail) -> String {
        switch e {
        case .thinking(let t):              return t
        case .toolCall(let n, _):           return n
        case .toolResult(let n, _, _):      return "\(n) ok"
        case .progress(let m):              return m
        case .approvalRequired(_, let a, _):return "approval: \(a)"
        case .approvalResponse(_, let ok):  return ok ? "approved" : "denied"
        case .done(let s):                  return s.isEmpty ? "done" : s
        case .error(let m, _):              return m
        case .interrupted:                  return "interrupted"
        case .budgetExceeded(let r):        return "budget: \(r)"
        case .scopeDigest(_, _, let d):     return d
        case .turnSummary(let s, _):        return s
        case .unknown(let t):               return t
        }
    }

    // MARK: Actions

    private func takeScreenshot(force: Bool) async {
        guard let client = appState.connection.apiClient,
              let target = activeTab?.targetId ?? targetId else { return }
        if !force && screenshot != nil { return }
        isLoadingScreenshot = true
        screenshotError = nil
        do {
            let base64 = try await client.screenshot(sessionId: sessionId, targetId: target)
            if let data = Data(base64Encoded: base64),
               let img = UIImage(data: data) {
                screenshot = img
            } else {
                screenshotError = "Could not decode screenshot"
            }
        } catch {
            screenshotError = error.localizedDescription
        }
        isLoadingScreenshot = false
    }
}
