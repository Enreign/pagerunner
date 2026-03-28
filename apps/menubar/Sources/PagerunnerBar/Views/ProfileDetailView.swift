import SwiftUI
import PagerunnerCore

struct ProfileDetailView: View {
    @Bindable var appState: AppState
    let profileName: String
    let controller: StatusItemController
    @Environment(\.daemonClient) private var daemon

    private var profile: Profile? { appState.profiles.first { $0.name == profileName } }
    private var sessions: [Session] { appState.sessionsFor(profile: profileName) }

    /// Parse "growthmate.io (stas@growthmate.io)" → name, email
    private var parsedName: String {
        guard let p = profile else { return profileName }
        if let parenStart = p.displayName.firstIndex(of: "(") {
            return String(p.displayName[..<parenStart]).trimmingCharacters(in: .whitespaces)
        }
        return p.displayName
    }
    private var parsedEmail: String? {
        guard let p = profile,
              let parenStart = p.displayName.firstIndex(of: "("),
              let parenEnd = p.displayName.lastIndex(of: ")") else { return nil }
        let start = p.displayName.index(after: parenStart)
        return String(p.displayName[start..<parenEnd])
    }

    @State private var stealth = false
    @State private var showAttachUI = false
    @State private var discoveredChromes: [DiscoveredChrome] = []
    @State private var attachPort = "9222"

    /// Build a port → user_data_dir map from running Chrome processes via ps.
    nonisolated private func chromePortDirs() -> [Int: String] {
        let task = Process()
        task.launchPath = "/bin/ps"
        task.arguments = ["axo", "args"]
        let pipe = Pipe()
        task.standardOutput = pipe
        task.standardError = Pipe()
        try? task.run()
        task.waitUntilExit()
        let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        var result: [Int: String] = [:]
        for line in output.components(separatedBy: "\n") {
            guard line.contains("Google Chrome") || line.contains("Chromium"),
                  !line.contains("Helper") else { continue }
            guard let portMatch = line.range(of: "--remote-debugging-port=\\d+", options: .regularExpression),
                  let dirMatch  = line.range(of: "--user-data-dir=\\S+", options: .regularExpression)
            else { continue }
            let port = Int(line[portMatch].dropFirst("--remote-debugging-port=".count)) ?? 0
            let dir  = String(line[dirMatch].dropFirst("--user-data-dir=".count))
            if port > 0 { result[port] = dir }
        }
        return result
    }

    /// Probe ports 9222–9232 for a live Chrome debug endpoint that matches this profile.
    private func discoverPorts() {
        let profileDataDir = profile?.userDataDir
        Task {
            // Build port→dir map on background thread
            let portDirs = await withCheckedContinuation { cont in
                DispatchQueue.global(qos: .userInitiated).async {
                    cont.resume(returning: chromePortDirs())
                }
            }
            var found: [DiscoveredChrome] = []
            for port in 9222...9232 {
                // If we have ps data, only include ports whose user-data-dir matches this profile
                if let knownDir = portDirs[port], let profileDir = profileDataDir {
                    guard knownDir == profileDir else { continue }
                }
                guard let url = URL(string: "http://localhost:\(port)/json/list") else { continue }
                var req = URLRequest(url: url, timeoutInterval: 0.3)
                req.httpMethod = "GET"
                guard let (data, resp) = try? await URLSession.shared.data(for: req),
                      (resp as? HTTPURLResponse)?.statusCode == 200,
                      let tabs = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
                else { continue }
                let pages = tabs.filter { $0["type"] as? String == "page" }
                let title = pages.first?["title"] as? String
                found.append(DiscoveredChrome(port: port, tabCount: pages.count, activeTitle: title))
            }
            discoveredChromes = found
            if let first = found.first { attachPort = String(first.port) }
        }
    }

    private func log(_ msg: String) {
        let line = (msg + "\n").data(using: .utf8) ?? Data()
        let url = URL(fileURLWithPath: "/tmp/pr_attach.log")
        if let fh = try? FileHandle(forWritingTo: url) {
            fh.seekToEndOfFile()
            fh.write(line)
            try? fh.close()
        } else {
            try? line.write(to: url)
        }
    }

