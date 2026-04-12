import SwiftUI
import PagerunnerKit

/// Full-screen first-run experience. Replaces the tab bar until a
/// successful connection is established.
struct OnboardingView: View {
    @Environment(AppState.self) private var appState

    @State private var host = ""
    @State private var port = "19876"
    @State private var token = ""
    @State private var useTLS = false
    @State private var isWorking = false
    @State private var error: String?
    @State private var revealToken = false
    @State private var probeStatus: ProbeStatus = .idle
    @State private var probeTask: Task<Void, Never>?

    enum ProbeStatus: Equatable {
        case idle
        case probing
        case reachable(AuthMode)
        case unreachable(String)

        var mode: AuthMode? {
            if case .reachable(let m) = self { return m }
            return nil
        }
    }
    @FocusState private var focusedField: Field?

    private enum Field { case host, port, token }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Theme.Spacing.section) {
                hero
                form
                probeBanner
                connectButton
                if let error {
                    errorBanner(error)
                        .transition(.move(edge: .top).combined(with: .opacity))
                }
                tailscaleHint
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.top, Theme.Spacing.section)
            .padding(.bottom, Theme.Spacing.section)
        }
        .background(Color.operatorBackground.ignoresSafeArea())
        .scrollDismissesKeyboard(.interactively)
        .onAppear { loadFromConnection() }
        .onChange(of: host) { scheduleProbe() }
        .onChange(of: port) { scheduleProbe() }
        .animation(.snappy, value: error)
        .animation(.snappy, value: isWorking)
        .animation(.snappy, value: probeStatus)
    }

    // Debounced probe of the daemon's /auth-info so we can hide the token
    // field when the server uses Tailscale auth.
    private func scheduleProbe() {
        probeTask?.cancel()
        guard !host.isEmpty, Int(port) != nil else {
            probeStatus = .idle
            return
        }
        probeStatus = .probing
        probeTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(450))
            if Task.isCancelled { return }
            appState.connection.host = host
            appState.connection.port = Int(port) ?? 19876
            appState.connection.useTLS = useTLS
            if let mode = await appState.connection.probeAuthMode() {
                probeStatus = .reachable(mode)
            } else {
                probeStatus = .unreachable(appState.connection.lastError ?? "Unreachable")
            }
        }
    }


    // MARK: Hero

    private var hero: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.regular) {
            ZStack {
                RoundedRectangle(cornerRadius: 20, style: .continuous)
                    .fill(.operatorCard)
                    .frame(width: 64, height: 64)
                Image(systemName: "figure.run")
                    .font(.system(size: 30, weight: .semibold))
                    .foregroundStyle(.accent)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("Connect")
                    .font(.largeTitle.bold())
                    .foregroundStyle(.primary)
                Text("Enter the address of your Pagerunner daemon. If it's on your tailnet, you're in — no token required.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
    }

    // MARK: Form

    private var form: some View {
        Card(padding: 0) {
            VStack(spacing: 0) {
                row(icon: "server.rack", label: "Host") {
                    TextField("100.64.0.1", text: $host)
                        .textContentType(.URL)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                        .submitLabel(.next)
                        .focused($focusedField, equals: .host)
                        .onSubmit { focusedField = .port }
                        .font(.mono)
                }
                separator
                row(icon: "number", label: "Port") {
                    TextField("19876", text: $port)
                        .keyboardType(.numberPad)
                        .submitLabel(probeStatus.mode == .token ? .next : .go)
                        .focused($focusedField, equals: .port)
                        .onSubmit {
                            if probeStatus.mode == .token {
                                focusedField = .token
                            } else if canConnect {
                                connect()
                            }
                        }
                        .font(.mono)
                }
                if probeStatus.mode == .token {
                    separator
                    row(icon: "key.fill", label: "Token") {
                        Group {
                            if revealToken {
                                TextField("bearer…", text: $token)
                            } else {
                                SecureField("bearer…", text: $token)
                            }
                        }
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .focused($focusedField, equals: .token)
                        .submitLabel(.go)
                        .onSubmit(connect)
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
                    .transition(.move(edge: .top).combined(with: .opacity))
                }
            }
        }
    }

    private var separator: some View {
        Divider().padding(.leading, 52)
    }

    private func row<Content: View>(icon: String, label: String, @ViewBuilder content: () -> Content) -> some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .foregroundStyle(.secondary)
                .frame(width: 28)
            Text(label)
                .font(.footnote.weight(.medium))
                .foregroundStyle(.secondary)
                .frame(width: 52, alignment: .leading)
            content()
        }
        .padding(.horizontal, Theme.Spacing.loose)
        .padding(.vertical, 14)
    }

    // MARK: Connect button

    @ViewBuilder
    private var probeBanner: some View {
        switch probeStatus {
        case .idle:
            EmptyView()
        case .probing:
            HStack(spacing: 10) {
                ProgressView().controlSize(.small)
                Text("Checking daemon…")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.regular)
        case .reachable(let mode):
            HStack(spacing: 10) {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(.accent)
                Text(mode == .tailscale
                     ? "Daemon reachable · Tailscale auth (no token)"
                     : "Daemon reachable · Token required")
                    .font(.footnote)
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.vertical, Theme.Spacing.regular)
            .background(Color.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
        case .unreachable(let msg):
            HStack(spacing: 10) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text("Can't reach daemon: \(msg)")
                    .font(.footnote)
                    .lineLimit(2)
                Spacer()
            }
            .padding(.horizontal, Theme.Spacing.loose)
            .padding(.vertical, Theme.Spacing.regular)
            .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
        }
    }

    private var connectButton: some View {
        Button(action: connect) {
            HStack(spacing: 10) {
                if isWorking {
                    ProgressView().controlSize(.small).tint(.white)
                } else {
                    Image(systemName: "bolt.fill")
                }
                Text(isWorking ? "Connecting…" : "Connect")
                    .font(.body.weight(.semibold))
            }
            .frame(maxWidth: .infinity, minHeight: 52)
            .background(.accent, in: RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
            .foregroundStyle(.white)
            .opacity(canConnect ? 1 : 0.5)
        }
        .buttonStyle(.plain)
        .disabled(!canConnect)
        .sensoryFeedback(.success, trigger: appState.connection.isConnected)
    }

    private func errorBanner(_ msg: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "xmark.octagon.fill")
                .foregroundStyle(.red)
            Text(msg)
                .foregroundStyle(.primary)
                .font(.footnote)
            Spacer()
        }
        .padding(.horizontal, Theme.Spacing.loose)
        .padding(.vertical, Theme.Spacing.regular)
        .background(Color.red.opacity(0.12), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
    }

    private var tailscaleHint: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "lock.shield")
                .foregroundStyle(.accent)
            VStack(alignment: .leading, spacing: 4) {
                Text("Tailscale tip")
                    .font(.footnote.weight(.semibold))
                Text("Bind the daemon to your Tailscale IP (100.x.x.x) to reach it over the tunnel from anywhere — no public exposure.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(Theme.Spacing.regular)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: Theme.Radius.chip, style: .continuous))
    }

    // MARK: Logic

    private var canConnect: Bool {
        guard !host.isEmpty, !port.isEmpty, !isWorking else { return false }
        switch probeStatus {
        case .reachable(.tailscale): return true
        case .reachable(.token):     return !token.isEmpty
        case .idle, .probing:        return false
        case .unreachable:           return false
        }
    }

    private func loadFromConnection() {
        host = appState.connection.host
        port = "\(appState.connection.port)"
        token = appState.connection.token
        useTLS = appState.connection.useTLS
    }

    private func connect() {
        guard canConnect else { return }
        focusedField = nil
        appState.connection.host = host
        appState.connection.port = Int(port) ?? 19876
        appState.connection.token = token
        appState.connection.useTLS = useTLS
        appState.connection.saveSettings()

        isWorking = true
        error = nil
        Task {
            await appState.connection.connect()
            if appState.connection.isConnected {
                appState.startPolling()
            } else {
                error = appState.connection.lastError ?? "Could not connect"
            }
            isWorking = false
        }
    }
}

#Preview {
    OnboardingView()
        .environment(AppState())
}
