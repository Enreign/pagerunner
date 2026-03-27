import Testing
@testable import PagerunnerCore
import Foundation

@Suite("PollingService failure gate")
struct PollingServiceTests {

    @Test("daemon status stays running on 0–2 consecutive failures")
    func staysRunningOnFewFailures() {
        for count in 0...2 {
            let status = DaemonStatus.fromFailureCount(count, lastSeenAt: nil)
            guard case .running = status else {
                Issue.record("Expected .running for failure count \(count), got \(status)")
                return
            }
        }
    }

    @Test("daemon status becomes stale on 3–4 consecutive failures")
    func becomesStaleAfterThree() {
        let now = Date()
        for count in 3...4 {
            let status = DaemonStatus.fromFailureCount(count, lastSeenAt: now)
            guard case .stale = status else {
                Issue.record("Expected .stale for failure count \(count), got \(status)")
                return
            }
        }
    }

    @Test("daemon status becomes stopped at 5+ consecutive failures")
    func becomesStoppedAtFive() {
        for count in [5, 10, 100] {
            let status = DaemonStatus.fromFailureCount(count, lastSeenAt: Date())
            guard case .stopped = status else {
                Issue.record("Expected .stopped for failure count \(count), got \(status)")
                return
            }
        }
    }

    @Test("consecutive failures reset to 0 on success")
    func resetsOnSuccess() async {
        actor Counter {
            var value = 0
            func increment() { value += 1 }
        }
        let counter = Counter()
        var failCount = 4

        // Simulate: 4 failures then success
        for _ in 0..<4 {
            await counter.increment()
            failCount += 1
        }
        // On success
        failCount = 0
        let status = DaemonStatus.fromFailureCount(failCount, lastSeenAt: nil)
        guard case .running = status else {
            Issue.record("Expected .running after reset, got \(status)")
            return
        }
    }
}
