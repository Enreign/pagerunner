import SwiftUI
import PagerunnerKit

struct SettingsView: View {
    @Environment(AppState.self) private var appState

    @State private var host = ""
    @State private var port = ""
    @State private var token = ""
    @State private var useTLS = false
    @State private var isTesting = false
    @State private var result: TestResult?
    @State private var revealToken = false

    enum TestResult: Equatable {
        case success(String)
        case failure(String)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.section) {
                connectionHeader
                connectionForm
                actions
                statusStrip
                tailscaleHint
                about
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.vertical, Theme.Spacing.regular)
        }
        .background(Color.operatorBackground)
        .navigationTitle("Settings")
        .navigationBarTitleDisplayMode(.large)
        .onAppear { loadFromConnection() }
        .animation(.snappy, value: result)
        .animation(.snappy, value: appState.connection.isConnected)
    }

    // MARK: Header

    private var connectionHeader: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.tight) {
            HStack(spacing: 10) {
                StatusDot(state: appState.connection.isConnected ? .live : .muted)
                Text(appState.connection.isConnected ? "Connected" : "Not connected")
                    .font(.headline)
                Spacer()
            }
            Text("Point the app at your Pagerunner daemon's HTTP API.")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: Form

    private var connectionForm: some View {
        Card(padding: 0) {
            VStack(spacing: 0) {
                field("Host", icon: "server.rack") {
                    TextField("100.64.0.1", text: $host)
                        .textContentType(.URL)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                        .font(.mono)
                }
                Divider().padding(.leading, 52)
                field("Port", icon: "number") {
                    TextField("19876", text: $port)
                        .keyboardType(.numberPad)
                        .font(.mono)
                }
                Divider().padding(.leading, 52)
                field("Token", icon: "key.fill") {
                    Group {
                        if revealToken {
                            TextField("bearer…", text: $token)
                        } else {
                            SecureField("bearer…", text: $token)
                        }
                    }
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .font(.mono)
                    Button {
                        revealToken.toggle()
                    } label: {
                        Image(systemName: revealToken ? "eye.slash" : "eye")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(revealToken ? "Hide token" : "Show token")
                }
                Divider().padding(.leading, 52)
                HStack(spacing: 12) {
                    Image(systemName: "lock.fill")
                        .foregroundStyle(.secondary)
                        .frame(width: 28)
                    Toggle("Use TLS", isOn: $useTLS)
                }
                .padding(.horizontal, Theme.Spacing.loose)
                .padding(.vertical, 14)
            }
        }
    }

    private func field<Content: View>(_ label: String, icon: String, @ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .foregroundStyle(.secondary)
                .frame(width: 28)
            Text(label)
                .font(.footnote.weight(.medium))
                .foregroundStyle(.secondary)
                .frame(width: 48, alignment: .leading)
            content()
        }
        .padding(.horizontal, Theme.Spacing.loose)
        .padding(.vertical, 14)
    }

    // MARK: Actions

    private var actions: some View {
        VStack(spacing: Theme.Spacing.regular) {
            Button(action: test) {
                HStack {
                    if isTesting {
                        ProgressView().controlSize(.small)
                    } else {
                        Image(systemName: "antenna.radiowaves.left.and.right")
                    }
                    Text(isTesting ? "Testing…" : "Test Connection")
                        .font(.body.weight(.semibold))
                    Spacer()
                }
                .frame(maxWidth: .infinity, minHeight: 50)
                .padding(.horizontal, Theme.Spacing.loose)
                .background(.operatorCard, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            }
            .buttonStyle(.plain)
            .disabled(host.isEmpty || port.isEmpty || isTesting)

            if let result {
                resultBanner(result)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }

            if appState.connection.isConnected {
                Button(role: .destructive, action: disconnect) {
                    HStack {
                        Image(systemName: "wifi.slash")
                        Text("Disconnect")
                            .font(.body.weight(.semibold))
                        Spacer()
                    }
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .padding(.horizontal, Theme.Spacing.loose)
                    .background(Color.red.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                    .foregroundStyle(.red)
                }
                .buttonStyle(.plain)
            } else {
                Button(action: connect) {
                    HStack {
                        Image(systemName: "bolt.fill")
                        Text("Connect")
                            .font(.body.weight(.semibold))
                        Spacer()
                    }
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .padding(.horizontal, Theme.Spacing.loose)
                    .background(.accent, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
                    .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
                .disabled(host.isEmpty || port.isEmpty)
                .opacity(host.isEmpty || port.isEmpty ? 0.5 : 1)
            }
        }
    }

    @ViewBuilder
    private func resultBanner(_ result: TestResult) -> some View {
        HStack(spacing: 10) {
            switch result {
            case .success(let msg):
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.accent)
                Text(msg).foregroundStyle(.primary)
            case .failure(let msg):
                Image(systemName: "xmark.circle.fill").foregroundStyle(.red)
                Text(msg).foregroundStyle(.primary)
            }
            Spacer()
        }
        .font(.subheadline)
        .padding(.horizontal, Theme.Spacing.loose)
        .padding(.vertical, Theme.Spacing.regular)
        .background(
            (result.isSuccess ? Color.accent : .red).opacity(0.12),
            in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous)
        )
    }

    // MARK: Status strip

    @ViewBuilder
    private var statusStrip: some View {
        if appState.connection.isConnected {
            Card(padding: Theme.Spacing.regular) {
                HStack(spacing: Theme.Spacing.regular) {
                    strip("DAEMON", value: "\(appState.connection.host):\(appState.connection.port)", mono: true)
                    Divider()
                    strip("POLLING", value: appState.isPolling ? "Active" : "Inactive")
                }
            }
        }
    }

    private func strip(_ label: String, value: String, mono: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.statLabel)
                .tracking(1.2)
                .foregroundStyle(.secondary)
            Text(value)
                .font(mono ? .monoFootnote : .footnote)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: Tailscale hint

    private var tailscaleHint: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "lock.shield")
                .foregroundStyle(.accent)
            VStack(alignment: .leading, spacing: 4) {
                Text("Tip")
                    .font(.footnote.weight(.semibold))
                Text("Bind the daemon to your Tailscale IP (100.x.x.x) so your phone can reach it over the tunnel without public exposure.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(Theme.Spacing.regular)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
    }

    // MARK: About

    private var about: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.regular) {
            SectionLabel(text: "ABOUT")
            Card(padding: Theme.Spacing.regular) {
                VStack(spacing: 0) {
                    aboutRow("Version", Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0.0")
                    Divider()
                    aboutRow("Build",   Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1")
                }
            }
        }
    }

    private func aboutRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(.subheadline)
            Spacer()
            Text(value)
                .font(.monoFootnote)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, Theme.Spacing.tight)
        .padding(.vertical, 10)
    }

    // MARK: Actions

    private func loadFromConnection() {
        host = appState.connection.host
        port = "\(appState.connection.port)"
        token = appState.connection.token
        useTLS = appState.connection.useTLS
    }

    private func applySettings() {
        appState.connection.host = host
        appState.connection.port = Int(port) ?? 9222
        appState.connection.token = token
        appState.connection.useTLS = useTLS
        appState.connection.saveSettings()
    }

    private func test() {
        applySettings()
        isTesting = true
        result = nil
        Task {
            let ok = await appState.connection.testConnection()
            result = ok
                ? .success("Reachable")
                : .failure(appState.connection.lastError ?? "Connection failed")
            isTesting = false
        }
    }

    private func connect() {
        applySettings()
        Task {
            await appState.connection.connect()
            if appState.connection.isConnected {
                appState.startPolling()
                result = .success("Connected")
            } else {
                result = .failure(appState.connection.lastError ?? "Connection failed")
            }
        }
    }

    private func disconnect() {
        appState.stopPolling()
        appState.connection.disconnect()
        result = nil
    }
}

private extension SettingsView.TestResult {
    var isSuccess: Bool {
        if case .success = self { return true }
        return false
    }
}

#Preview {
    NavigationStack {
        SettingsView()
    }
    .environment(AppState())
}
