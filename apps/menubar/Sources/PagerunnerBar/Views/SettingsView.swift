import SwiftUI
import PagerunnerCore

struct SettingsView: View {
    @Bindable var appState: AppState

    @State private var profileToRename: Profile? = nil
    @State private var showRenameSheet = false
    @State private var profileToRemove: Profile? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            backHeader
            contentArea
            Spacer()
        }
        .sheet(isPresented: $showRenameSheet) {
            if let profile = profileToRename {
                RenameSheet(
                    title: "Rename Profile",
                    prompt: "Enter a new name for \"\(profile.displayName)\"",
                    isPresented: $showRenameSheet,
                    initialValue: profile.displayName,
                    onConfirm: { newName in
                        Task {
                            try? await appState.renameProfile(profile, newDisplayName: newName)
                        }
                    }
                )
            }
        }
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

    @ViewBuilder
    private var backHeader: some View {
        HStack(spacing: 6) {
            Button { appState.navigation = .overview } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
            }
            .buttonStyle(.plain)
            Text("Settings")
                .font(.system(size: 12, weight: .semibold))
                .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))
            Spacer()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
        .background(Color(red: 0, green: 0.478, blue: 1).opacity(0.06))
        .overlay(alignment: .top) {
            Rectangle().fill(Color.black.opacity(0.08)).frame(height: 0.5)
        }
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.black.opacity(0.08)).frame(height: 0.5)
        }
    }

    @ViewBuilder
    private var contentArea: some View {
        VStack(alignment: .leading, spacing: 16) {
            if !appState.profiles.isEmpty {
                profilesSection
                Divider()
            }
            behaviorSection
            Divider()
            notificationsSection
        }
        .padding(12)
    }

    @ViewBuilder
    private var profilesSection: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("PROFILES")
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(.secondary)
                .kerning(0.5)

            ForEach(Array(appState.profiles.enumerated()), id: \.element.id) { _, profile in
                HStack {
                    Circle()
                        .fill(Color.accentColor)
                        .frame(width: 8, height: 8)

                    Text(profile.displayName)
                        .font(.system(size: 12))

                    Spacer()

                    Button("Rename") {
                        profileToRename = profile
                        showRenameSheet = true
                    }
                    .buttonStyle(.plain)
                    .foregroundColor(.accentColor)
                    .font(.system(size: 11))

                    Button("Remove") {
                        profileToRemove = profile
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.red)
                    .font(.system(size: 11))
                }
                .padding(.vertical, 2)
            }
        }
    }

    @ViewBuilder
    private var behaviorSection: some View {
        Toggle(isOn: $appState.launchAtLogin) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Launch at login")
                    .font(.system(size: 12))
                Text("Start Pagerunner when you log in")
                    .font(.system(size: 10))
                    .foregroundColor(.secondary)
            }
        }
        .toggleStyle(.switch)
        .controlSize(.small)

        Divider()

        VStack(alignment: .leading, spacing: 4) {
            Text("Binary path")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(.secondary)
            Text(appState.binaryPath ?? "Not found")
                .font(.system(size: 11, design: .monospaced))
                .foregroundColor(appState.binaryPath != nil ? .primary : .red)
                .lineLimit(2)
                .truncationMode(.middle)
        }

        VStack(alignment: .leading, spacing: 4) {
            Text("Daemon socket")
                .font(.system(size: 11, weight: .medium))
                .foregroundColor(.secondary)
            Text("~/.pagerunner/daemon.sock")
                .font(.system(size: 11, design: .monospaced))
        }

        Divider()

        HStack {
            Text("Version")
                .font(.system(size: 11))
                .foregroundColor(.secondary)
            Spacer()
            Text("0.3.0")
                .font(.system(size: 11, design: .monospaced))
                .foregroundColor(.secondary)
        }
    }

    @ViewBuilder
    private var notificationsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("NOTIFICATIONS")
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(.secondary)
                .kerning(0.5)

            // Global: daemon health
            HStack {
                Toggle(isOn: Binding(
                    get: { NotificationSettings.notifyOnDaemonHealth() },
                    set: { NotificationSettings.setNotifyOnDaemonHealth($0) }
                )) {
                    Text("Daemon health alerts")
                        .font(.system(size: 12))
                }
                .toggleStyle(.switch)
                .controlSize(.small)
            }

            // Global: explicit notify tool (always on — informational only)
            HStack {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                Text("Agent-sent notifications always deliver")
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
            }

            if !appState.profiles.isEmpty {
                Divider()
                Text("PER PROFILE")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .kerning(0.5)

                ForEach(appState.profiles, id: \.id) { profile in
                    NotificationProfileRow(profile: profile)
                }
            }
        }
    }
}

private struct NotificationProfileRow: View {
    let profile: Profile
    @State private var crash: Bool = true
    @State private var idle: Bool = true
    @State private var start: Bool = false
    @State private var idleMinutes: Int = 30

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(profile.displayName)
                .font(.system(size: 11, weight: .medium))
            HStack(spacing: 12) {
                Toggle("Crash", isOn: $crash)
                    .onChange(of: crash) { NotificationSettings.setNotifyOnCrash(crash, profile: profile.name) }
                Toggle("Idle", isOn: $idle)
                    .onChange(of: idle) { NotificationSettings.setNotifyOnIdle(idle, profile: profile.name) }
                if idle {
                    Picker("", selection: $idleMinutes) {
                        Text("15m").tag(15)
                        Text("30m").tag(30)
                        Text("60m").tag(60)
                    }
                    .pickerStyle(.segmented)
                    .frame(width: 110)
                    .onChange(of: idleMinutes) {
                        NotificationSettings.setIdleThresholdMinutes(idleMinutes, profile: profile.name)
                    }
                }
                Toggle("Start", isOn: $start)
                    .onChange(of: start) { NotificationSettings.setNotifyOnStart(start, profile: profile.name) }
            }
            .font(.system(size: 11))
            .toggleStyle(.checkbox)
            .controlSize(.small)
        }
        .padding(.vertical, 2)
        .onAppear {
            crash = NotificationSettings.notifyOnCrash(profile: profile.name)
            idle = NotificationSettings.notifyOnIdle(profile: profile.name)
            start = NotificationSettings.notifyOnStart(profile: profile.name)
            idleMinutes = NotificationSettings.idleThresholdMinutes(profile: profile.name)
        }
    }
}
