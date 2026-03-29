import SwiftUI
import PagerunnerCore

/// Default home screen: scrollable list of all profiles, two sections.
struct OverviewView: View {
    @Bindable var appState: AppState

    // MARK: - Sheet state for Rename / Remove
    @State private var profileToRename: Profile? = nil
    @State private var profileToRemove: Profile? = nil
    @State private var showRenameSheet = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Your profiles section — always visible
            HStack {
                Text("Your profiles")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
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

            if appState.personalProfiles.isEmpty {
                VStack(spacing: 4) {
                    Text("No profiles yet")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.secondary)
                    Text("Tap + to add a Chrome profile")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.vertical, 12)
            } else {
                ForEach(Array(appState.personalProfiles.enumerated()), id: \.element.id) { index, profile in
                    ProfileRowView(
                        profile: profile,
                        index: index,
                        onRename: {
                            profileToRename = profile
                            showRenameSheet = true
                        },
                        onRemove: {
                            profileToRemove = profile
                        },
                        appState: appState
                    )
                }
            }

            // Divider between sections
            Rectangle()
                .fill(Color.primary.opacity(0.08))
                .frame(height: 0.5)
                .padding(.vertical, 4)

            // Agent profiles section — always visible
            HStack {
                Text("Agent profiles")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
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

            if appState.agentProfiles.isEmpty {
                VStack(spacing: 4) {
                    Text("No agent profiles yet")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.secondary)
                    Text("Tap + to create a dedicated Chrome profile")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.vertical, 12)
            } else {
                ForEach(Array(appState.agentProfiles.enumerated()), id: \.element.id) { index, profile in
                    ProfileRowView(
                        profile: profile,
                        index: index,
                        onRename: {
                            profileToRename = profile
                            showRenameSheet = true
                        },
                        onRemove: {
                            profileToRemove = profile
                        },
                        appState: appState
                    )
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
        .onAppear {
            appState.triggerDiscovery()  // pre-load for when user opens + menu
        }
        // MARK: - Rename sheet
        .sheet(isPresented: $showRenameSheet) {
            if let profile = profileToRename {
                RenameSheet(
                    title: "Rename Profile",
                    prompt: "Enter a new display name for \"\(profile.displayName)\"",
                    isPresented: $showRenameSheet,
                    initialValue: profile.displayName
                ) { newName in
                    Task {
                        try? await appState.renameProfile(profile, newDisplayName: newName)
                    }
                }
            }
        }
        // MARK: - Remove confirmation
        .confirmationDialog(
            "Remove \"\(profileToRemove?.displayName ?? "")\"?",
            isPresented: Binding(
                get: { profileToRemove != nil },
                set: { if !$0 { profileToRemove = nil } }
            ),
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                if let profile = profileToRemove {
                    Task {
                        try? await appState.removeProfile(profile)
                    }
                }
                profileToRemove = nil
            }
            Button("Cancel", role: .cancel) {
                profileToRemove = nil
            }
        } message: {
            Text("This will remove the profile from pagerunner. The Chrome profile data will not be deleted.")
        }
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