    private func attach(port: Int) {
        log("attach called: port=\(port) profile=\(profileName)")
        showAttachUI = false
        Task { @MainActor in
            do {
                let result = try await daemon.call(tool: "attach_session", args: [
                    "debug_port": port,
                    "profile": profileName
                ] as [String: Any])
                log("attach success: \(result)")
            } catch {
                log("attach error: \(error)")
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Profile identity bar — non-interactive
            HStack(spacing: 6) {
                Text(parsedName)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundColor(Color(red: 0, green: 0.35, blue: 0.75))
                if let email = parsedEmail {
                    Text(email)
                        .font(.system(size: 12))
                        .foregroundColor(Color(red: 0.35, green: 0.55, blue: 0.85))
                }
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

            VStack(alignment: .leading, spacing: 0) {
            // Session blocks
            ForEach(Array(sessions.enumerated()), id: \.element.id) { index, session in
                SessionBlockView(
                    session: session,
                    index: index,
                    tabs: appState.tabsFor(session: session.id),
                    appState: appState,
                    controller: controller
                )
            }

            // Open new window row
            HStack(spacing: 10) {
                Button {
                    let wasStealthy = stealth
                    stealth = false
                    Task { @MainActor in
                        var args: [String: Any] = ["profile": profileName]
                        if wasStealthy { args["stealth"] = true }
                        _ = try? await daemon.call(tool: "open_session", args: args)
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text("+")
                            .font(.system(size: 13, weight: .medium))
                        Text("Open new window")
                            .font(.system(size: 12))
                    }
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                }
                .buttonStyle(.plain)
                .disabled(appState.daemonStatus == .stopped)

                Spacer()

                Toggle(isOn: $stealth) {
                    Text("Stealth")
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                }
                .toggleStyle(.checkbox)
                .controlSize(.small)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)

            // Attach to running Chrome — expanded panel
            if showAttachUI {
                AttachPanel(
                    discoveredChromes: discoveredChromes,
                    attachPort: $attachPort,
                    onAttach: { port in attach(port: port) },
                    onDismiss: {
                        withAnimation(.easeInOut(duration: 0.12)) { showAttachUI = false }
                    }
                )
                .transition(.opacity.combined(with: .move(edge: .top)))
            }

            // "Attach to running Chrome" toggle link
            Button {
                withAnimation(.easeInOut(duration: 0.15)) { showAttachUI.toggle() }
                if showAttachUI { discoverPorts() }
            } label: {
                HStack(spacing: 5) {
                    Image(systemName: showAttachUI ? "chevron.down" : "chevron.right")
                        .font(.system(size: 8, weight: .medium))
                    Image(systemName: "dot.radiowaves.left.and.right")
                        .font(.system(size: 10))
                    Text("Attach to running Chrome")
                        .font(.system(size: 11))
                }
                .foregroundColor(showAttachUI ? Color(red: 0, green: 0.478, blue: 1) : .secondary)
            }
            .buttonStyle(.plain)
            .padding(.horizontal, 12)
            .padding(.top, 2)
            .padding(.bottom, 8)
            .disabled(appState.daemonStatus == .stopped)

            // Saved checkpoints (hidden if none)
            CheckpointListView(appState: appState, profileName: profileName)
        }
        }
    }
}

// MARK: - Attach panel

struct DiscoveredChrome: Identifiable {
    let port: Int
    let tabCount: Int
    let activeTitle: String?
    var id: Int { port }
}

private struct AttachPanel: View {
    let discoveredChromes: [DiscoveredChrome]
    @Binding var attachPort: String
    let onAttach: (Int) -> Void
    let onDismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if discoveredChromes.isEmpty {
                // No Chrome found — explain what to do
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Image(systemName: "info.circle")
                            .font(.system(size: 10))
                            .foregroundColor(.secondary)
                        Text("No Chrome found with debug port")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(red: 0.3, green: 0.3, blue: 0.3))
                    }
                    Text("Start Chrome with:\n--remote-debugging-port=9222")
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundColor(.secondary)
                        .padding(.leading, 16)
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 6)
            } else {
                // Discovered instances — tap row to attach
                VStack(alignment: .leading, spacing: 0) {
                    Text("Found")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundColor(.secondary)
                        .padding(.horizontal, 12)
                        .padding(.top, 8)
                        .padding(.bottom, 4)
                    ForEach(discoveredChromes) { chrome in
                        DiscoveredPortRow(chrome: chrome) { onAttach(chrome.port) }
                    }
                }
                .padding(.bottom, 4)

                Rectangle()
                    .fill(Color.primary.opacity(0.07))
                    .frame(height: 0.5)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 4)
            }

            // Manual port row
            HStack(spacing: 6) {
                Text("Port")
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                    .frame(width: 28, alignment: .leading)

                TextField("9222", text: $attachPort)
                    .textFieldStyle(.plain)
                    .font(.system(size: 11, design: .monospaced))
                    .frame(width: 46)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 3)
                    .background(Color.primary.opacity(0.06))
                    .cornerRadius(4)
                    .overlay(RoundedRectangle(cornerRadius: 4)
                        .stroke(Color.primary.opacity(0.15), lineWidth: 0.5))

                Spacer()

                Button("Attach") {
                    if let port = Int(attachPort) { onAttach(port) }
                }
                .font(.system(size: 10))
                .foregroundColor(Int(attachPort) != nil
                    ? Color(red: 0, green: 0.478, blue: 1) : .secondary)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(Int(attachPort) != nil
                    ? Color(red: 0, green: 0.478, blue: 1).opacity(0.08) : Color.clear)
                .cornerRadius(4)
                .buttonStyle(.plain)
                .disabled(Int(attachPort) == nil)

                Button(action: onDismiss) {
                    Text("✕")
                        .font(.system(size: 9))
                        .foregroundColor(.secondary)
                        .frame(width: 16, height: 16)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 8)
            .padding(.top, discoveredChromes.isEmpty ? 0 : 2)
        }
        .background(Color(red: 0, green: 0.478, blue: 1).opacity(0.03))
        .overlay(alignment: .top) {
            Rectangle().fill(Color.primary.opacity(0.07)).frame(height: 0.5)
        }
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.primary.opacity(0.07)).frame(height: 0.5)
        }
    }
}

