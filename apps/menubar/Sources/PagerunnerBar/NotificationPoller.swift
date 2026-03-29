import Foundation
import PagerunnerCore

/// Polls the daemon every 2 seconds for pending notifications and fires them
/// via NotificationService. Runs independently of PollingService — stays at
/// 2s even when the popover is closed so notify() tool results arrive promptly.
@MainActor
final class NotificationPoller {
    private let daemon = DaemonClient()
    private let notificationService: NotificationService

    init(notificationService: NotificationService) {
        self.notificationService = notificationService
    }

    func start() {
        Task { @MainActor [weak self] in
            while !Task.isCancelled {
                await self?.poll()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    private func poll() async {
        guard let raw = try? await daemon.call(tool: "list_notifications") else { return }
        guard let notifs = raw["notifications"]?.arrayValue else { return }

        for item in notifs {
            guard let obj = item.objectValue,
                  let title = obj["title"]?.stringValue,
                  let level = obj["level"]?.stringValue else { continue }

            let body = obj["body"]?.stringValue
            let profileName = obj["profile_name"]?.stringValue
            let sessionId = obj["session_id"]?.stringValue

            notificationService.notifyExplicit(
                title: title,
                body: body,
                level: level,
                profileName: profileName,
                sessionId: sessionId
            )
        }
    }
}
