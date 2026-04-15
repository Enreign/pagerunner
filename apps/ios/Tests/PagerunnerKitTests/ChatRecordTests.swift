import Testing
import Foundation
@testable import PagerunnerKit

@Suite("ChatRecord")
struct ChatRecordTests {
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    @Test func userRoundtrip() throws {
        let record = ChatRecord.user(id: UUID(), text: "hello", sentAt: Date(timeIntervalSince1970: 1_000_000))
        let data = try encoder.encode(record)
        let decoded = try decoder.decode(ChatRecord.self, from: data)
        #expect(decoded == record)
    }

    @Test func agentDoneRoundtrip() throws {
        let record = ChatRecord.agentDone(id: UUID(), summary: "Did the thing", at: Date(timeIntervalSince1970: 1_000_001))
        let data = try encoder.encode(record)
        let decoded = try decoder.decode(ChatRecord.self, from: data)
        #expect(decoded == record)
    }

    @Test func screenshotMetadataRoundtrip() throws {
        let record = ChatRecord.screenshot(
            id: UUID(),
            sessionId: "s-1",
            targetId: "t-1",
            tabTitle: "Inbox",
            tabUrl: "https://gmail.com/",
            at: Date(timeIntervalSince1970: 1_000_002)
        )
        let data = try encoder.encode(record)
        let decoded = try decoder.decode(ChatRecord.self, from: data)
        #expect(decoded == record)
    }

    @Test func errorRoundtrip() throws {
        let record = ChatRecord.error(id: UUID(), message: "boom", at: Date(timeIntervalSince1970: 1_000_003))
        let data = try encoder.encode(record)
        let decoded = try decoder.decode(ChatRecord.self, from: data)
        #expect(decoded == record)
    }
}
