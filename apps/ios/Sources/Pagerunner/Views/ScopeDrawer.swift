import SwiftUI
import PagerunnerKit

struct ScopeDrawer: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var goalDraft: String = ""
    @State private var notesDraft: String = ""
    @State private var showPicker = false
    @State private var showNotes = false

    private var scope: Scope {
        appState.currentThread?.scope ?? Scope()
    }

    var body: some View {
        NavigationStack {
            List {
                goalSection
                if showNotes || scope.notes != nil {
                    notesSection
                }
                tabsSection
                turnLogSection
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(.thinMaterial)
            .navigationTitle("Scope")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        if !showNotes && scope.notes == nil {
                            Button("Add notes") { showNotes = true }
                        }
                        Button("Clear scope", role: .destructive) {
                            appState.setScope(Scope())
                        }
                        .disabled(scope.tabs.isEmpty && scope.goal == nil && scope.notes == nil)
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                }
            }
            .sheet(isPresented: $showPicker) {
                ScopeTabPickerView()
                    .presentationDetents([.medium, .large])
            }
            .onAppear {
                goalDraft = scope.goal ?? ""
                notesDraft = scope.notes ?? ""
            }
        }
    }

    // MARK: - Sections

    private var goalSection: some View {
        Section {
            TextField("Goal for this thread (optional)", text: $goalDraft, axis: .vertical)
                .lineLimit(1...2)
                .onSubmit { appState.updateScopeGoal(goalDraft) }
                .onChange(of: goalDraft) { _, new in
                    if new.count > 200 { goalDraft = String(new.prefix(200)) }
                }
        } header: {
            Text("Goal")
        } footer: {
            Text("What you want the agent to accomplish across these tabs.")
                .font(.caption2)
        }
    }

    private var notesSection: some View {
        Section {
            TextField("Notes (optional)", text: $notesDraft, axis: .vertical)
                .lineLimit(3...8)
                .onSubmit { appState.updateScopeNotes(notesDraft) }
                .onChange(of: notesDraft) { _, new in
                    if new.count > 2000 { notesDraft = String(new.prefix(2000)) }
                }
        } header: {
            Text("Notes")
        }
    }

    private var tabsSection: some View {
        Section {
            if scope.tabs.isEmpty {
                Button {
                    showPicker = true
                } label: {
                    Label("Add tabs", systemImage: "plus.circle.fill")
                        .foregroundStyle(.accent)
                }
            } else {
                ForEach(scope.tabs) { tab in
                    tabRow(tab)
                }
                Button {
                    showPicker = true
                } label: {
                    Label("Add tab", systemImage: "plus")
                        .foregroundStyle(.accent)
                }
            }
        } header: {
            HStack {
                Text("Tabs")
                Spacer()
                if scope.tabs.count > 0 {
                    Text("\(scope.tabs.count)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                if scope.tabs.count > 8 {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .help("More than 8 tabs — agent prompt may degrade")
                }
            }
        }
    }

    private func tabRow(_ tab: ScopeTab) -> some View {
        let ctx = PinnedContext(sessionId: tab.sessionId, targetId: tab.targetId)
        let thumb = appState.thumbnails.image(for: ctx)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Group {
                    if let img = thumb {
                        Image(uiImage: img).resizable().scaledToFill()
                    } else {
                        Color.operatorSubtle
                            .overlay(Image(systemName: "photo").font(.caption2).foregroundStyle(.tertiary))
                    }
                }
                .frame(width: 40, height: 40)
                .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))

                VStack(alignment: .leading, spacing: 2) {
                    Text(tab.label.isEmpty ? "Untitled tab" : tab.label)
                        .font(.subheadline.weight(.medium))
                        .lineLimit(1)
                    purposeField(for: tab)
                }
                Spacer()
            }
            if let digest = tab.digest {
                Text(digest)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
                if let touched = tab.lastTouchedAt {
                    Text("touched \(touched, style: .relative) ago")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) {
                appState.removeTabFromScope(tabId: tab.id)
            } label: {
                Label("Remove", systemImage: "trash")
            }
        }
        .task(id: ctx) {
            if let client = appState.connection.apiClient {
                appState.thumbnails.fetchIfNeeded(ctx, client: client)
            }
        }
    }

    private func purposeField(for tab: ScopeTab) -> some View {
        PurposeEditor(initial: tab.purpose ?? "", tabId: tab.id)
    }

    private var turnLogSection: some View {
        Section {
            if scope.turnLog.isEmpty {
                Text("No turns logged yet.")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            } else {
                NavigationLink {
                    TurnLogView()
                } label: {
                    HStack {
                        Label("Turn log", systemImage: "list.bullet.rectangle")
                        Spacer()
                        Text("\(scope.turnLog.count)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }
}

/// Inline editor for a tab's `purpose`. Commits on submit or focus loss.
private struct PurposeEditor: View {
    @Environment(AppState.self) private var appState
    @FocusState private var focused: Bool
    @State private var text: String
    let tabId: String

    init(initial: String, tabId: String) {
        self._text = State(initialValue: initial)
        self.tabId = tabId
    }

    var body: some View {
        TextField("purpose (optional)", text: $text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .focused($focused)
            .onSubmit { commit() }
            .onChange(of: focused) { _, isFocused in
                if !isFocused { commit() }
            }
            .onChange(of: text) { _, new in
                if new.count > 60 { text = String(new.prefix(60)) }
            }
    }

    private func commit() {
        appState.updateTabPurpose(tabId: tabId, purpose: text.isEmpty ? nil : text)
    }
}
