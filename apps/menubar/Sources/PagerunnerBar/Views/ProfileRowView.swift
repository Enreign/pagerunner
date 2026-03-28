import SwiftUI
import PagerunnerCore

/// A single profile row shown in OverviewView.
/// Includes a right-click context menu with Rename… and Remove… actions.
struct ProfileRowView: View {
    let profile: Profile
    let index: Int
    let onRename: () -> Void
    let onRemove: () -> Void

    @Bindable var appState: AppState
    @State private var isHovered = false

    private var sessions: [Session] { appState.sessionsFor(profile: profile.name) }
    private var aliveSessions: [Session] { sessions.filter { $0.status == .alive } }

    /// Parse "growthmate.io (stas@growthmate.io)" → name: "growthmate.io"
    private var profileName: String {
        if let parenStart = profile.displayName.firstIndex(of: "(") {
            return String(profile.displayName[..<parenStart]).trimmingCharacters(in: .whitespaces)
        }
        return profile.displayName
    }

    private var profileEmail: String? {
        guard let parenStart = profile.displayName.firstIndex(of: "("),
              let parenEnd = profile.displayName.lastIndex(of: ")") else { return nil }
        let start = profile.displayName.index(after: parenStart)
        return String(profile.displayName[start..<parenEnd])
    }

    var body: some View {
        Button {
            appState.navigation = .profile(profile.name)
        } label: {
            HStack(spacing: 9) {
                // Profile icon with status dot
                ProfileIcon(profile: profile, index: index, size: 32)
                    .overlay(alignment: .bottomTrailing) {
                        Circle()
                            .fill(aliveSessions.isEmpty
                                  ? Color(white: 0.33)
                                  : Color(red: 0.133, green: 0.773, blue: 0.369))
                            .frame(width: 7, height: 7)
                            .overlay(Circle().stroke(Color(red: 228/255, green: 228/255, blue: 228/255), lineWidth: 1.5))
                            .offset(x: 1, y: 1)
                    }

                // Name + email
                VStack(alignment: .leading, spacing: 1) {
                    Text(profileName)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133)) // #222
                    if let email = profileEmail {
                        Text(email)
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533)) // #888
                            .lineLimit(1)
                    }
                }

                Spacer()

                // Right side: session count text badge + chevron
                HStack(spacing: 6) {
                    if !aliveSessions.isEmpty {
                        Text("\(aliveSessions.count) window\(aliveSessions.count == 1 ? "" : "s")")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.086, green: 0.396, blue: 0.204)) // #166534
                            .padding(.horizontal, 7)
                            .padding(.vertical, 1)
                            .background(Color(red: 0.133, green: 0.773, blue: 0.369).opacity(0.12))
                            .cornerRadius(10)
                    } else {
                        Text("idle")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.33, green: 0.33, blue: 0.33)) // #555
                            .padding(.horizontal, 7)
                            .padding(.vertical, 1)
                            .background(Color.black.opacity(0.08))
                            .cornerRadius(10)
                    }

                    Text("›")
                        .font(.system(size: 11))
                        .foregroundColor(Color(white: 0.733)) // #bbb
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(
                RoundedRectangle(cornerRadius: 4)
                    .fill(isHovered ? Color.black.opacity(0.04) : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
        .contextMenu {
            Button("Rename…") { onRename() }
            Divider()
            Button(role: .destructive) {
                onRemove()
            } label: {
                Label("Remove…", systemImage: "trash")
            }
        }
    }
}
