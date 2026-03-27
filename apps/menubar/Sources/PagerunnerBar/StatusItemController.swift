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
        // Template image auto-adapts dark/light mode
        button.image = NSImage(systemSymbolName: "safari", accessibilityDescription: "Pagerunner")
        button.image?.isTemplate = true
        button.action = #selector(togglePopover)
        button.target = self
    }

    private func setupPopover() {
        let contentView = PanelView(appState: appState, pollingService: pollingService, controller: self)
        let hostingVC = NSHostingController(rootView: contentView)
        hostingVC.view.frame = NSRect(x: 0, y: 0, width: 310, height: 560)
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
    }

    func closePopover() {
        pollingService.panelDidClose()
        popover.performClose(nil)
    }

    /// Focus a Chrome tab by URL using AppleScript.
    func focusTab(url: String) {
        let script = """
        tell application "Google Chrome"
            set winList to every window
            repeat with w in winList
                set tabList to every tab of w
                repeat with t in tabList
                    if URL of t is "\(url)" then
                        set index of w to 1
                        set active tab index of w to (get index of t)
                        activate
                        return
                    end if
                end repeat
            end repeat
        end tell
        """
        var error: NSDictionary?
        NSAppleScript(source: script)?.executeAndReturnError(&error)
    }
}
