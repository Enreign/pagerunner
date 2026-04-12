import SwiftUI
import PagerunnerKit

struct SessionDetailView: View {
    @Environment(AppState.self) private var appState

    let session: Session

    @State private var screenshotData: Data?
    @State private var showingScreenshot = false
    @State private var showingCheckpointSheet = false
    @State private var checkpointName = ""
    @State private var navigateURL = ""
    @State private var showingNavigateSheet = false
    @State private var navigateTargetId: String?

    private var tabs: [Tab] {
        appState.tabs[session.id] ?? []
    }

    var body: some View {
        List {
            headerSection
            tabsSection
            screenshotSection
        }
        .navigationTitle(session.profile)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItemGroup(placement: .topBarTrailing) {
                Button {
                    Task { try? await appState.newTab(sessionId: session.id) }
                } label: {
                    Image(systemName: "plus.square")
                }

                Button {
                    Task { await takeScreenshot() }
                } label: {
                    Image(systemName: "camera")
                }

                Button {
                    showingCheckpointSheet = true
                } label: {
                    Image(systemName: "bookmark")
                }
            }
        }
        .refreshable {
            await appState.fetchTabs(for: session.id)
        }
        .task {
            await appState.fetchTabs(for: session.id)
        }
        .sheet(isPresented: $showingCheckpointSheet) {
            checkpointSheet
        }
        .sheet(isPresented: $showingNavigateSheet) {
            navigateSheet
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        Section {
            HStack {
                Text("Status")
                Spacer()
                StatusBadge(status: session.status, showLabel: true)
            }

            HStack {
                Text("Profile")
                Spacer()
                Text(session.profile)
                    .foregroundStyle(.secondary)
            }

            HStack {
                Text("Session ID")
                Spacer()
                Text(session.id)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospaced()
                    .lineLimit(1)
                    .truncationMode(.middle)
            }

            if session.stealth {
                HStack {
                    Text("Stealth Mode")
                    Spacer()
                    Image(systemName: "eye.slash.fill")
                        .foregroundStyle(.purple)
                    Text("Enabled")
                        .foregroundStyle(.purple)
                }
            }
        } header: {
            Text("Session Info")
        }
    }

    // MARK: - Tabs

    private var tabsSection: some View {
        Section {
            if tabs.isEmpty {
                Text("No tabs open")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(tabs) { tab in
                    tabRow(tab)
                        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                            Button(role: .destructive) {
                                Task {
                                    try? await appState.closeTab(
                                        sessionId: session.id,
                                        targetId: tab.targetId
                                    )
                                }
                            } label: {
                                Label("Close", systemImage: "xmark.circle")
                            }
                        }
                        .swipeActions(edge: .leading) {
                            Button {
                                navigateTargetId = tab.targetId
                                showingNavigateSheet = true
                            } label: {
                                Label("Navigate", systemImage: "arrow.right")
                            }
                            .tint(.blue)

                            Button {
                                Task { await takeScreenshot(targetId: tab.targetId) }
                            } label: {
                                Label("Screenshot", systemImage: "camera")
                            }
                            .tint(.indigo)
                        }
                }
            }
        } header: {
            HStack {
                Text("Tabs")
                Spacer()
                Text("\(tabs.count)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func tabRow(_ tab: Tab) -> some View {
        HStack(spacing: 12) {
            faviconView(for: tab.url)

            VStack(alignment: .leading, spacing: 4) {
                Text(tab.title.isEmpty ? "Untitled" : tab.title)
                    .font(.subheadline)
                    .lineLimit(1)

                Text(tab.url)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
        .padding(.vertical, 2)
    }

    private func faviconView(for urlString: String) -> some View {
        Group {
            if let url = URL(string: urlString),
               let host = url.host
            {
                AsyncImage(
                    url: URL(string: "https://www.google.com/s2/favicons?domain=\(host)&sz=32")
                ) { image in
                    image.resizable()
                } placeholder: {
                    Image(systemName: "globe")
                        .foregroundStyle(.secondary)
                }
                .frame(width: 20, height: 20)
            } else {
                Image(systemName: "globe")
                    .foregroundStyle(.secondary)
                    .frame(width: 20, height: 20)
            }
        }
    }

    // MARK: - Screenshot

    private var screenshotSection: some View {
        Group {
            if let data = screenshotData, let uiImage = UIImage(data: data) {
                Section("Screenshot") {
                    Button {
                        showingScreenshot.toggle()
                    } label: {
                        Image(uiImage: uiImage)
                            .resizable()
                            .aspectRatio(contentMode: showingScreenshot ? .fit : .fill)
                            .frame(maxHeight: showingScreenshot ? .infinity : 200)
                            .clipped()
                            .clipShape(RoundedRectangle(cornerRadius: 8))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    // MARK: - Actions

    private func takeScreenshot(targetId: String? = nil) async {
        guard let tid = targetId ?? tabs.first?.targetId,
              let client = appState.connection.apiClient else { return }
        do {
            let base64String = try await client.screenshot(
                sessionId: session.id,
                targetId: tid
            )
            screenshotData = Data(base64Encoded: base64String)
        } catch {
            // Ignore screenshot errors
        }
    }

    // MARK: - Sheets

    private var checkpointSheet: some View {
        NavigationStack {
            Form {
                Section("Checkpoint Name") {
                    TextField("Optional name", text: $checkpointName)
                }
            }
            .navigationTitle("Save Checkpoint")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { showingCheckpointSheet = false }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        Task {
                            try? await appState.saveCheckpoint(
                                sessionId: session.id,
                                name: checkpointName.isEmpty ? nil : checkpointName
                            )
                            checkpointName = ""
                            showingCheckpointSheet = false
                        }
                    }
                }
            }
        }
        .presentationDetents([.medium])
    }

    private var navigateSheet: some View {
        NavigationStack {
            Form {
                Section("URL") {
                    TextField("https://example.com", text: $navigateURL)
                        .textContentType(.URL)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                }
            }
            .navigationTitle("Navigate")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        showingNavigateSheet = false
                        navigateURL = ""
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Go") {
                        guard let targetId = navigateTargetId else { return }
                        Task {
                            try? await appState.navigate(
                                sessionId: session.id,
                                targetId: targetId,
                                url: navigateURL
                            )
                            navigateURL = ""
                            showingNavigateSheet = false
                            await appState.fetchTabs(for: session.id)
                        }
                    }
                    .disabled(navigateURL.isEmpty)
                }
            }
        }
        .presentationDetents([.medium])
    }
}

#Preview {
    NavigationStack {
        SessionDetailView(
            session: Session(
                id: "abc123-def456",
                profile: "personal",
                displayName: "Personal",
                stealth: false,
                status: .alive
            )
        )
    }
    .environment(AppState())
}
