import Foundation
import Testing
@testable import PagerunnerCore

@Suite("ConfigEditor")
struct ConfigEditorTests {

    let sampleConfig = """
        [daemon]
        log_level = "info"

        [[profiles]]
        name = "work"
        display_name = "Work Chrome"
        user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Default"

        [[profiles]]
        name = "personal"
        display_name = "Personal Chrome"
        user_data_dir = "/Users/stas/Library/Application Support/Google/Chrome/Profile 1"
        """

    func writeTempConfig(_ content: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("config_editor_test_\(UUID().uuidString).toml")
        try content.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    // Test 1: renameProfile updates the correct display_name, leaves others untouched
    @Test func renameProfileUpdatesDisplayName() throws {
        let url = try writeTempConfig(sampleConfig)
        defer { try? FileManager.default.removeItem(at: url) }

        try ConfigEditor.renameProfile(name: "work", newDisplayName: "Work Laptop", configURL: url)

        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(result.contains("display_name = \"Work Laptop\""))
        // personal profile must remain untouched
        #expect(result.contains("display_name = \"Personal Chrome\""))
        // original work display_name must be gone
        #expect(!result.contains("display_name = \"Work Chrome\""))
    }

    // Test 2: removeProfile removes the correct [[profiles]] block, leaves others untouched
    @Test func removeProfileRemovesBlock() throws {
        let url = try writeTempConfig(sampleConfig)
        defer { try? FileManager.default.removeItem(at: url) }

        try ConfigEditor.removeProfile(name: "work", configURL: url)

        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(!result.contains("name = \"work\""))
        #expect(!result.contains("Work Chrome"))
        // personal profile must remain
        #expect(result.contains("name = \"personal\""))
        #expect(result.contains("Personal Chrome"))
        // preamble must remain
        #expect(result.contains("[daemon]"))
    }

    // Test 3: addAttachedProfile appends block with kind="attached" and debug_port
    @Test func addAttachedProfileAppendsBlock() throws {
        let url = try writeTempConfig(sampleConfig)
        defer { try? FileManager.default.removeItem(at: url) }

        try ConfigEditor.addAttachedProfile(name: "agent-1", displayName: "Agent Tab", port: 9222, configURL: url)

        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(result.contains("name = \"agent-1\""))
        #expect(result.contains("display_name = \"Agent Tab\""))
        #expect(result.contains("kind = \"attached\""))
        #expect(result.contains("debug_port = 9222"))
        // existing profiles must still be there
        #expect(result.contains("name = \"work\""))
        #expect(result.contains("name = \"personal\""))
    }

    // Test 4: renameProfile throws if profile name not found
    @Test func renameProfileThrowsIfNotFound() throws {
        let url = try writeTempConfig(sampleConfig)
        defer { try? FileManager.default.removeItem(at: url) }

        #expect(throws: (any Error).self) {
            try ConfigEditor.renameProfile(name: "nonexistent", newDisplayName: "Whatever", configURL: url)
        }
    }

    // Test 5: removeProfile throws if profile name not found
    @Test func removeProfileThrowsIfNotFound() throws {
        let url = try writeTempConfig(sampleConfig)
        defer { try? FileManager.default.removeItem(at: url) }

        #expect(throws: (any Error).self) {
            try ConfigEditor.removeProfile(name: "nonexistent", configURL: url)
        }
    }
}
