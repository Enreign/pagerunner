import Testing
import Foundation
@testable import PagerunnerKit

@Suite("Scope migration")
struct ScopeMigrationTests {
    private let encoder = JSONEncoder()
    private let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .iso8601
        return d
    }()

    @Test func legacyPinnedContextBecomesSingleTabScope() throws {
        // Payload written by the previous iOS release (scope-free schema).
        let legacy = """
        {
          "id": "F47AC10B-58CC-4372-A567-0E02B2C3D479",
          "title": "existing thread",
          "pinnedContext": { "sessionId": "s-legacy", "targetId": "t-legacy" },
          "records": [],
          "createdAt": "2026-04-14T10:00:00Z",
          "updatedAt": "2026-04-14T10:00:00Z"
        }
        """.data(using: .utf8)!
        let thread = try decoder.decode(ChatThread.self, from: legacy)
        #expect(thread.title == "existing thread")
        #expect(thread.scope.tabs.count == 1)
        #expect(thread.scope.tabs[0].sessionId == "s-legacy")
        #expect(thread.scope.tabs[0].targetId == "t-legacy")
        #expect(thread.scope.tabs[0].label == "") // rehydrated at runtime
        #expect(thread.scope.goal == nil)
    }

    @Test func legacyWithNilPinnedContextBecomesEmptyScope() throws {
        let legacy = """
        {
          "id": "F47AC10B-58CC-4372-A567-0E02B2C3D480",
          "title": "untouched",
          "pinnedContext": null,
          "records": [],
          "createdAt": "2026-04-14T10:00:00Z",
          "updatedAt": "2026-04-14T10:00:00Z"
        }
        """.data(using: .utf8)!
        let thread = try decoder.decode(ChatThread.self, from: legacy)
        #expect(thread.scope.tabs.isEmpty)
    }

    @Test func legacyMissingPinnedContextBecomesEmptyScope() throws {
        let legacy = """
        {
          "id": "F47AC10B-58CC-4372-A567-0E02B2C3D481",
          "title": "very old",
          "records": [],
          "createdAt": "2026-04-14T10:00:00Z",
          "updatedAt": "2026-04-14T10:00:00Z"
        }
        """.data(using: .utf8)!
        let thread = try decoder.decode(ChatThread.self, from: legacy)
        #expect(thread.scope.tabs.isEmpty)
    }

    @Test func newScopeRoundtripPreservesShape() throws {
        let tab = ScopeTab(sessionId: "s-1", targetId: "t-a", label: "Notion")
        let thread = ChatThread(
            title: "fresh",
            scope: Scope(tabs: [tab], goal: "test"),
            records: []
        )
        let enc = JSONEncoder()
        enc.dateEncodingStrategy = .iso8601
        let data = try enc.encode(thread)
        let decoded = try decoder.decode(ChatThread.self, from: data)
        #expect(decoded.scope.tabs == [tab])
        #expect(decoded.scope.goal == "test")
    }
}
