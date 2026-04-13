import AppKit
import Observation
import SwiftUI

@MainActor
final class VoiceHUDController {
    private let appState: AppState
    private let panel: NSPanel
    private let hostingController: NSHostingController<VoiceHUDView>

    init(appState: AppState) {
        self.appState = appState

        let rootView = VoiceHUDView(appState: appState)
        self.hostingController = NSHostingController(rootView: rootView)
        self.panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 320, height: 104),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )

        panel.isFloatingPanel = true
        panel.level = .statusBar
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.hidesOnDeactivate = false
        panel.ignoresMouseEvents = true
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        panel.contentViewController = hostingController
        panel.orderOut(nil)

        observeState()
    }

    private func observeState() {
        withObservationTracking {
            _ = appState.shouldShowVoiceHUD
            _ = appState.isVoiceHUDExpanded
            _ = appState.voiceHUDTitle
            _ = appState.voiceHUDDetail
            _ = appState.globalPushToTalkPressed
            _ = appState.voiceTranscriptPreview
            _ = appState.voiceError
            _ = appState.voiceStatus
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                self?.observeState()
                self?.updateVisibility()
            }
        }

        updateVisibility()
    }

    private func updateVisibility() {
        if appState.shouldShowVoiceHUD {
            resizePanel()
            positionPanel()
            panel.orderFrontRegardless()
            return
        }
        panel.orderOut(nil)
        appState.voiceTranscriptPreview = ""
    }

    private func resizePanel() {
        let targetSize = appState.isVoiceHUDExpanded
            ? NSSize(width: 320, height: 104)
            : NSSize(width: 244, height: 62)
        guard panel.frame.size != targetSize else { return }
        panel.setContentSize(targetSize)
    }

    private func positionPanel() {
        guard let screen = NSScreen.main ?? NSScreen.screens.first else { return }
        let visible = screen.visibleFrame
        let size = panel.frame.size
        let x = visible.midX - size.width / 2
        let y = visible.minY + 26
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }
}
