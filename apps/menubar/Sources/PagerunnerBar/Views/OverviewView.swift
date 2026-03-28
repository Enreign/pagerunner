import SwiftUI
import PagerunnerCore

/// Default home screen: scrollable list of all profiles, two sections.
struct OverviewView: View {
    @Bindable var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if !appState.personalProfiles.isEmpty {
                // Section label (spec: ov-section-label)
                HStack {
                    Text("Your profiles")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533)) // #888
                        .textCase(.uppercase)
                        .tracking(0.5)
                    Spacer()
                    Button { appState.navigation = .addProfile } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                            .frame(width: 18, height: 18)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("Add profile")
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 4)

                ForEach(Array(appState.personalProfiles.enumerated()), id: \.element.id) { index, profile in
                    ProfileRow(profile: profile, index: index, appState: appState)
                }
            }

            if !appState.personalProfiles.isEmpty && !appState.agentProfiles.isEmpty {
                // Divider between sections (spec: ov-divider)
                Rectangle()
                    .fill(Color.primary.opacity(0.08))
                    .frame(height: 0.5)
                    .padding(.vertical, 4)
            }

            if !appState.agentProfiles.isEmpty {
                HStack {
                    Text("Agent profiles")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533)) // #888
                        .textCase(.uppercase)
                        .tracking(0.5)
                    Spacer()
                    Button { appState.navigation = .addProfile } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                            .frame(width: 18, height: 18)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("Add agent profile")
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 4)

                ForEach(Array(appState.agentProfiles.enumerated()), id: \.element.id) { index, profile in
                    ProfileRow(profile: profile, index: index, appState: appState)
                }
            }

            if appState.profiles.isEmpty {
                Text("No profiles configured")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.top, 20)
            }
        }
    }
}

struct ProfileRow: View {
    let profile: Profile
    let index: Int
    @Bindable var appState: AppState
    @State private var isHovered = false
    private var sessions: [Session] { appState.sessionsFor(profile: profile.name) }
    private var aliveSessions: [Session] { sessions.filter { $0.status == .alive } }

    /// Parse "growthmate.io (stas@growthmate.io)" → name: "growthmate.io", email: "stas@growthmate.io"
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

                // Right side: session count text badge + chevron (spec: ov-sessions + ov-chevron)
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
    }

}

/// Profile icon that shows Chrome profile picture if available, gradient fallback otherwise.
struct ProfileIcon: View {
    let profile: Profile
    let index: Int
    let size: CGFloat

    private var profileImage: NSImage? {
        guard let dir = profile.userDataDir else { return nil }
        // Chrome stores the Google account photo here
        let path = (dir as NSString).appendingPathComponent("Google Profile Picture.png")
        return NSImage(contentsOfFile: path)
    }

    var body: some View {
        if let image = profileImage {
            Image(nsImage: image)
                .resizable()
                .aspectRatio(contentMode: .fill)
                .frame(width: size, height: size)
                .clipShape(Circle())
        } else if profile.kind == "agent" {
            ZStack {
                Circle()
                    .fill(Color(white: 0.82))
                    .frame(width: size, height: size)
                Image(systemName: "cpu")
                    .font(.system(size: size * 0.45, weight: .medium))
                    .foregroundColor(Color(white: 0.38))
            }
        } else {
            Circle()
                .fill(profileGradient(index: index))
                .frame(width: size, height: size)
        }
    }
}
