import AppKit
import SwiftUI
import UserNotifications
import PagerunnerCore

struct SessionBlockView: View {
    let session: Session
    let index: Int
    let tabs: [PagerunnerCore.Tab]
    @Bindable var appState: AppState
    let controller: StatusItemController
    @Environment(\.daemonClient) private var daemon
    @State private var isCollapsed = false
    @State private var showCloseConfirm = false

    private var checkpointsForSession: [Checkpoint] {
        appState.checkpointsFor(profile: session.profile)
    }

    private var isAlive: Bool { session.status == .alive }

    private var closeButton: some View {
        Button {
            Task { @MainActor in
                _ = try? await daemon.call(tool: "close_session", args: ["session_id": session.id])
            }
        } label: {
            Text("✕")
                .font(.system(size: 9))
                .foregroundColor(isAlive ? .secondary : Color(red: 0.6, green: 0.2, blue: 0.2))
                .frame(width: 18, height: 18)
                .background(isAlive ? Color.primary.opacity(0.07) : Color(red: 0.9, green: 0.2, blue: 0.2).opacity(0.12))
                .clipShape(Circle())
        }
        .buttonStyle(.plain)
        .help(isAlive ? "Close window" : "Dismiss")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if isAlive {
                // Active window — full header with collapse, +, Snapshot, ×
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .medium))
                        .foregroundColor(.secondary)
                        .rotationEffect(isCollapsed ? .degrees(0) : .degrees(90))
                        .animation(.easeInOut(duration: 0.15), value: isCollapsed)

                    Text("Window \(index + 1)")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))

                    Text("Active")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundColor(Color(red: 0.086, green: 0.396, blue: 0.204))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color(red: 0.133, green: 0.773, blue: 0.369).opacity(0.13))
                        .cornerRadius(4)

                    if session.stealth {
                        Text("stealth")
                            .font(.system(size: 9))
                            .foregroundStyle(.tertiary)
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(Color.gray.opacity(0.15))
                            .cornerRadius(3)
                    }

                    Spacer()

                    Button {
                        Task { @MainActor in
                            _ = try? await daemon.call(tool: "new_tab", args: ["session_id": session.id])
                        }
                    } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 10, weight: .medium))
                            .foregroundColor(.secondary)
                            .frame(width: 20, height: 20)
                            .background(Color.primary.opacity(0.07))
                            .clipShape(Circle())
                    }
                    .buttonStyle(.plain)
                    .help("New tab")

                    Button {
                        Task { @MainActor in
                            _ = try? await daemon.call(tool: "save_session_checkpoint", args: ["session_id": session.id])
                        }
                    } label: {
                        Text("Snapshot")
                            .font(.system(size: 10))
                            .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                            .padding(.horizontal, 7)
                            .padding(.vertical, 2)
                            .background(Color(red: 0, green: 0.478, blue: 1).opacity(0.08))
                            .cornerRadius(4)
                            .overlay(RoundedRectangle(cornerRadius: 4)
                                .stroke(Color(red: 0, green: 0.478, blue: 1).opacity(0.25), lineWidth: 0.5))
                    }
                    .buttonStyle(.plain)
                    .help("Save a snapshot of this window to restore later")

                    closeButton
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .contentShape(Rectangle())
                .onTapGesture { withAnimation(.easeInOut(duration: 0.15)) { isCollapsed.toggle() } }
                .contextMenu {
                    Button("Save checkpoint") {
                        Task { @MainActor in
                            _ = try? await daemon.call(
                                tool: "save_session_checkpoint",
                                args: ["session_id": session.id]
                            )
                        }
                    }

                    if !checkpointsForSession.isEmpty {
                        Menu("Restore checkpoint…") {
                            ForEach(checkpointsForSession, id: \.checkpointId) { cp in
                                Button(cp.name.isEmpty ? "Checkpoint \(cp.checkpointId.prefix(6))" : cp.name) {
                                    Task { @MainActor in
                                        _ = try? await daemon.call(
                                            tool: "restore_session_checkpoint",
                                            args: [
                                                "session_id": session.id,
                                                "checkpoint_id": cp.checkpointId
                                            ]
                                        )
                                    }
                                }
                            }
                        }
                    }

                    Divider()

                    Button("View session log") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(
                            "pagerunner audit --session \(session.id)",
                            forType: .string
                        )
                        let content = UNMutableNotificationContent()
                        content.title = "Command copied to clipboard"
                        content.body = "pagerunner audit --session \(session.id.prefix(8))…"
                        let request = UNNotificationRequest(
                            identifier: "clipboard-\(UUID().uuidString)",
                            content: content,
                            trigger: nil
                        )
                        UNUserNotificationCenter.current().add(request)
                    }

                    Divider()

                    Button("Close session", role: .destructive) {
                        showCloseConfirm = true
                    }
                }

                if !isCollapsed {
                    ForEach(tabs) { tab in
                        TabRowView(tab: tab, sessionId: session.id, tabs: tabs, controller: controller)
                    }
                    .padding(.bottom, 4)
                }

            } else {
                // Failed/crashed window — compact single row, only close button
                let everAlive = appState.everAliveSessions.contains(session.id)
                HStack(spacing: 6) {
                    Image(systemName: everAlive ? "exclamationmark.triangle" : "xmark.circle")
                        .font(.system(size: 10))
                        .foregroundColor(everAlive
                            ? Color(red: 0.7, green: 0.45, blue: 0.0)
                            : Color(red: 0.6, green: 0.2, blue: 0.2))

                    Text("Window \(index + 1)")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundColor(Color(red: 0.5, green: 0.5, blue: 0.5))

                    Text(everAlive ? "Chrome process stopped" : "Failed to open")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)

                    Spacer()

                    closeButton
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
            }

            // Bottom separator
            Rectangle()
                .fill(Color.primary.opacity(0.07))
                .frame(height: 0.5)
        }
        .confirmationDialog(
            "Close session?",
            isPresented: $showCloseConfirm,
            titleVisibility: .visible
        ) {
            Button("Close session", role: .destructive) {
                Task { @MainActor in
                    _ = try? await daemon.call(
                        tool: "close_session",
                        args: ["session_id": session.id]
                    )
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This will close all tabs in Window \(index + 1).")
        }
    }
}

