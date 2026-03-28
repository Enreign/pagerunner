import SwiftUI
import PagerunnerCore

// MARK: - Chrome Local State decoding

private struct ChromeLocalState: Codable {
    let profile: ChromeProfilesSection
}
private struct ChromeProfilesSection: Codable {
    let infoCache: [String: ChromeProfileEntry]
    enum CodingKeys: String, CodingKey { case infoCache = "info_cache" }
}
private struct ChromeProfileEntry: Codable {
    let name: String
    let userName: String
    enum CodingKeys: String, CodingKey { case name, userName = "user_name" }
}

private struct DiscoveredProfile: Identifiable {
    let dirName: String
    let fullPath: String
    let displayName: String
    let email: String
    let avatarImage: NSImage?
    var id: String { dirName }
    var gradientIndex: Int { abs(dirName.hashValue) % 5 }
}

// MARK: - Main view

@MainActor
struct AddProfileView: View {
    @Bindable var appState: AppState
    @State private var selectedTab = 0
    @State private var discoveredProfiles: [DiscoveredProfile] = []
    @State private var isLoading = true
    @State private var agentName = ""
    @State private var isAdding = false
    @State private var errorMessage: String? = nil
    @State private var instanceToAttach: DiscoveredInstance? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            headerBar

            HStack(spacing: 0) {
                tabButton("Discovered", tag: 0)
                tabButton("New Agent", tag: 1)
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 4)

