import SwiftUI

struct SettingsView: View {
    @Bindable var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Back header — matches Add Profile style
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

            VStack(alignment: .leading, spacing: 16) {

                // Launch at login
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

                // Binary path
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

                // Daemon socket
                VStack(alignment: .leading, spacing: 4) {
                    Text("Daemon socket")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(.secondary)
                    Text("~/.pagerunner/daemon.sock")
                        .font(.system(size: 11, design: .monospaced))
                }

                Divider()

                // Version
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
            .padding(12)

            Spacer()
        }
    }
}
