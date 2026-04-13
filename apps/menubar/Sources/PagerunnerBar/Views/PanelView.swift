import SwiftUI
import PagerunnerCore

/// Top-level panel content. Contains the navigation strip at the top and
/// either OverviewView or ProfileDetailView as the body.
struct PanelView: View {
    @Bindable var appState: AppState
    let pollingService: PollingService
    let controller: StatusItemController
    @State private var daemonProcess: Process?

    var body: some View {
        ZStack {
            // Background vibrancy — matches spec (.sidebar material)
            VisualEffectBackground()
                .ignoresSafeArea()

            VStack(spacing: 0) {
                // Banner + nav strip: hidden when stopped (and not transitioning) or while starting/restarting
                let isTransitioning = appState.transition == .starting || appState.transition == .restarting
                if (appState.daemonStatus != .stopped || appState.transition != .none) && !isTransitioning {
                    DaemonBanner(appState: appState, onStart: {
                        appState.transition = .starting
                        guard let binary = appState.binaryPath else { return }
                        let proc = Process()
                        proc.launchPath = binary
                        proc.arguments = ["daemon"]
                        try? proc.run()
                        daemonProcess = proc
                    }, onStop: {
                        appState.transition = .stopping
                        if let proc = daemonProcess {
                            proc.terminate()
                            daemonProcess = nil
                        } else {
                            let kill = Process()
                            kill.launchPath = "/usr/bin/pkill"
                            kill.arguments = ["-f", "pagerunner daemon"]
                            try? kill.run()
                        }
                    })

                    NavigationStrip(appState: appState)
                }

                // Main content area
                if case .stopped = appState.daemonStatus, appState.transition == .none {
                    StoppedView(onStart: {
                        appState.transition = .starting
                        guard let binary = appState.binaryPath else { return }
                        let proc = Process()
                        proc.launchPath = binary
                        proc.arguments = ["daemon"]
                        try? proc.run()
                        daemonProcess = proc
                    })
                } else if appState.transition == .starting || appState.transition == .restarting {
                    StartingView(restarting: appState.transition == .restarting)
                } else {
                    mainContent
                }

                // Bottom bar (spec: .bottom — Settings + About + Quit)
                BottomBar(appState: appState)
            }
        }
        .frame(width: 352)
        .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122)) // #1d1d1f
        .colorScheme(.light)
    }

    @ViewBuilder
    private var mainContent: some View {
        switch appState.navigation {
        case .overview:
            ScrollView {
                OverviewView(appState: appState)
            }
        case .profile(let name):
            ScrollView {
                ProfileDetailView(appState: appState, profileName: name, controller: controller)
            }
        case .settings:
            ScrollView {
                SettingsView(appState: appState)
            }
        case .addProfile:
            ScrollView {
                AddProfileView(appState: appState)
            }
        case .agent:
            AgentView(appState: appState)
        }
    }
}

// MARK: - Daemon status banner

struct DaemonBanner: View {
    let appState: AppState
    let onStart: () -> Void
    let onStop: () -> Void

    private var isTransitioning: Bool { appState.transition != .none }

