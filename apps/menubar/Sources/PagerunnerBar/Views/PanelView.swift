import SwiftUI
import PagerunnerCore

/// Top-level panel content. Contains the navigation strip at the top and
/// either OverviewView or ProfileDetailView as the body.
struct PanelView: View {
    @Bindable var appState: AppState
    let pollingService: PollingService
    let controller: StatusItemController

    var body: some View {
        ZStack(alignment: .bottom) {
            // Background vibrancy — matches spec (.sidebar material)
            VisualEffectBackground()
                .ignoresSafeArea()

            VStack(spacing: 0) {
                // Daemon status banner
                DaemonBanner(appState: appState)
                    .padding(.horizontal, 12)
                    .padding(.top, 10)

                Divider().padding(.vertical, 6)

                // Navigation strip: Overview + compact profile icon tabs
                NavigationStrip(appState: appState)
                    .padding(.horizontal, 12)

                Divider().padding(.top, 6)

                // Main content area
                ScrollView {
                    switch appState.navigation {
                    case .overview:
                        OverviewView(appState: appState)
                            .padding(12)
                    case .profile(let name):
                        ProfileDetailView(appState: appState, profileName: name, controller: controller)
                            .padding(12)
                    }
                }
                .frame(maxHeight: 440)
            }
        }
        .frame(width: 310)
    }
}

// MARK: - Daemon status banner

struct DaemonBanner: View {
    let appState: AppState

    var body: some View {
        HStack {
            Circle()
                .fill(dotColor)
                .frame(width: 8, height: 8)
            Text(bannerText)
                .font(.system(size: 11, weight: .medium))
            Spacer()
            if case .stopped = appState.daemonStatus {
                Button("Start") { /* TODO: start daemon */ }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.mini)
            } else if case .running = appState.daemonStatus {
                Text("\(appState.sessionCount) sessions · \(appState.tabCount) tabs")
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
                Button("Stop") { /* TODO: stop daemon */ }
                    .buttonStyle(.bordered)
                    .controlSize(.mini)
            }
        }
        .padding(8)
        .background(bannerBackground)
        .cornerRadius(8)
    }

    private var dotColor: Color {
        switch appState.daemonStatus {
        case .running: return .green
        case .stale:   return .yellow
        case .stopped: return .red
        }
    }
    private var bannerText: String {
        switch appState.daemonStatus {
        case .running:             return "Daemon running"
        case .stale(let at):       return "Last seen \(Int(-at.timeIntervalSinceNow))s ago"
        case .stopped:             return "Daemon stopped"
        }
    }
    private var bannerBackground: Color {
        switch appState.daemonStatus {
        case .running: return Color(red: 34/255, green: 197/255, blue: 94/255).opacity(0.10)
        case .stale:   return Color(red: 245/255, green: 158/255, blue: 11/255).opacity(0.08)
        case .stopped: return Color(red: 239/255, green: 68/255, blue: 68/255).opacity(0.08)
        }
    }
}

// MARK: - Navigation strip

struct NavigationStrip: View {
    @Bindable var appState: AppState

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                // Overview tab (always leftmost)
                Button {
                    appState.navigation = .overview
                } label: {
                    HStack(spacing: 4) {
                        Image(systemName: "square.grid.2x2")
                            .font(.system(size: 12))
                        Text("Overview").font(.system(size: 11))
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(
                        appState.navigation == .overview
                            ? Color.accentColor.opacity(0.15) : Color.clear
                    )
                    .cornerRadius(6)
                }
                .buttonStyle(.plain)
                .help("Overview")

                Divider().frame(height: 18)

                // Personal profiles (compact icons)
                ForEach(appState.personalProfiles) { profile in
                    profileIcon(profile, style: .colorful)
                }

                if !appState.personalProfiles.isEmpty && !appState.agentProfiles.isEmpty {
                    Divider().frame(height: 18)
                }

                // Agent profiles (grey)
                ForEach(appState.agentProfiles) { profile in
                    profileIcon(profile, style: .muted)
                }
            }
        }
    }

    private enum IconStyle { case colorful, muted }

    private func profileIcon(_ profile: Profile, style: IconStyle) -> some View {
        Button {
            appState.navigation = .profile(profile.name)
        } label: {
            ZStack {
                Circle()
                    .fill(style == .colorful ? profileColor(profile.name) : Color.gray.opacity(0.3))
                    .frame(width: 24, height: 24)
                Text(String(profile.displayName.prefix(1)).uppercased())
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(style == .colorful ? .white : .secondary)
            }
            .overlay(
                Circle().stroke(
                    appState.navigation == .profile(profile.name) ? Color.accentColor : Color.clear,
                    lineWidth: 2
                )
            )
        }
        .buttonStyle(.plain)
        .help(profile.displayName)
    }

    private func profileColor(_ name: String) -> Color {
        // Deterministic color from profile name hash
        let colors: [Color] = [.blue, .purple, .pink, .orange, .teal, .indigo, .cyan, .mint]
        let idx = abs(name.hashValue) % colors.count
        return colors[idx]
    }
}

// MARK: - NSVisualEffectView bridge

struct VisualEffectBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        view.state = .active
        return view
    }
    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {}
}