            if selectedTab == 0 {
                discoveredTab
            } else {
                newAgentTab
            }
        }
        .task {
            await loadDiscovered()
            appState.triggerDiscovery()
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(5))
                appState.triggerDiscovery()
            }
        }
        .sheet(item: $instanceToAttach) { instance in
            AttachSheet(
                instance: instance,
                existingProfiles: appState.profiles,
                isPresented: Binding(
                    get: { instanceToAttach != nil },
                    set: { if !$0 { instanceToAttach = nil } }
                ),
                onAttachNew: { displayName in
                    appState.attachDiscovered(instance, displayName: displayName)
                },
                onMergeInto: { profile in
                    appState.mergeDiscovered(instance, intoProfile: profile.name)
                }
            )
            .preferredColorScheme(.light)
        }
    }

    // MARK: - Tab button

    private func tabButton(_ label: String, tag: Int) -> some View {
        let active = selectedTab == tag
        return Button { selectedTab = tag } label: {
            Text(label)
                .font(.system(size: 11, weight: active ? .semibold : .regular))
                .foregroundColor(active
                    ? Color(red: 0, green: 0.478, blue: 1)
                    : Color(red: 0.4, green: 0.4, blue: 0.4))
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(active
                    ? Color(red: 0, green: 0.478, blue: 1).opacity(0.09)
                    : Color.clear)
                .cornerRadius(5)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Header

    private var headerBar: some View {
        HStack(spacing: 6) {
            Button { appState.navigation = .overview } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
            }
            .buttonStyle(.plain)
            Text("Add Profile")
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

    // MARK: - Discovered tab

    private var discoveredTab: some View {
        VStack(spacing: 0) {
            // Chrome profiles from file system
            if isLoading {
                ProgressView()
                    .frame(maxWidth: .infinity)
                    .padding(.top, 32)
            } else if !discoveredProfiles.isEmpty {
                ForEach(discoveredProfiles) { profile in
                    DiscoveredProfileRow(profile: profile, isAdding: isAdding) {
                        Task { await addDiscovered(profile) }
                    }
                }
            }

            // Running Chrome instances (debug port discovery)
            let vmInstances = appState.discoveredInstances.filter { $0.isVM }
            let localInstances = appState.discoveredInstances.filter { !$0.isVM }

            if !vmInstances.isEmpty {
                sectionHeader("VM / Container Chrome")
                ForEach(vmInstances) { instance in
                    DebugPortInstanceRow(instance: instance) {
                        instanceToAttach = instance
                    }
                }
            }

            if !localInstances.isEmpty {
                sectionHeader("Running Chrome")
                ForEach(localInstances) { instance in
                    DebugPortInstanceRow(instance: instance) {
                        instanceToAttach = instance
                    }
                }
            }

            if !isLoading && discoveredProfiles.isEmpty && appState.discoveredInstances.isEmpty {
                Text("No new Chrome profiles found")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.top, 24)
            }

            if let error = errorMessage {
                Text(error)
                    .font(.system(size: 11))
                    .foregroundColor(.red)
                    .padding(.horizontal, 12)
                    .padding(.top, 4)
            }
        }
    }

    private func sectionHeader(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 9, weight: .semibold))
            .foregroundStyle(.secondary)
            .textCase(.uppercase)
            .tracking(0.5)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12)
            .padding(.top, 10)
            .padding(.bottom, 2)
    }

    // MARK: - New Agent tab

    private var newAgentTab: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Profile name")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundColor(.secondary)
                TextField("my-agent", text: $agentName)
                    .textFieldStyle(.plain)
                    .font(.system(size: 12, design: .monospaced))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background(Color.primary.opacity(0.06))
                    .cornerRadius(5)
                    .overlay(RoundedRectangle(cornerRadius: 5)
                        .stroke(Color.primary.opacity(0.15), lineWidth: 0.5))
                Text("A Chrome data dir will be created at ~/.chrome-<name>")
                    .font(.system(size: 10))
                    .foregroundColor(.secondary)
            }

            Button {
                Task { await addAgent() }
            } label: {
                HStack(spacing: 6) {
                    if isAdding {
                        ProgressView().scaleEffect(0.7).frame(width: 14, height: 14)
                    }
                    Text("Add Agent Profile")
                        .font(.system(size: 12, weight: .medium))
                }
                .foregroundColor(.white)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
                .background(agentNameValid
                    ? Color(red: 0, green: 0.478, blue: 1)
                    : Color.gray.opacity(0.35))
                .cornerRadius(8)
            }
            .buttonStyle(.plain)
            .disabled(!agentNameValid || isAdding)

            if let error = errorMessage {
                Text(error)
                    .font(.system(size: 11))
                    .foregroundColor(.red)
            }

            Spacer()
        }
        .padding(12)
    }

    private var agentNameValid: Bool { !normalizeAgentName(agentName).isEmpty }

    private func normalizeAgentName(_ raw: String) -> String {
        raw.lowercased()
           .replacingOccurrences(of: " ", with: "-")
           .filter { $0.isLetter || $0.isNumber || $0 == "-" }
    }

    // MARK: - Data loading

    private func loadDiscovered() async {
        isLoading = true
        let existing = Set(appState.profiles.compactMap { $0.userDataDir })
        let chromeBase = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Google/Chrome")
        let localStatePath = chromeBase.appendingPathComponent("Local State")

        guard let data = try? Data(contentsOf: localStatePath),
              let state = try? JSONDecoder().decode(ChromeLocalState.self, from: data) else {
            isLoading = false
            return
        }

        var found: [DiscoveredProfile] = []
        for (dirName, entry) in state.profile.infoCache {
            let fullPath = chromeBase.appendingPathComponent(dirName).path
            guard !existing.contains(fullPath), !entry.userName.isEmpty else { continue }
            let avatarPath = chromeBase.appendingPathComponent(dirName)
                .appendingPathComponent("Google Profile Picture.png").path
            found.append(DiscoveredProfile(
                dirName: dirName,
                fullPath: fullPath,
                displayName: entry.name,
                email: entry.userName,
                avatarImage: NSImage(contentsOfFile: avatarPath)
            ))
        }
        found.sort {
            if $0.dirName == "Default" { return true }
            if $1.dirName == "Default" { return false }
            let a = Int($0.dirName.replacingOccurrences(of: "Profile ", with: "")) ?? 0
            let b = Int($1.dirName.replacingOccurrences(of: "Profile ", with: "")) ?? 0
            return a < b
        }
        discoveredProfiles = found
        isLoading = false
    }

    // MARK: - Actions

    private func addDiscovered(_ profile: DiscoveredProfile) async {
        isAdding = true
        errorMessage = nil
        let displayName = "\(profile.displayName) (\(profile.email))"
        let base = profile.dirName.lowercased().replacingOccurrences(of: " ", with: "-")
        var name = base
        var suffix = 2
        let existingNames = Set(appState.profiles.map { $0.name })
        while existingNames.contains(name) { name = "\(base)-\(suffix)"; suffix += 1 }
        do {
            try appendProfileToConfig(name: name, displayName: displayName,
                                      userDataDir: profile.fullPath, kind: "personal")
            await appState.restartDaemon()
            appState.navigation = .overview
        } catch {
            errorMessage = "Failed to write config: \(error.localizedDescription)"
            isAdding = false
        }
    }

    private func addAgent() async {
        let name = normalizeAgentName(agentName)
        guard !name.isEmpty else { return }
        isAdding = true
        errorMessage = nil
        let dataDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".chrome-\(name)").path
        do {
            try FileManager.default.createDirectory(atPath: dataDir,
                withIntermediateDirectories: true)
            try appendProfileToConfig(name: name, displayName: name,
                                      userDataDir: dataDir, kind: "agent")
            await appState.restartDaemon()
            appState.navigation = .overview
        } catch {
            errorMessage = "Failed to create agent profile: \(error.localizedDescription)"
            isAdding = false
        }
    }

    // MARK: - Config writing

    private func appendProfileToConfig(name: String, displayName: String,
                                       userDataDir: String, kind: String) throws {
        let configURL = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".pagerunner/config.toml")
        if !FileManager.default.fileExists(atPath: configURL.path) {
            try "".write(to: configURL, atomically: true, encoding: .utf8)
        }
        var existing = try String(contentsOf: configURL, encoding: .utf8)
        if !existing.isEmpty && !existing.hasSuffix("\n") { existing += "\n" }
        var block = "\n[[profiles]]\n"
        block += "name = \"\(name)\"\n"
        block += "display_name = \"\(displayName)\"\n"
        block += "user_data_dir = \"\(userDataDir)\"\n"
        if kind == "agent" { block += "kind = \"agent\"\n" }
        try (existing + block).write(to: configURL, atomically: true, encoding: .utf8)
    }

}

