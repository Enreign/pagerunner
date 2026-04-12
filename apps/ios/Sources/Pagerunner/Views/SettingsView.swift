import SwiftUI
import PagerunnerKit

struct SettingsView: View {
    @Environment(AppState.self) private var appState

    @State private var host = ""
    @State private var port = ""
    @State private var token = ""
    @State private var useTLS = false
    @State private var isTesting = false
    @State private var testResult: TestResult?

    enum TestResult {
        case success(String)
        case failure(String)
    }

    var body: some View {
        Form {
            connectionSection
            actionsSection
            statusSection
            aboutSection
            tailscaleHint
        }
        .navigationTitle("Settings")
        .onAppear {
            loadFromConnection()
        }
    }

    // MARK: - Connection

    private var connectionSection: some View {
        Section {
            HStack {
                Text("Host")
                    .frame(width: 60, alignment: .leading)
                TextField("127.0.0.1", text: $host)
                    .textContentType(.URL)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
            }

            HStack {
                Text("Port")
                    .frame(width: 60, alignment: .leading)
                TextField("9222", text: $port)
                    .keyboardType(.numberPad)
            }

            SecureField("Authentication Token", text: $token)

            Toggle("Use TLS", isOn: $useTLS)
        } header: {
            Text("Connection")
        } footer: {
            Text("Enter the host and port of your Pagerunner daemon.")
        }
    }

    // MARK: - Actions

    private var actionsSection: some View {
        Section {
            Button {
                testConnection()
            } label: {
                HStack {
                    Label("Test Connection", systemImage: "antenna.radiowaves.left.and.right")
                    Spacer()
                    if isTesting {
                        ProgressView()
                    }
                }
            }
            .disabled(host.isEmpty || port.isEmpty || isTesting)

            if appState.connection.isConnected {
                Button(role: .destructive) {
                    disconnect()
                } label: {
                    Label("Disconnect", systemImage: "wifi.slash")
                }
            } else {
                Button {
                    connect()
                } label: {
                    Label("Connect", systemImage: "wifi")
                }
                .disabled(host.isEmpty || port.isEmpty)
            }

            if let result = testResult {
                switch result {
                case .success(let message):
                    Label(message, systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                case .failure(let message):
                    Label(message, systemImage: "xmark.circle.fill")
                        .foregroundStyle(.red)
                }
            }
        }
    }

    // MARK: - Status

    private var statusSection: some View {
        Section("Status") {
            HStack {
                Text("Connection")
                Spacer()
                HStack(spacing: 6) {
                    Circle()
                        .fill(appState.connection.isConnected ? .green : .red)
                        .frame(width: 8, height: 8)
                    Text(appState.connection.isConnected ? "Connected" : "Disconnected")
                        .foregroundStyle(.secondary)
                }
            }

            if appState.connection.isConnected {
                HStack {
                    Text("Polling")
                    Spacer()
                    Text(appState.isPolling ? "Active" : "Inactive")
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: - About

    private var aboutSection: some View {
        Section("About") {
            HStack {
                Text("App Version")
                Spacer()
                Text(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0.0")
                    .foregroundStyle(.secondary)
            }

            HStack {
                Text("Build")
                Spacer()
                Text(Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1")
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: - Tailscale Hint

    private var tailscaleHint: some View {
        Section {
            Label {
                Text("Set the bind address to your Tailscale IP (100.x.x.x) for secure remote access without exposing your daemon to the public internet.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } icon: {
                Image(systemName: "lock.shield")
                    .foregroundStyle(.blue)
            }
        } header: {
            Text("Tailscale Tip")
        }
    }

    // MARK: - Actions

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

    private func testConnection() {
        applySettings()
        isTesting = true
        testResult = nil

        Task {
            let success = await appState.connection.testConnection()
            if success {
                testResult = .success("Connection successful")
            } else {
                testResult = .failure(appState.connection.lastError ?? "Connection failed")
            }
            isTesting = false
        }
    }

    private func connect() {
        applySettings()
        Task {
            await appState.connection.connect()
            if appState.connection.isConnected {
                appState.startPolling()
                testResult = .success("Connected")
            } else {
                testResult = .failure(appState.connection.lastError ?? "Connection failed")
            }
        }
    }

    private func disconnect() {
        appState.stopPolling()
        appState.connection.disconnect()
        testResult = nil
    }
}

#Preview {
    NavigationStack {
        SettingsView()
    }
    .environment(AppState())
}
