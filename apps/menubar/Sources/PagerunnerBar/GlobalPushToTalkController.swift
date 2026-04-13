import AppKit
import Observation

@MainActor
final class GlobalPushToTalkController {
    private let appState: AppState
    private var globalMonitor: Any?
    private var localMonitor: Any?
    private var isPressed = false
    private var suppressNextRelease = false

    init(appState: AppState) {
        self.appState = appState
        installMonitors()
        observeSettings()
    }

    private func installMonitors() {
        globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            Task { @MainActor [weak self] in
                self?.handle(event: event)
            }
        }

        localMonitor = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
            self?.handle(event: event)
            return event
        }
    }

    private func observeSettings() {
        withObservationTracking {
            _ = appState.globalPushToTalkEnabled
            _ = appState.globalHotkeyTrigger
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                self?.observeSettings()
                self?.syncStateWithSettings()
            }
        }
    }

    private func syncStateWithSettings() {
        guard !appState.globalPushToTalkEnabled else { return }
        isPressed = false
        suppressNextRelease = false
        appState.endGlobalPushToTalk()
    }

    private func handle(event: NSEvent) {
        guard appState.globalPushToTalkEnabled else { return }
        guard event.type == .flagsChanged else { return }
        guard event.keyCode == appState.globalHotkeyTrigger.keyCode else { return }

        let pressed = appState.globalHotkeyTrigger.isPressed(in: event)
        if pressed && !isPressed {
            isPressed = true
            if appState.globalHotkeyTrigger.supportsCommandToggle,
               event.modifierFlags.contains(.command)
            {
                suppressNextRelease = true
                appState.toggleContinuousDictation()
                return
            }
            appState.beginGlobalPushToTalk()
        } else if !pressed && isPressed {
            isPressed = false
            if suppressNextRelease {
                suppressNextRelease = false
                return
            }
            appState.endGlobalPushToTalk()
        }
    }
}

private extension AppState.GlobalHotkeyTrigger {
    var keyCode: UInt16 {
        switch self {
        case .functionKey:
            return 63
        case .rightOption:
            return 61
        }
    }

    func isPressed(in event: NSEvent) -> Bool {
        switch self {
        case .functionKey:
            return event.modifierFlags.contains(.function)
        case .rightOption:
            return event.modifierFlags.contains(.option)
        }
    }

    var supportsCommandToggle: Bool {
        switch self {
        case .functionKey:
            return true
        case .rightOption:
            return false
        }
    }
}
