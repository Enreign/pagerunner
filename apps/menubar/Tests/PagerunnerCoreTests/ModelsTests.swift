import Foundation
import Testing
@testable import PagerunnerCore

@Suite("Models Codable round-trips")
struct ModelsTests {

    @Test("ListProfilesResponse decodes correctly")
    func listProfilesResponse() throws {
        let json = """
        {"ok":true,"data":[
            {"name":"personal","display_name":"Stas","kind":"personal"},
            {"name":"agent-1","display_name":"Agent 1","kind":"agent"}
        ]}
        """
        let resp = try JSONDecoder().decode(ListProfilesResponse.self, from: Data(json.utf8))
        #expect(resp.ok == true)
        #expect(resp.data.count == 2)
        #expect(resp.data[0].name == "personal")
        #expect(resp.data[0].kind == "personal")
        #expect(resp.data[1].kind == "agent")
    }

    @Test("ListSessionsResponse decodes with alive and crashed status")
    func listSessionsResponse() throws {
        let json = """
        {"ok":true,"data":[
            {"id":"s1","profile":"personal","display_name":"Stas","stealth":false,"status":"alive"},
            {"id":"s2","profile":"personal","display_name":"Stas","stealth":true,"status":"crashed"}
        ]}
        """
        let resp = try JSONDecoder().decode(ListSessionsResponse.self, from: Data(json.utf8))
        #expect(resp.data[0].status == .alive)
        #expect(resp.data[1].status == .crashed)
        #expect(resp.data[1].stealth == true)
    }

    @Test("ListTabsResponse decodes correctly")
    func listTabsResponse() throws {
        let json = """
        {"ok":true,"data":[
            {"target_id":"T1","url":"https://github.com/anthropics","title":"anthropics · GitHub"}
        ]}
        """
        let resp = try JSONDecoder().decode(ListTabsResponse.self, from: Data(json.utf8))
        #expect(resp.data[0].targetId == "T1")
        #expect(resp.data[0].url == "https://github.com/anthropics")
    }

    @Test("ListCheckpointsResponse decodes correctly")
    func listCheckpointsResponse() throws {
        let json = """
        {"ok":true,"data":[{
            "checkpoint_id":"ckpt-uuid","name":"Research sprint",
            "saved_at":1711500000,"profile":"personal",
            "tab_count":3,"origins":["github.com","linear.app","notion.so"]
        }]}
        """
        let resp = try JSONDecoder().decode(ListCheckpointsResponse.self, from: Data(json.utf8))
        #expect(resp.data[0].checkpointId == "ckpt-uuid")
        #expect(resp.data[0].tabCount == 3)
        #expect(resp.data[0].origins.count == 3)
    }

    @Test("DaemonResponse with double-serialized result")
    func daemonResponseInner() throws {
        // The daemon wraps the inner JSON as a string
        let innerJSON = #"{"ok":true,"data":[]}"#
        let outer = """
        {"id":"abc","result":"\(innerJSON.replacingOccurrences(of: "\"", with: "\\\""))","error":null}
        """
        let resp = try JSONDecoder().decode(DaemonResponse.self, from: Data(outer.utf8))
        #expect(resp.id == "abc")
        #expect(resp.error == nil)
        // result should be the raw escaped string
        #expect(resp.result?.contains("\"ok\"") == true)
    }

    // MARK: - AttachState tests

    @Test("AttachState cases exist and Equatable works")
    func attachStateEquatable() {
        let idle = AttachState.idle
        let attaching = AttachState.attaching
        let attached = AttachState.attached(profileDisplayName: "Test")
        let failed = AttachState.failed("err")

        #expect(idle == .idle)
        #expect(attaching == .attaching)
        #expect(attached == .attached(profileDisplayName: "Test"))
        #expect(failed == .failed("err"))

        // Different cases are not equal
        #expect(idle != attaching)
        #expect(attached != failed)
        #expect(AttachState.failed("a") != AttachState.failed("b"))
    }

    // MARK: - DiscoveredInstance tests

    @Test("DiscoveredInstance initializes with correct values")
    func discoveredInstanceCreation() {
        let instance = DiscoveredInstance(
            id: "port-9225",
            port: 9225,
            tabCount: 3,
            isVM: true,
            attachState: .idle
        )

        #expect(instance.id == "port-9225")
        #expect(instance.port == 9225)
        #expect(instance.tabCount == 3)
        #expect(instance.isVM == true)
        #expect(instance.attachState == .idle)
    }

    @Test("DiscoveredInstance attachState can be mutated")
    func discoveredInstanceAttachStateMutation() {
        var instance = DiscoveredInstance(
            id: "port-9226",
            port: 9226,
            tabCount: 1,
            isVM: false,
            attachState: .idle
        )

        instance.attachState = .attaching
        #expect(instance.attachState == .attaching)

        instance.attachState = .attached(profileDisplayName: "Test")
        #expect(instance.attachState == .attached(profileDisplayName: "Test"))

        instance.attachState = .failed("connection refused")
        #expect(instance.attachState == .failed("connection refused"))
    }

    // MARK: - Profile.debugPort and kind? tests

    @Test("Profile decodes with debugPort and kind present")
    func profileDecodesWithDebugPortAndKind() throws {
        let json = """
        {"ok":true,"data":[
            {"name":"attached-1","display_name":"Chrome 9225","kind":"attached","debug_port":9225}
        ]}
        """
        let resp = try JSONDecoder().decode(ListProfilesResponse.self, from: Data(json.utf8))
        #expect(resp.data[0].name == "attached-1")
        #expect(resp.data[0].kind == "attached")
        #expect(resp.data[0].debugPort == 9225)
    }

    @Test("Profile decodes without debugPort and kind (existing profiles still decode)")
    func profileDecodesWithoutDebugPortAndKind() throws {
        let json = """
        {"ok":true,"data":[
            {"name":"personal","display_name":"Stas"}
        ]}
        """
        let resp = try JSONDecoder().decode(ListProfilesResponse.self, from: Data(json.utf8))
        #expect(resp.data[0].name == "personal")
        #expect(resp.data[0].kind == nil)
        #expect(resp.data[0].debugPort == nil)
    }
}
