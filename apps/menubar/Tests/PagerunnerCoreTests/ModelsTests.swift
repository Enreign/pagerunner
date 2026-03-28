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
}
