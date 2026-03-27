import AppKit
import SwiftUI
import PagerunnerCore

@main
struct PagerunnerBarApp {
    static func main() {
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.run()
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItemController: StatusItemController!
    private var appState = AppState()
    private var pollingService: PollingService!
    private var notificationService: NotificationService!

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Keepalive window: 1×1 NSWindow, orderOut immediately.
        // Without this, the app's run loop exits when the popover closes (no Dock icon).
        let keepalive = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1, height: 1),
            styleMask: [],
            backing: .buffered,
            defer: false
        )
        keepalive.isReleasedWhenClosed = false
        keepalive.orderOut(nil)

        // Request notification permission and register categories
        notificationService = NotificationService()
        Task { @MainActor in
            await notificationService.requestPermission()
        }

        // Resolve binary path
        Task { @MainActor in
            appState.binaryPath = await resolveBinaryPath()
        }

        // Set up polling
        let daemon = DaemonClient()
        pollingService = PollingService { @MainActor [weak self] in
            await self?.poll(client: daemon)
        }

        statusItemController = StatusItemController(appState: appState, pollingService: pollingService)
        pollingService.start()
    }

    private func poll(client: DaemonClient) async {
        do {
            // 1. list_sessions
            let sessionsRaw = try await client.call(tool: "list_sessions")
            guard let data = sessionsRaw["data"]?.arrayValue else { return }
            let sessions: [Session] = data.compactMap { item -> Session? in
                guard let obj = item.objectValue,
                      let id = obj["id"]?.stringValue,
                      let profile = obj["profile"]?.stringValue,
                      let displayName = obj["display_name"]?.stringValue,
                      let statusStr = obj["status"]?.stringValue else { return nil }
                let stealth = obj["stealth"]?.boolValue ?? false
                // Re-encode as JSON and decode via Codable to respect CodingKeys mapping.
                let dict: [String: Any] = [
                    "id": id,
                    "profile": profile,
                    "display_name": displayName,
                    "stealth": stealth,
                    "status": statusStr
                ]
                guard let jsonData = try? JSONSerialization.data(withJSONObject: dict),
                      let session = try? JSONDecoder().decode(Session.self, from: jsonData) else { return nil }
                return session
            }
            appState.sessions = sessions
            appState.recordSuccess()

            // 2. list_tabs for each alive session (serial, best-effort)
            for session in sessions where session.status == SessionStatus.alive {
                if let tabsRaw = try? await client.call(tool: "list_tabs", args: ["session_id": session.id]) {
                    if let tabData = tabsRaw["data"]?.arrayValue {
                        appState.tabs[session.id] = tabData.compactMap { t -> PagerunnerCore.Tab? in
                            guard let obj = t.objectValue,
                                  let targetId = obj["target_id"]?.stringValue,
                                  let url = obj["url"]?.stringValue,
                                  let title = obj["title"]?.stringValue else { return nil }
                            let dict: [String: Any] = ["target_id": targetId, "url": url, "title": title]
                            guard let jsonData = try? JSONSerialization.data(withJSONObject: dict),
                                  let tab = try? JSONDecoder().decode(PagerunnerCore.Tab.self, from: jsonData) else { return nil }
                            return tab
                        }
                    }
                } else {
                    appState.tabs[session.id] = []
                }
            }
        } catch {
            appState.recordFailure()
        }
    }
}

/// Resolve the pagerunner binary path: ~/.local/bin/pagerunner, then `which pagerunner`.
func resolveBinaryPath() async -> String? {
    let localBin = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".local/bin/pagerunner").path
    if FileManager.default.isExecutableFile(atPath: localBin) {
        return localBin
    }
    let task = Process()
    task.launchPath = "/usr/bin/env"
    task.arguments = ["which", "pagerunner"]
    let pipe = Pipe()
    task.standardOutput = pipe
    task.standardError = Pipe()
    try? task.run()
    task.waitUntilExit()
    let data = pipe.fileHandleForReading.readDataToEndOfFile()
    let path = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return path.isEmpty ? nil : path
}