    var body: some View {
        HStack(spacing: 7) {
            if !isTransitioning {
                Circle()
                    .fill(dotColor)
                    .frame(width: 7, height: 7)
                    .shadow(color: dotColor.opacity(0.5), radius: 2)
            }

            Text(transitionText)
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(isTransitioning ? Color(red: 0.533, green: 0.533, blue: 0.533) : bannerTextColor)

            Spacer()

            if isTransitioning {
                // hide buttons during transition
            } else if case .running = appState.daemonStatus {
                Text("\(appState.sessionCount) windows · \(appState.tabCount) tabs")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533)) // #888
                Button("Stop") { onStop() }
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                    .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(bannerBackground)
        // Spec: border-bottom: .5px solid rgba(0,0,0,.1) — no corner radius
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.primary.opacity(0.1)).frame(height: 0.5)
        }
    }

    private var dotColor: Color {
        switch appState.daemonStatus {
        case .running: return Color(red: 0.133, green: 0.773, blue: 0.369)
        case .stale:   return Color(red: 0.961, green: 0.620, blue: 0.043)
        case .stopped: return Color(red: 0.937, green: 0.267, blue: 0.267)
        }
    }
    private var transitionText: String {
        switch appState.transition {
        case .starting:    return "Starting…"
        case .restarting:  return "Restarting…"
        case .stopping:    return "Stopping…"
        case .none:        return bannerText
        }
    }
    private var bannerText: String {
        switch appState.daemonStatus {
        case .running:       return "Pagerunner is live"
        case .stale:         return "Connection lost"
        case .stopped:       return "Pagerunner stopped"
        }
    }
    private var bannerTextColor: Color {
        switch appState.daemonStatus {
        case .running: return Color(red: 0.086, green: 0.396, blue: 0.204) // #166534
        case .stale:   return Color(red: 0.573, green: 0.251, blue: 0.055) // #92400e
        case .stopped: return Color(red: 0.863, green: 0.149, blue: 0.149) // #dc2626
        }
    }
    private var bannerBackground: Color {
        switch appState.daemonStatus {
        case .running: return Color(red: 34/255, green: 197/255, blue: 94/255).opacity(0.09)
        case .stale:   return Color(red: 245/255, green: 158/255, blue: 11/255).opacity(0.08)
        case .stopped: return Color(red: 239/255, green: 68/255, blue: 68/255).opacity(0.08)
        }
    }
}

// MARK: - Navigation strip

struct NavigationStrip: View {
    @Bindable var appState: AppState

