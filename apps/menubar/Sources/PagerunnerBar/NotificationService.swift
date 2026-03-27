import UserNotifications
import Foundation
import PagerunnerCore

/// Manages system notifications for session crashes, daemon stops, and checkpoint saves.
@MainActor
final class NotificationService: NSObject, UNUserNotificationCenterDelegate {
    private let center = UNUserNotificationCenter.current()

    override init() {
        super.init()
        center.delegate = self
    }

    func requestPermission() async {
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound])
        registerCategories()
    }

    // MARK: - Notification types

    func notifySessionCrashed(profile: String, sessionId: String) {
        let content = UNMutableNotificationContent()
        content.title = "Session crashed — \(profile)"
        content.body = "Session \(sessionId.prefix(8)) stopped unexpectedly."
        content.sound = .default
        content.categoryIdentifier = "SESSION_CRASHED"
        schedule(content, id: "crash-\(sessionId)")
    }

    func notifyDaemonStopped() {
        let content = UNMutableNotificationContent()
        content.title = "Pagerunner daemon stopped"
        content.body = "The background daemon stopped unexpectedly."
        content.sound = .default
        content.categoryIdentifier = "DAEMON_STOPPED"
        schedule(content, id: "daemon-stopped")
    }

    func notifyCheckpointSaved(name: String) {
        let content = UNMutableNotificationContent()
        content.title = "Checkpoint saved"
        content.body = "\"\(name)\" saved successfully."
        content.categoryIdentifier = "CHECKPOINT_SAVED"
        schedule(content, id: "ckpt-saved-\(UUID().uuidString)")
    }

    func notifyAgentIdle(profileName: String) {
        let content = UNMutableNotificationContent()
        content.title = "Agent \(profileName) idle for 30 min"
        content.body = "No tab activity detected. Close session to free resources."
        content.categoryIdentifier = "AGENT_IDLE"
        schedule(content, id: "agent-idle-\(profileName)")
    }

    // MARK: - Categories (actionable buttons)

    private func registerCategories() {
        let restart = UNNotificationAction(identifier: "RESTART_SESSION", title: "Restart", options: .foreground)
        let dismiss = UNNotificationAction(identifier: "DISMISS", title: "Dismiss", options: .destructive)

        let restartDaemon = UNNotificationAction(identifier: "RESTART_DAEMON", title: "Restart daemon", options: .foreground)

        let closeSession = UNNotificationAction(identifier: "CLOSE_SESSION", title: "Close session", options: .destructive)
        let keepSession = UNNotificationAction(identifier: "KEEP_SESSION", title: "Keep", options: [])

        center.setNotificationCategories([
            UNNotificationCategory(identifier: "SESSION_CRASHED", actions: [restart, dismiss], intentIdentifiers: []),
            UNNotificationCategory(identifier: "DAEMON_STOPPED", actions: [restartDaemon, dismiss], intentIdentifiers: []),
            UNNotificationCategory(identifier: "AGENT_IDLE", actions: [closeSession, keepSession], intentIdentifiers: []),
            UNNotificationCategory(identifier: "CHECKPOINT_SAVED", actions: [], intentIdentifiers: []),
        ])
    }

    // MARK: - UNUserNotificationCenterDelegate

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        // Action handling will be wired to AppState in a follow-up
        completionHandler()
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }

    // MARK: - Helpers

    private func schedule(_ content: UNMutableNotificationContent, id: String) {
        let request = UNNotificationRequest(identifier: id, content: content, trigger: nil)
        center.add(request)
    }
}
