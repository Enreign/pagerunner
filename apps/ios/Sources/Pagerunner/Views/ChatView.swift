import SwiftUI
import PagerunnerKit

struct ChatView: View {
    @Environment(AppState.self) private var appState

    @State private var draft = ""
    @State private var isSending = false
    @State private var showThreads = false
    @State private var showScope = false
    @State private var showSettings = false
    @State private var inspectorContext: InspectorContext?
    @State private var fullscreenScreenshot: ChatItemView.FullscreenScreenshot?
    @FocusState private var composerFocused: Bool

    struct InspectorContext: Identifiable, Equatable {
        let id = UUID()
        let sessionId: String
        let targetId: String?
    }

    var body: some View {
        VStack(spacing: 0) {
            ScopeChip { showScope = true }
                .padding(.horizontal, Theme.Spacing.loose)
                .padding(.top, Theme.Spacing.tight)
                .padding(.bottom, Theme.Spacing.tight)
            transcript
            composer
        }
        .background(Color.operatorBackground.ignoresSafeArea())
        .navigationTitle("Pagerunner")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button {
                    showThreads = true
                } label: {
                    Image(systemName: "line.3.horizontal")
                }
                .accessibilityLabel("Threads")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    showSettings = true
                } label: {
                    Image(systemName: "gearshape")
                }
                .accessibilityLabel("Settings")
            }
        }
        .sheet(isPresented: $showThreads) {
            ThreadsDrawer()
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showScope) {
            ScopeDrawer()
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
        }
        .sheet(isPresented: $showSettings) {
            NavigationStack { SettingsView() }
        }
        .sheet(item: $inspectorContext) { ctx in
            NavigationStack {
                SessionInspectorView(sessionId: ctx.sessionId, targetId: ctx.targetId)
            }
        }
        .fullScreenCover(item: $fullscreenScreenshot) { shot in
            ScreenshotFullscreenView(image: shot.image, caption: shot.caption)
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
                            ChatItemView(item: item, onOpenInspector: { ctx in
                                inspectorContext = ctx
                            }, onOpenFullscreen: { shot in
                                fullscreenScreenshot = shot
                            })
                            .id(item.id)
                        }
                        if appState.isAgentRunning {
                            workingRow
                                .id("working-indicator")
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

    private var workingRow: some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: "figure.run")
                .font(.caption)
                .foregroundStyle(.accent)
                .frame(width: 24, height: 24)
                .background(.operatorCard, in: Circle())
            HStack(spacing: 4) {
                Circle().fill(.accent).frame(width: 6, height: 6).animatedDot(offset: 0)
                Circle().fill(.accent).frame(width: 6, height: 6).animatedDot(offset: 0.2)
                Circle().fill(.accent).frame(width: 6, height: 6).animatedDot(offset: 0.4)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 10)
            .background(.operatorCard, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            Spacer()
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
        GlassEffectContainer(spacing: 12) {
            HStack(spacing: Theme.Spacing.regular) {
                TextField("Ask Pagerunner…", text: $draft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .lineLimit(1...5)
                    .padding(.horizontal, Theme.Spacing.regular)
                    .padding(.vertical, 10)
                    .glassEffect(.regular.interactive(), in: .capsule)
                    .focused($composerFocused)
                    .submitLabel(.send)
                    .onSubmit(send)

                Button(action: send) {
                    Image(systemName: isSending ? "ellipsis" : "arrow.up")
                        .font(.headline.weight(.bold))
                        .frame(width: 44, height: 44)
                        .foregroundStyle(.white)
                }
                .buttonStyle(.glassProminent)
                .tint(canSend ? .accent : .accent.opacity(0.3))
                .disabled(!canSend)
                .accessibilityLabel("Send message")
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.vertical, Theme.Spacing.regular)
        }
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
