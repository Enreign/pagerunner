import Testing
@testable import PagerunnerBar
@testable import PagerunnerCore

@Suite("AppState")
@MainActor
struct AppStateTests {
    @Test("agent is the default landing view")
    func defaultNavigationIsAgent() {
        let state = AppState()
        #expect(state.navigation == .agent)
    }

    @Test("starting voice without a resolved binary surfaces an actionable error")
    func startVoiceWithoutBinarySetsVoiceError() {
        let state = AppState()

        state.startVoice()

        #expect(state.voiceActive == false)
        #expect(state.voiceError != nil)
    }

    @Test("voice-first defaults use push-to-talk with the Fn trigger")
    func voiceFirstDefaults() {
        let state = AppState()

        #expect(state.voiceMode == .pushToTalk)
        #expect(state.globalPushToTalkEnabled)
        #expect(state.globalHotkeyTrigger == .functionKey)
        #expect(state.shouldShowVoiceHUD)
        #expect(state.isVoiceHUDExpanded == false)
    }

    @Test("voice HUD becomes visible while the global hold-to-talk key is down")
    func voiceHUDVisibilityTracksHotkey() {
        let state = AppState()

        state.globalPushToTalkPressed = true

        #expect(state.shouldShowVoiceHUD)
        #expect(state.voiceHUDTitle == "Hold to talk")
        #expect(state.voiceHUDDetail == "Hold Fn to talk, or tap Command-Fn to start and stop dictation")
    }

    @Test("continuous dictation shortcut switches voice into always-listening mode")
    func continuousDictationShortcutPrefersAlwaysListening() {
        let state = AppState()

        state.toggleContinuousDictation()

        #expect(state.voiceMode == .alwaysListening)
        #expect(state.voiceError != nil)
    }

    @Test("checkpointsFor returns checkpoints for matching profile only")
    func checkpointsForFiltersCorrectly() {
        let state = AppState()
        let cp1 = Checkpoint(checkpointId: "cp1", name: "morning", savedAt: 1_700_000_000,
                             profile: "personal", tabCount: 2, origins: ["https://linear.app"])
        let cp2 = Checkpoint(checkpointId: "cp2", name: "afternoon", savedAt: 1_700_001_000,
                             profile: "personal", tabCount: 1, origins: ["https://github.com"])
        let cp3 = Checkpoint(checkpointId: "cp3", name: "agent-save", savedAt: 1_700_002_000,
                             profile: "agent-1", tabCount: 3, origins: [])
        state.checkpoints = ["personal": [cp1, cp2], "agent-1": [cp3]]

        let result = state.checkpointsFor(profile: "personal")
        #expect(result.count == 2)
        #expect(result.map(\.checkpointId).contains("cp1"))
        #expect(result.map(\.checkpointId).contains("cp2"))
    }

    @Test("checkpointsFor returns empty array when profile has no checkpoints")
    func checkpointsForReturnsEmptyForMissingProfile() {
        let state = AppState()
        state.checkpoints = [:]
        #expect(state.checkpointsFor(profile: "nonexistent").isEmpty)
    }
}