struct TabRowView: View {
    let tab: PagerunnerCore.Tab
    let sessionId: String
    let tabs: [PagerunnerCore.Tab]
    let controller: StatusItemController
    @Environment(\.daemonClient) private var daemon
    @State private var isHovered = false

    /// Show host + path from URL, fall back to title
    private var displayText: String {
        if let url = URL(string: tab.url),
           let host = url.host {
            let path = url.path
            if path.isEmpty || path == "/" {
                return host
            }
            let trimmed = path.hasSuffix("/") ? String(path.dropLast()) : path
            return host + trimmed
        }
        return tab.title.isEmpty ? tab.url : tab.title
    }

    private var faviconURL: URL? {
        guard let url = URL(string: tab.url),
              let host = url.host,
              !host.isEmpty else { return nil }
        return URL(string: "https://www.google.com/s2/favicons?domain=\(host)&sz=32")
    }

    var body: some View {
        Button {
            controller.focusTab(sessionId: sessionId, targetId: tab.targetId)
            controller.closePopover()
        } label: {
            HStack(spacing: 6) {
                Group {
                    if let favicon = faviconURL {
                        AsyncImage(url: favicon) { phase in
                            if case .success(let image) = phase {
                                image
                                    .resizable()
                                    .interpolation(.high)
                                    .frame(width: 14, height: 14)
                            } else {
                                Image(systemName: "globe")
                                    .font(.system(size: 11))
                                    .foregroundStyle(.tertiary)
                            }
                        }
                    } else {
                        Image(systemName: "globe")
                            .font(.system(size: 11))
                            .foregroundStyle(.tertiary)
                    }
                }
                .frame(width: 16, alignment: .center)

                Text(displayText)
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.267, green: 0.267, blue: 0.267))
                    .lineLimit(1)
                    .truncationMode(.tail)

                Spacer()

                Button {
                    Task { @MainActor in
                        _ = try? await daemon.call(tool: "close_tab", args: ["session_id": sessionId, "target_id": tab.targetId])
                    }
                } label: {
                    Text("✕")
                        .font(.system(size: 9))
                        .foregroundColor(.secondary)
                        .frame(width: 16, height: 16)
                }
                .buttonStyle(.plain)
                .disabled(tabs.count <= 1)
                .opacity(isHovered ? 1 : 0)
                .help(tabs.count <= 1 ? "Cannot close last tab" : "Close tab")
            }
            .padding(.leading, 28)
            .padding(.trailing, 12)
            .padding(.vertical, 3)
            .background(isHovered ? Color.primary.opacity(0.04) : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
        .contextMenu {
            Button("Focus tab") {
                controller.focusTab(sessionId: sessionId, targetId: tab.targetId)
                controller.closePopover()
            }

            Divider()

            Button("Snapshot this tab") {
                guard let origin = originFrom(url: tab.url) else { return }
                Task { @MainActor in
                    _ = try? await daemon.call(
                        tool: "save_snapshot",
                        args: ["session_id": sessionId, "target_id": tab.targetId, "origin": origin]
                    )
                }
            }

            Button("Copy URL") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(tab.url, forType: .string)
            }

            Divider()

            // Hidden when only one tab — .disabled() is unreliable on context menu buttons in SwiftUI
            if tabs.count > 1 {
                Button("Close tab", role: .destructive) {
                    Task { @MainActor in
                        _ = try? await daemon.call(
                            tool: "close_tab",
                            args: ["session_id": sessionId, "target_id": tab.targetId]
                        )
                    }
                }
            }
        }
    }
}