private struct DiscoveredPortRow: View {
    let chrome: DiscoveredChrome
    let onAttach: () -> Void
    @State private var isHovered = false

    var body: some View {
        Button(action: onAttach) {
            HStack(spacing: 8) {
                Circle()
                    .fill(Color(red: 0.133, green: 0.773, blue: 0.369))
                    .frame(width: 6, height: 6)
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 5) {
                        Text("Chrome")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundColor(Color(red: 0.1, green: 0.1, blue: 0.1))
                        Text("·")
                            .font(.system(size: 11))
                            .foregroundColor(.secondary)
                        Text("port \(String(chrome.port))")
                            .font(.system(size: 10, design: .monospaced))
                            .foregroundColor(.secondary)
                        Text("·")
                            .font(.system(size: 11))
                            .foregroundColor(.secondary)
                        Text("\(chrome.tabCount) tab\(chrome.tabCount == 1 ? "" : "s")")
                            .font(.system(size: 10))
                            .foregroundColor(.secondary)
                    }
                    if let title = chrome.activeTitle, !title.isEmpty {
                        Text(title)
                            .font(.system(size: 10))
                            .foregroundColor(Color(red: 0.4, green: 0.4, blue: 0.4))
                            .lineLimit(1)
                            .truncationMode(.tail)
                    }
                }
                Spacer()
                Text("Attach")
                    .font(.system(size: 10))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(Color(red: 0, green: 0.478, blue: 1).opacity(isHovered ? 0.12 : 0.07))
                    .cornerRadius(4)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 5)
            .background(isHovered ? Color.primary.opacity(0.04) : Color.clear)
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
    }
}