    var body: some View {
        HStack(spacing: 0) {
            agentTab

            Rectangle().fill(Color.primary.opacity(0.1)).frame(width: 0.5)
                .padding(.vertical, 6)

            // Overview tab — compact secondary tab
            Button {
                appState.navigation = .overview
            } label: {
                VStack(spacing: 2) {
                    Image(systemName: "square.grid.2x2")
                        .font(.system(size: 13, weight: .medium))
                    Text("Overview")
                        .font(.system(size: 10, weight: appState.navigation == .overview ? .medium : .regular))
                }
                .foregroundStyle(appState.navigation == .overview
                                 ? Color(red: 0, green: 0.478, blue: 1)
                                 : Color(red: 0.4, green: 0.4, blue: 0.4))
                .frame(width: 58)
                .padding(.vertical, 7)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("Overview")

            // Profile icons — scrollable if too many to fit
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 0) {
                    ForEach(Array(appState.personalProfiles.enumerated()), id: \.element.id) { index, profile in
                        profileTab(profile, index: index, style: .colorful)
                    }

                    if !appState.personalProfiles.isEmpty && !appState.agentProfiles.isEmpty {
                        Rectangle()
                            .fill(Color.primary.opacity(0.15))
                            .frame(width: 0.5, height: 20)
                            .padding(.horizontal, 2)
                    }

                    ForEach(Array(appState.agentProfiles.enumerated()), id: \.element.id) { index, profile in
                        profileTab(profile, index: index, style: .muted)
                    }
                }
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .background(Color.primary.opacity(0.07))
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.primary.opacity(0.12)).frame(height: 0.5)
        }
    }

    private enum IconStyle { case colorful, muted }

    private var agentAccent: Color {
        switch appState.agentState {
        case .running:
            return Color(red: 0, green: 0.478, blue: 1)
        case .waitingApproval:
            return Color(red: 0.961, green: 0.620, blue: 0.043)
        case .completed:
            return Color(red: 0.133, green: 0.773, blue: 0.369)
        case .error:
            return Color(red: 0.937, green: 0.267, blue: 0.267)
        case .idle:
            return Color(red: 0.4, green: 0.4, blue: 0.4)
        }
    }

    private var agentTab: some View {
        Button {
            appState.navigation = .agent
        } label: {
            HStack(spacing: 8) {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: "waveform.circle.fill")
                        .font(.system(size: 18))
                        .foregroundStyle(appState.navigation == .agent ? Color(red: 0, green: 0.478, blue: 1) : agentAccent)
                    if appState.agentState != .idle || appState.voiceStatus != .idle {
                        Circle()
                            .fill(agentAccent)
                            .frame(width: 7, height: 7)
                            .overlay(Circle().stroke(Color.white, lineWidth: 1.5))
                            .offset(x: 4, y: -4)
                    }
                }

                VStack(alignment: .leading, spacing: 1) {
                    Text("Agent")
                        .font(.system(size: 12, weight: .semibold))
                    Text(agentTabSubtitle)
                        .font(.system(size: 10))
                }
                .foregroundStyle(appState.navigation == .agent
                                 ? Color(red: 0, green: 0.478, blue: 1)
                                 : Color(red: 0.4, green: 0.4, blue: 0.4))
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .frame(width: 112, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 9)
                    .fill(appState.navigation == .agent
                          ? Color(red: 0, green: 0.478, blue: 1).opacity(0.09)
                          : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("Pagerunner Agent")
    }

    private var agentTabSubtitle: String {
        if appState.voiceStatus == .listening { return "Listening" }
        if appState.voiceStatus == .speaking { return "Speaking" }
        switch appState.agentState {
        case .running: return "Working"
        case .waitingApproval: return "Approve"
        case .completed: return "Ready"
        case .error: return "Needs retry"
        case .idle: return "Ready"
        }
    }

    private func profileTab(_ profile: Profile, index: Int, style: IconStyle) -> some View {
        let sessions = appState.sessionsFor(profile: profile.name)
        let aliveSessions = sessions.filter { $0.status == .alive }
        let tabCount = aliveSessions.reduce(0) { $0 + appState.tabsFor(session: $1.id).count }
        let isActive = appState.navigation == .profile(profile.name)

        return Button {
            appState.navigation = .profile(profile.name)
        } label: {
            ZStack(alignment: .topTrailing) {
                ProfileIcon(profile: profile, index: index, size: 22)

                // Tab count badge — only when there are open tabs
                if tabCount > 0 {
                    Text("\(tabCount)")
                        .font(.system(size: 8, weight: .bold))
                        .foregroundColor(.white)
                        .frame(minWidth: 13, minHeight: 13)
                        .padding(.horizontal, tabCount > 9 ? 3 : 0)
                        .background(Color(red: 0, green: 0.478, blue: 1))
                        .clipShape(Capsule())
                        .overlay(Capsule().stroke(Color(red: 228/255, green: 228/255, blue: 228/255), lineWidth: 1.5))
                        .offset(x: 3, y: -2)
                }
            }
            .frame(maxHeight: .infinity)
            .padding(.horizontal, 8)
            .background(isActive ? Color(red: 0, green: 0.478, blue: 1).opacity(0.06) : Color.clear)
            .overlay(alignment: .bottom) {
                if isActive {
                    Rectangle()
                        .fill(Color(red: 0, green: 0.478, blue: 1))
                        .frame(height: 2)
                }
            }
        }
        .buttonStyle(.plain)
        .help(profile.displayName)
    }
}

// MARK: - Stopped state (centered illustration)

struct StoppedView: View {
    let onStart: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Spacer()

            // Sleeping moon icon
            Text("🌙")
                .font(.system(size: 48))

            VStack(spacing: 6) {
                Text("Pagerunner is sleeping")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))

                Text("Start it to manage your Chrome windows")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    .multilineTextAlignment(.center)
            }

            Button {
                onStart()
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "play.fill")
                        .font(.system(size: 10))
                    Text("Wake up")
                        .font(.system(size: 12, weight: .medium))
                }
                .foregroundColor(.white)
                .padding(.horizontal, 20)
                .padding(.vertical, 8)
                .background(Color(red: 0, green: 0.478, blue: 1))
                .cornerRadius(8)
            }
            .buttonStyle(.plain)

            Spacer()
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 24)
    }
}

// MARK: - Starting state (centered illustration)

struct StartingView: View {
    var restarting: Bool = false

