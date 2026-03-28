import Foundation

// MARK: - Daemon status (failure gate output)

public enum DaemonStatus: Sendable, Equatable {
    case running
    case stale(lastSeenAt: Date)
    case stopped

    /// Convert a consecutive failure count to a daemon status.
    /// - Parameters:
    ///   - count: Number of consecutive poll failures.
    ///   - lastSeenAt: Last time a successful poll was received (used for .stale).
    public static func fromFailureCount(_ count: Int, lastSeenAt: Date?) -> DaemonStatus {
        switch count {
        case 0...2: return .running
        case 3...4: return .stale(lastSeenAt: lastSeenAt ?? Date())
        default:    return .stopped
        }
    }

    public static func == (lhs: DaemonStatus, rhs: DaemonStatus) -> Bool {
        switch (lhs, rhs) {
        case (.running, .running): return true
        case (.stopped, .stopped): return true
        case (.stale, .stale):     return true
        default:                   return false
        }
    }
}

// MARK: - PollingService

/// Drives the background poll loop. Calls `poll()` on the provided handler
/// and updates `AppState` via callbacks.
///
/// Intervals: panel visible = 2s, panel hidden = 10s.
/// On panel open: cancel current task, start 2s task with no initial sleep.
/// On panel close: cancel 2s task, start 10s task.
@MainActor
public final class PollingService {
    public typealias PollHandler = @MainActor () async -> Void

    private var currentTask: Task<Void, Never>?
    private var panelVisible = false
    private let pollHandler: PollHandler

    public init(pollHandler: @escaping PollHandler) {
        self.pollHandler = pollHandler
    }

    /// Call when the popover opens.
    public func panelDidOpen() {
        panelVisible = true
        startLoop(interval: 2, immediate: true)
    }

    /// Call when the popover closes.
    public func panelDidClose() {
        panelVisible = false
        startLoop(interval: 10, immediate: false)
    }

    /// Start the menu bar app polling. Call once on app launch.
    public func start() {
        startLoop(interval: 10, immediate: true)
    }

    /// Stop polling (e.g., on app quit).
    public func stop() {
        currentTask?.cancel()
        currentTask = nil
    }

    private func startLoop(interval: Int, immediate: Bool) {
        currentTask?.cancel()
        // Run on MainActor — pollHandler is @MainActor so we must NOT use Task.detached.
        // Using Task { @MainActor in ... } keeps us on the main actor while still being
        // async-cancellable (satisfies the Swift 6 strict concurrency requirement).
        currentTask = Task { @MainActor [weak self] in
            if !immediate {
                try? await Task.sleep(for: .seconds(interval))
            }
            while !Task.isCancelled {
                await self?.pollHandler()
                try? await Task.sleep(for: .seconds(interval))
            }
        }
    }
}
