import SwiftUI
import PagerunnerKit

struct ContextsDrawer: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    private let columns = [
        GridItem(.flexible(), spacing: 12),
        GridItem(.flexible(), spacing: 12),
    ]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: Theme.Spacing.section) {
                    if hasAnyTabs {
                        tabsGrid
                    } else {
                        emptyTabsState
                    }
                    profilesSection
                }
                .padding(Theme.Spacing.loose)
            }
            .background(Color.operatorBackground)
            .navigationTitle("Contexts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        appState.setPinnedContext(nil)
                        dismiss()
                    } label: {
                        Label("Unpin", systemImage: "pin.slash")
                    }
                    .disabled(appState.pinnedContext == nil)
                }
            }
            .task {
                for session in appState.aliveSessions {
                    await appState.fetchTabs(for: session.id)
                }
            }
        }
    }

    private var hasAnyTabs: Bool {
        appState.aliveSessions.contains { (appState.tabs[$0.id]?.isEmpty ?? true) == false }
    }

    @ViewBuilder
    private var tabsGrid: some View {
        ForEach(appState.aliveSessions) { session in
            VStack(alignment: .leading, spacing: 8) {
                Text(session.profile)
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.secondary)
                let tabs = appState.tabs[session.id] ?? []
                if tabs.isEmpty {
                    Text("No tabs")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                } else {
                    LazyVGrid(columns: columns, spacing: 12) {
                        ForEach(tabs) { tab in
                            tabTile(session: session, tab: tab)
                        }
                    }
                }
            }
        }
    }

    private func tabTile(session: Session, tab: PagerunnerKit.Tab) -> some View {
        let ctx = PinnedContext(sessionId: session.id, targetId: tab.targetId)
        let thumb = appState.thumbnails.image(for: ctx)
        let isPinned = appState.pinnedContext == ctx

        return Button {
            appState.setPinnedContext(ctx)
            dismiss()
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                ZStack {
                    if let img = thumb {
                        Image(uiImage: img).resizable().scaledToFill()
                    } else {
                        Color.operatorSubtle
                            .overlay(Image(systemName: "photo").foregroundStyle(.tertiary))
                    }
                }
                .frame(height: 110)
                .clipShape(RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous)
                        .stroke(isPinned ? Color.accent : .clear, lineWidth: 2)
                )

                Text(tab.title.isEmpty ? "Untitled" : tab.title)
                    .font(.caption.weight(.medium))
                    .lineLimit(1)
                Text(URL(string: tab.url)?.host() ?? tab.url)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .buttonStyle(.plain)
        .task(id: ctx) {
            if let client = appState.connection.apiClient {
                appState.thumbnails.fetchIfNeeded(ctx, client: client)
            }
        }
    }

    private var emptyTabsState: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("No live tabs yet")
                .font(.subheadline)
            Text("Open a session below to get started.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(Theme.Spacing.regular)
        .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
    }

    private var profilesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Profiles")
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
            ForEach(appState.profiles) { profile in
                Button {
                    Task {
                        try? await appState.openSession(profile: profile.name)
                    }
                } label: {
                    HStack {
                        Image(systemName: "plus.circle.fill")
                            .foregroundStyle(.accent)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(profile.name)
                                .font(.subheadline.weight(.medium))
                            Text(profile.displayName)
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text("\(appState.sessionsForProfile(profile.name).count)")
                            .font(.caption.bold())
                            .foregroundStyle(.accent)
                    }
                    .padding(Theme.Spacing.regular)
                    .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
                }
                .buttonStyle(.plain)
            }
        }
    }
}
