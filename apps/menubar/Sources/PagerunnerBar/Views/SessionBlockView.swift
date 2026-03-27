import SwiftUI
import PagerunnerCore

struct SessionBlockView: View {
    let session: Session
    let tabs: [PagerunnerCore.Tab]
    @Bindable var appState: AppState
    let controller: StatusItemController

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Session header
            HStack {
                // Status badge
                statusBadge

                Text(session.id.prefix(8))
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(.secondary)
                if session.stealth {
                    Text("stealth")
                        .font(.system(size: 9))
                        .foregroundStyle(.tertiary)
                        .padding(.horizontal, 4)
                        .background(Color.gray.opacity(0.15))
                        .cornerRadius(3)
                }

                Spacer()

                // Save checkpoint button
                Button {
                    // TODO: call save_session_checkpoint
                } label: {
                    Image(systemName: "square.and.arrow.down")
                        .font(.system(size: 11))
                }
                .buttonStyle(.plain)
                .help("Save checkpoint")

                // Close session button
                Button {
                    // TODO: call close_session
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 10))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .help("Close session")
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .background(Color.gray.opacity(0.05))
            .cornerRadius(8)

            // Tab rows
            if !tabs.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(tabs) { tab in
                        TabRowView(
                            tab: tab,
                            sessionId: session.id,
                            tabs: tabs,
                            controller: controller
                        )
                    }
                }
                .padding(.leading, 8)
                .padding(.top, 2)
            }
        }
    }

    private var statusBadge: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(session.status == .alive ? Color.green : Color.red)
                .frame(width: 6, height: 6)
            Text(session.status == .alive ? "active" : "dead")
                .font(.system(size: 9))
                .foregroundColor(session.status == .alive ? .green : .red)
        }
        .padding(.horizontal, 5)
        .padding(.vertical, 2)
        .background(
            (session.status == .alive ? Color.green : Color.red).opacity(0.1)
        )
        .cornerRadius(4)
    }
}

struct TabRowView: View {
    let tab: PagerunnerCore.Tab
    let sessionId: String
    let tabs: [PagerunnerCore.Tab]
    let controller: StatusItemController
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 6) {
            // Favicon placeholder (future: AsyncImage from favicon URL)
            Image(systemName: "globe")
                .font(.system(size: 9))
                .foregroundStyle(.tertiary)
                .frame(width: 12)

            Text(tab.title.isEmpty ? tab.url : tab.title)
                .font(.system(size: 10))
                .lineLimit(1)
                .truncationMode(.middle)

            Spacer()

            // Close button (visible on hover, disabled if last tab)
            if isHovered {
                Button {
                    // TODO: call close_tab
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 9))
                }
                .buttonStyle(.plain)
                .disabled(tabs.count <= 1)
                .help(tabs.count <= 1 ? "Cannot close last tab" : "Close tab")
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(isHovered ? Color.gray.opacity(0.08) : Color.clear)
        .cornerRadius(4)
        .onHover { isHovered = $0 }
        .onTapGesture {
            controller.focusTab(url: tab.url)
            controller.closePopover()
        }
    }
}
