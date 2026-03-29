import AppKit
import SwiftUI
import PagerunnerCore

@MainActor
final class StatusItemController {
    private var statusItem: NSStatusItem
    private var popover: NSPopover
    private let appState: AppState
    private let pollingService: PollingService

    init(appState: AppState, pollingService: PollingService) {
        self.appState = appState
        self.pollingService = pollingService

        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        popover = NSPopover()
        popover.behavior = .transient
        popover.animates = true

        setupStatusButton()
        setupPopover()
    }

    private func setupStatusButton() {
        guard let button = statusItem.button else { return }
        button.action = #selector(togglePopover)
        button.target = self
        observeStatusIcon()
    }

    /// Reactively update the status bar icon based on daemon state.
    /// Uses withObservationTracking to re-run whenever appState changes.
    private func observeStatusIcon() {
        withObservationTracking {
            updateIcon()
        } onChange: {
            Task { @MainActor in self.observeStatusIcon() }
        }
    }

    private func updateIcon() {
        guard let button = statusItem.button else { return }
        let symbolName: String
        switch (appState.transition, appState.daemonStatus) {
        case (.starting, _), (.restarting, _), (.stopping, _):
            symbolName = "figure.walk"
        case (_, .running):
            symbolName = "figure.run"
        case (_, .stale):
            symbolName = "figure.walk"
        case (_, .stopped):
            symbolName = "figure.stand"
        default:
            symbolName = "figure.stand"
        }
        let img = NSImage(systemSymbolName: symbolName, accessibilityDescription: "Pagerunner")
        img?.isTemplate = true
        button.image = img
    }

    private func setupPopover() {
        let contentView = PanelView(appState: appState, pollingService: pollingService, controller: self)
            .environment(\.daemonClient, DaemonClient())
        let hostingVC = FirstClickHostingController(rootView: contentView)
        popover.contentViewController = hostingVC
        popover.contentSize = NSSize(width: 310, height: 560)
    }

    @objc private func togglePopover() {
        if popover.isShown {
            closePopover()
        } else {
            openPopover()
        }
    }

    func openPopover() {
        guard let button = statusItem.button else { return }
        pollingService.panelDidOpen()
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        // Make popover the key window so clicks register immediately
        popover.contentViewController?.view.window?.makeKey()
    }

    func closePopover() {
        pollingService.panelDidClose()
        popover.performClose(nil)
    }

}

/// NSHostingView subclass that accepts first mouse click without requiring window activation.
private class FirstClickView<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

/// NSHostingController that uses FirstClickView so popover buttons respond on first click.
class FirstClickHostingController<Content: View>: NSHostingController<Content> {
    override func loadView() {
        view = FirstClickView(rootView: rootView)
    }
}

extension StatusItemController {
    /// Focus a specific Chrome tab by session + targetId via CDP, then bring Chrome to front.
    func focusTab(sessionId: String, targetId: String) {
        let daemon = DaemonClient()
        Task {
            // Target.activateTarget focuses both the tab and the Chrome window on macOS.
            // AppleScript "activate" is intentionally omitted — it brings whichever Chrome
            // is registered first to front, which is wrong when multiple instances are running.
            _ = try? await daemon.call(tool: "activate_tab", args: [
                "session_id": sessionId,
                "target_id": targetId
            ])
        }
    }
}
