import Testing
import Foundation
@testable import PagerunnerKit

@Suite("Scope")
struct ScopeTests {
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    @Test func emptyScopeRoundtrip() throws {
        let s = Scope()
        let data = try encoder.encode(s)
        let decoded = try decoder.decode(Scope.self, from: data)
        #expect(decoded == s)
        #expect(decoded.tabs.isEmpty)
        #expect(decoded.goal == nil)
        #expect(decoded.notes == nil)
        #expect(decoded.turnLog.isEmpty)
    }

    @Test func fullScopeRoundtrip() throws {
        let tab = ScopeTab(
            sessionId: "s-1",
            targetId: "t-a",
            label: "Notion – Budget",
            purpose: "source",
            digest: "47 rows",
            lastTouchedAt: Date(timeIntervalSince1970: 1)
        )
        let entry = TurnLogEntry(
            userGoal: "check budget",
            summary: "Pulled rows 1–47",
            touchedTabIds: [tab.id],
            timestamp: Date(timeIntervalSince1970: 2)
        )
        let s = Scope(tabs: [tab], goal: "weekly review", notes: "header row is 2", turnLog: [entry])
        let data = try encoder.encode(s)
        let decoded = try decoder.decode(Scope.self, from: data)
        #expect(decoded == s)
    }

    @Test func scopeTabIdDerivation() throws {
        let a = ScopeTab(sessionId: "s-1", targetId: "t-a", label: "x")
        let b = ScopeTab(sessionId: "s-1", targetId: nil, label: "y")
        #expect(a.id == "s-1-t-a")
        #expect(b.id == "s-1-first")
    }

    @Test func turnLogCapDropsOldest() {
        var s = Scope()
        for i in 0..<25 {
            s.append(TurnLogEntry(
                userGoal: "g\(i)",
                summary: "s\(i)",
                touchedTabIds: [],
                timestamp: Date(timeIntervalSince1970: TimeInterval(i))
            ))
        }
        #expect(s.turnLog.count == 20)
        #expect(s.turnLog.first?.userGoal == "g5")   // first 5 dropped
        #expect(s.turnLog.last?.userGoal == "g24")
    }

    @Test func digestIsTruncatedAtFiveHundredChars() {
        var tab = ScopeTab(sessionId: "s-1", targetId: "t-a", label: "x")
        tab.setDigest(String(repeating: "z", count: 800))
        #expect(tab.digest?.count == 500)
    }
}
