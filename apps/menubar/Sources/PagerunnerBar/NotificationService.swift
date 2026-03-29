import UserNotifications
import Foundation
import OSLog
import PagerunnerCore

@MainActor
final class NotificationService: NSObject, UNUserNotificationCenterDelegate {
    private let center = UNUserNotificationCenter.current()

    // Set via configure() after init to avoid circular dependency
    weak var appState: AppState?
    weak var controller: StatusItemController?

    override init() {
        super.init()
        center.delegate = self
    }

    func configure(appState: AppState, controller: StatusItemController) {
        self.appState = appState
        self.controller = controller
    }

    func requestPermission() async {
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
        registerCategories()
    }

    // MARK: - Notification types

    func notifyExplicit(title: String, body: String?, level: String, profileName: String?, sessionId: String?) {
        let content = UNMutableNotificationContent()
        content.title = title
        if let body { content.body = body }
        content.sound = sound(for: level)
        content.categoryIdentifier = "NOTIFY_TOOL"
        content.userInfo = userInfo(profileName: profileName, sessionId: sessionId)
        schedule(content, id: "notify-\(UUID().uuidString)")
    }

    func notifySessionCrashed(profile: String, sessionId: String) {
        let content = UNMutableNotificationContent()
        content.title = "Session crashed — \(profile)"
        content.body = "Tap Restart to reopen."
        content.sound = .default
        content.categoryIdentifier = "SESSION_CRASHED"
        content.userInfo = userInfo(profileName: profile, sessionId: sessionId)
        schedule(content, id: "crash-\(sessionId)")
    }

    func notifyDaemonStopped() {
        let content = UNMutableNotificationContent()
        content.title = "Pagerunner stopped unexpectedly"
        content.body = "Tap Restart Daemon to recover."
        content.sound = .default
        content.categoryIdentifier = "DAEMON_STOPPED"
        schedule(content, id: "daemon-stopped-\(Date().timeIntervalSince1970)")
    }

    func notifyCheckpointSaved(name: String) {
        let content = UNMutableNotificationContent()
        content.title = "Checkpoint saved"
        content.body = "\"\(name)\" saved successfully."
        content.categoryIdentifier = "CHECKPOINT_SAVED"
        schedule(content, id: "ckpt-saved-\(UUID().uuidString)")
    }

    func notifyAgentIdle(profileName: String, idleMinutes: Int, sessionId: String) {
        let content = UNMutableNotificationContent()
        content.title = "Agent \(profileName) idle \(idleMinutes)min"
        content.body = "No tab activity detected."
        content.sound = .default
        content.categoryIdentifier = "AGENT_IDLE"
        content.userInfo = userInfo(profileName: profileName, sessionId: sessionId)
        schedule(content, id: "agent-idle-\(profileName)")
    }

    func notifySessionStarted(profileName: String) {
        let content = UNMutableNotificationContent()
        content.title = "\(profileName) session started"
        content.categoryIdentifier = "SESSION_STARTED"
        content.userInfo = userInfo(profileName: profileName, sessionId: nil)
        schedule(content, id: "session-start-\(profileName)-\(Date().timeIntervalSince1970)")
    }

    // MARK: - Categories

    private func registerCategories() {
        let view = UNNotificationAction(identifier: "VIEW", title: "View", options: .foreground)
        let restart = UNNotificationAction(identifier: "RESTART_SESSION", title: "Restart", options: .foreground)
        let restartDaemon = UNNotificationAction(identifier: "RESTART_DAEMON", title: "Restart Daemon", options: .foreground)
        let closeSession = UNNotificationAction(identifier: "CLOSE_SESSION", title: "Close Session", options: .destructive)
        let dismiss = UNNotificationAction(identifier: "DISMISS", title: "Dismiss", options: .destructive)

        center.setNotificationCategories([
            UNNotificationCategory(identifier: "NOTIFY_TOOL",     actions: [view],                intentIdentifiers: []),
            UNNotificationCategory(identifier: "SESSION_CRASHED", actions: [view, restart],       intentIdentifiers: []),
            UNNotificationCategory(identifier: "AGENT_IDLE",      actions: [view, closeSession],  intentIdentifiers: []),
            UNNotificationCategory(identifier: "DAEMON_STOPPED",  actions: [restartDaemon, dismiss], intentIdentifiers: []),
            UNNotificationCategory(identifier: "SESSION_STARTED", actions: [],                    intentIdentifiers: []),
            UNNotificationCategory(identifier: "CHECKPOINT_SAVED",actions: [],                    intentIdentifiers: []),
        ])
    }

    // MARK: - Delegate

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        let profileName = userInfo["notif.profileName"] as? String
        let sessionId = userInfo["notif.sessionId"] as? String
        let actionIdentifier = response.actionIdentifier

        // Call completionHandler immediately — async side-effects fire in a separate task.
        completionHandler()

        Task { @MainActor [weak self] in
            guard let self else { return }
            switch actionIdentifier {
            case "VIEW", UNNotificationDefaultActionIdentifier:
                self.controller?.openPopover()
                if let name = profileName {
                    self.appState?.navigation = .profile(name)
                } else {
                    self.appState?.navigation = .overview
                }
            case "RESTART_SESSION":
                if let name = profileName {
                    // Note: stealth/anonymize not preserved — intentional
                    _ = try? await DaemonClient().call(
                        tool: "open_session",
                        args: ["profile": name]
                    )
                }
            case "CLOSE_SESSION":
                if let sid = sessionId {
                    _ = try? await DaemonClient().call(
                        tool: "close_session",
                        args: ["session_id": sid]
                    )
                }
            case "RESTART_DAEMON":
                await self.appState?.restartDaemon()
            default:
                break
            }
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }

    // MARK: - Helpers

    private static let log = Logger(subsystem: "io.pagerunner.bar", category: "notifications")

    private nonisolated func schedule(_ content: UNMutableNotificationContent, id: String) {
        let request = UNNotificationRequest(identifier: id, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request) { error in
            if let error {
                Logger(subsystem: "io.pagerunner.bar", category: "notifications")
                    .error("Failed to schedule notification \(id): \(error)")
            }
        }
    }

    private func sound(for level: String) -> UNNotificationSound? {
        switch level {
        case "warning", "error": return .default
        default: return nil
        }
    }

    private func userInfo(profileName: String?, sessionId: String?) -> [AnyHashable: Any] {
        var info: [AnyHashable: Any] = [:]
        if let p = profileName { info["notif.profileName"] = p }
        if let s = sessionId { info["notif.sessionId"] = s }
        return info
    }
}
