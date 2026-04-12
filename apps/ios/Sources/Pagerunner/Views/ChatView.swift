import SwiftUI
import PagerunnerKit

struct ChatView: View {
    @Environment(AppState.self) private var appState

    @State private var draft = ""
    @State private var isSending = false
    @State private var showSessions = false
    @State private var inspectorContext: InspectorContext?
    @FocusState private var composerFocused: Bool

    struct InspectorContext: Identifiable, Equatable {
        let id = UUID()
        let sessionId: String
        let targetId: String?
    }

    var body: some View {
        VStack(spacing: 0) {
            transcript
            composer
        }
        .background(Color.operatorBackground.ignoresSafeArea())
        .navigationTitle("Pagerunner")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button {
                    showSessions = true
                } label: {
                    Image(systemName: "macwindow.on.rectangle")
                }
                .accessibilityLabel("Sessions")
            }
        }
        .sheet(isPresented: $showSessions) {
            SessionsSheet(onOpenInspector: { ctx in
                showSessions = false
                inspectorContext = ctx
            })
            .presentationDetents([.medium, .large])
            .presentationDragIndicator(.visible)
        }
        .sheet(item: $inspectorContext) { ctx in
            NavigationStack {
                SessionInspectorView(sessionId: ctx.sessionId, targetId: ctx.targetId)
            }
        }
    }

    // MARK: Transcript

    private var transcript: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: Theme.Spacing.regular) {
                    if appState.chatItems.isEmpty {
                        emptyState
                    } else {
                        ForEach(appState.chatItems) { item in
                            ChatItemView(item: item) { ctx in
                                inspectorContext = ctx
                            }
                            .id(item.id)
                        }
                    }
                }
                .padding(.horizontal, Theme.Spacing.loose)
                .padding(.vertical, Theme.Spacing.regular)
            }
            .onChange(of: appState.chatItems.count) {
                if let last = appState.chatItems.last {
                    withAnimation(.snappy) {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
            }
        }
    }

    private var emptyState: some View {
        VStack(alignment: .leading, spacing: 16) {
            Spacer(minLength: 120)
            Text("Ask Pagerunner anything")
                .font(.title2.bold())
            Text("Try:")
                .font(.footnote)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 10) {
                suggestion("Open GitHub in my work profile")
                suggestion("Summarise unread mail in stas_shymansky")
                suggestion("Screenshot the active tab")
            }
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func suggestion(_ text: String) -> some View {
        Button {
            draft = text
            composerFocused = true
        } label: {
            HStack {
                Image(systemName: "sparkles")
                    .foregroundStyle(.accent)
                Text(text)
                    .foregroundStyle(.primary)
                Spacer()
                Image(systemName: "arrow.up.forward")
                    .foregroundStyle(.secondary)
            }
            .padding(Theme.Spacing.regular)
            .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    // MARK: Composer

    private var composer: some View {
        HStack(spacing: Theme.Spacing.regular) {
            TextField("Ask Pagerunner…", text: $draft, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...5)
                .padding(.horizontal, Theme.Spacing.regular)
                .padding(.vertical, 10)
                .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                .focused($composerFocused)
                .submitLabel(.send)
                .onSubmit(send)

            Button(action: send) {
                Image(systemName: isSending ? "ellipsis" : "arrow.up")
                    .font(.headline.weight(.bold))
                    .frame(width: 44, height: 44)
                    .background(canSend ? Color.accent : Color.accent.opacity(0.3),
                                in: Circle())
                    .foregroundStyle(.white)
            }
            .buttonStyle(.plain)
            .disabled(!canSend)
            .accessibilityLabel("Send message")
        }
        .padding(.horizontal, Theme.Spacing.loose)
        .padding(.vertical, Theme.Spacing.regular)
        .background(Color.operatorBackground)
    }

    private var canSend: Bool {
        !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !isSending
    }

    private func send() {
        guard canSend else { return }
        let text = draft
        draft = ""
        isSending = true
        Task {
            await appState.sendUserMessage(text)
            isSending = false
        }
    }
}

#Preview {
    NavigationStack { ChatView() }
        .environment(AppState())
}