    var body: some View {
        VStack(spacing: 16) {
            Spacer()

            ProgressView()
                .scaleEffect(1.5)
                .frame(width: 48, height: 48)

            VStack(spacing: 6) {
                Text(restarting ? "Pagerunner is restarting" : "Pagerunner is starting")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))

                Text(restarting ? "Applying changes…" : "Connecting to Chrome…")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    .multilineTextAlignment(.center)
            }

            Spacer()
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 24)
    }
}

// MARK: - Bottom bar (Settings + Quit)

/// Reusable menu row with rounded hover highlight.
struct MenuBarRow<Label: View>: View {
    @Binding var hovered: Bool
    let action: () -> Void
    @ViewBuilder let label: () -> Label

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                label()
                Spacer()
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 5)
            .background(
                RoundedRectangle(cornerRadius: 4)
                    .fill(hovered ? Color.black.opacity(0.04) : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
    }
}

struct BottomBar: View {
    @Bindable var appState: AppState
    @State private var settingsHovered = false
    @State private var quitHovered = false

    private var isOnSubpage: Bool {
        switch appState.navigation {
        case .overview: return false
        case .profile: return false
        case .agent: return false
        case .settings, .addProfile: return true
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            Rectangle().fill(Color.primary.opacity(0.1)).frame(height: 0.5)

            if isOnSubpage {
                // Subpages (Settings, Add Profile) show no bottom items
            } else {
                MenuBarRow(hovered: $settingsHovered) {
                    appState.navigation = .settings
                } label: {
                    Text("⚙").font(.system(size: 12)).frame(width: 16)
                        .foregroundColor(Color(white: 0.47))
                    Text("Settings…").font(.system(size: 13))
                }

                MenuBarRow(hovered: $quitHovered) {
                    NSApplication.shared.terminate(nil)
                } label: {
                    Text("⏻").font(.system(size: 12)).frame(width: 16)
                        .foregroundColor(Color(red: 0.86, green: 0.15, blue: 0.15))
                    Text("Quit").font(.system(size: 13))
                        .foregroundColor(Color(red: 0.86, green: 0.15, blue: 0.15))
                }
            }
        }
        .padding(.vertical, isOnSubpage ? 0 : 4)
    }
}

// MARK: - Shared gradient helper

func profileGradient(index: Int) -> AnyShapeStyle {
    let palettes: [[(red: Double, green: Double, blue: Double)]] = [
        [(0.259, 0.522, 0.957), (0.918, 0.263, 0.208), (0.204, 0.659, 0.325)], // Google
        [(0.114, 0.306, 0.847), (0.486, 0.227, 0.929), (0.035, 0.569, 0.698)], // Blue/purple/cyan
        [(0.851, 0.467, 0.024), (0.863, 0.149, 0.149), (0.086, 0.639, 0.290)], // Amber/red/green
        [(0.859, 0.153, 0.467), (0.576, 0.200, 0.918), (0.145, 0.388, 0.922)], // Pink/purple/blue
        [(0.035, 0.569, 0.698), (0.086, 0.639, 0.290), (0.792, 0.545, 0.016)], // Cyan/green/yellow
    ]
    let p = palettes[index % palettes.count]
    let c1 = Color(red: p[0].red, green: p[0].green, blue: p[0].blue)
    let c2 = Color(red: p[1].red, green: p[1].green, blue: p[1].blue)
    let c3 = Color(red: p[2].red, green: p[2].green, blue: p[2].blue)
    return AnyShapeStyle(AngularGradient(stops: [
        .init(color: c1, location: 0),
        .init(color: c1, location: 0.333),
        .init(color: c2, location: 0.333),
        .init(color: c2, location: 0.667),
        .init(color: c3, location: 0.667),
        .init(color: c3, location: 1.0),
    ], center: .center))
}

// MARK: - NSVisualEffectView bridge

struct VisualEffectBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        view.wantsLayer = true
        view.layer?.backgroundColor = NSColor(red: 228/255, green: 228/255, blue: 228/255, alpha: 1).cgColor
        return view
    }
    func updateNSView(_ nsView: NSView, context: Context) {}
}