// MARK: - Discovered profile row

private struct DiscoveredProfileRow: View {
    let profile: DiscoveredProfile
    let isAdding: Bool
    let onAdd: () -> Void
    @State private var isHovered = false

    var body: some View {
        Button(action: onAdd) {
            HStack(spacing: 9) {
                avatarView
                VStack(alignment: .leading, spacing: 1) {
                    Text(profile.displayName)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))
                    Text(profile.email)
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                        .lineLimit(1)
                }
                Spacer()
                if isAdding {
                    ProgressView().scaleEffect(0.6).frame(width: 16, height: 16)
                } else {
                    Text("Add")
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                        .padding(.horizontal, 7)
                        .padding(.vertical, 2)
                        .background(Color(red: 0, green: 0.478, blue: 1).opacity(isHovered ? 0.12 : 0.07))
                        .cornerRadius(4)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(isHovered && !isAdding ? Color.black.opacity(0.04) : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(isAdding)
        .onHover { isHovered = $0 }
    }

    private var avatarView: some View {
        Group {
            if let img = profile.avatarImage {
                Image(nsImage: img)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: 32, height: 32)
                    .clipShape(Circle())
            } else {
                ZStack {
                    Circle()
                        .fill(profileGradient(index: profile.gradientIndex))
                        .frame(width: 32, height: 32)
                    Text(String(profile.displayName.prefix(1)).uppercased())
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(.white)
                }
            }
        }
    }
}

// MARK: - Debug port instance row

private struct DebugPortInstanceRow: View {
    let instance: DiscoveredInstance
    let onAttach: () -> Void
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: 9) {
            Text("⊙")
                .font(.system(size: 18))
                .foregroundStyle(.secondary)
                .frame(width: 32, alignment: .center)

            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 4) {
                    Text(":\(instance.port)")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))
                    if instance.isVM {
                        Text("VM")
                            .font(.system(size: 9, weight: .semibold))
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(Color.orange.opacity(0.15))
                            .foregroundStyle(.orange)
                            .clipShape(Capsule())
                    }
                }
                Text("\(instance.tabCount) tab\(instance.tabCount == 1 ? "" : "s")")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
            }

            Spacer()

            switch instance.attachState {
            case .idle:
                Text("Attach")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(Color(red: 0, green: 0.478, blue: 1).opacity(isHovered ? 0.12 : 0.07))
                    .cornerRadius(4)
            case .attaching:
                ProgressView().scaleEffect(0.6).frame(width: 16, height: 16)
            case .attached(let profileDisplayName):
                VStack(alignment: .trailing, spacing: 1) {
                    Text("Attached")
                        .font(.system(size: 11))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                    Text(profileDisplayName)
                        .font(.system(size: 10))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533).opacity(0.8))
                }
            case .failed(let msg):
                Text(msg)
                    .font(.system(size: 10))
                    .foregroundColor(.red)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(isHovered ? Color.black.opacity(0.04) : Color.clear)
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .onTapGesture {
            if case .idle = instance.attachState { onAttach() }
        }
    }
}
