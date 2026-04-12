import Testing
import Foundation
@testable import PagerunnerKit

@Suite("Model Decoding")
struct ModelsTests {

    private let decoder: JSONDecoder = {
        let d = JSONDecoder()
        return d
    }()

    @Test func profileDecoding() throws {
        let json = """
        {"name": "personal", "display_name": "Personal (user@example.com)", "kind": "personal", "user_data_dir": "/tmp/chrome", "debug_port": null}
        """.data(using: .utf8)!
        let profile = try decoder.decode(Profile.self, from: json)
        #expect(profile.name == "personal")
        #expect(profile.displayName == "Personal (user@example.com)")
        #expect(profile.kind == "personal")
        #expect(profile.userDataDir == "/tmp/chrome")
        #expect(profile.debugPort == nil)
    }

    @Test func sessionDecoding() throws {
        let json = """
        {"id": "s-123", "profile": "personal", "display_name": "Personal", "stealth": false, "status": "alive"}
        """.data(using: .utf8)!
        let session = try decoder.decode(Session.self, from: json)
        #expect(session.id == "s-123")
        #expect(session.status == .alive)
        #expect(session.isAlive)
        #expect(!session.stealth)
    }

    @Test func sessionCrashedStatus() throws {
        let json = """
        {"id": "s-456", "profile": "work", "display_name": "Work", "stealth": true, "status": "crashed"}
        """.data(using: .utf8)!
        let session = try decoder.decode(Session.self, from: json)
        #expect(session.status == .crashed)
        #expect(!session.isAlive)
        #expect(session.stealth)
    }

    @Test func tabDecoding() throws {
        let json = """
        {"target_id": "t-abc", "url": "https://example.com", "title": "Example"}
        """.data(using: .utf8)!
        let tab = try decoder.decode(Tab.self, from: json)
        #expect(tab.targetId == "t-abc")
        #expect(tab.url == "https://example.com")
        #expect(tab.title == "Example")
        #expect(tab.id == "t-abc")
    }

    @Test func checkpointDecoding() throws {
        let json = """
        {"checkpoint_id": "cp-1", "name": "Before deploy", "saved_at": 1700000000, "profile": "work", "tab_count": 3, "origins": ["https://github.com"]}
        """.data(using: .utf8)!
        let cp = try decoder.decode(Checkpoint.self, from: json)
        #expect(cp.checkpointId == "cp-1")
        #expect(cp.name == "Before deploy")
        #expect(cp.tabCount == 3)
        #expect(cp.origins == ["https://github.com"])
    }

    @Test func networkLogEntryDecoding() throws {
        let json = """
        {"request_id": "r1", "url": "https://api.example.com/data", "method": "GET", "status": 200, "duration_ms": 150, "timestamp_ms": 1700000000000, "tab_id": "t1"}
        """.data(using: .utf8)!
        let entry = try decoder.decode(NetworkLogEntry.self, from: json)
        #expect(entry.requestId == "r1")
        #expect(entry.method == "GET")
        #expect(entry.status == 200)
        #expect(entry.durationMs == 150)
    }

    @Test func networkLogResultDecoding() throws {
        let json = """
        {"ok": true, "entries": [], "total_matched": 0, "total_captured": 10, "result_truncated": false}
        """.data(using: .utf8)!
        let result = try decoder.decode(NetworkLogResult.self, from: json)
        #expect(result.ok)
        #expect(result.entries.isEmpty)
        #expect(result.totalCaptured == 10)
    }

    @Test func dataEnvelopeDecoding() throws {
        let json = """
        {"data": [{"name": "test", "display_name": "Test", "kind": null, "user_data_dir": null, "debug_port": null}]}
        """.data(using: .utf8)!
        let envelope = try decoder.decode(DataEnvelope<[Profile]>.self, from: json)
        #expect(envelope.data.count == 1)
        #expect(envelope.data[0].name == "test")
    }

    @Test func healthResponseDecoding() throws {
        let json = """
        {"status": "ok", "version": "0.8.0"}
        """.data(using: .utf8)!
        let health = try decoder.decode(HealthResponse.self, from: json)
        #expect(health.status == "ok")
        #expect(health.version == "0.8.0")
    }

    @Test func notificationDecoding() throws {
        let json = """
        {"id": "notif-1", "title": "Session crashed", "body": "Chrome exited", "level": "error", "session_id": "s1", "profile_name": "personal", "created_at": 1700000000000000}
        """.data(using: .utf8)!
        let notif = try decoder.decode(DaemonNotification.self, from: json)
        #expect(notif.notificationId == "notif-1")
        #expect(notif.title == "Session crashed")
        #expect(notif.level == "error")
    }

    @Test func toolCallResponseDecoding() throws {
        let json = """
        {"ok": true, "result": {"session_id": "s-new"}, "error": null}
        """.data(using: .utf8)!
        let resp = try decoder.decode(ToolCallResponse.self, from: json)
        #expect(resp.ok)
        #expect(resp.error == nil)
    }

    @Test func toolCallResponseError() throws {
        let json = """
        {"ok": false, "result": null, "error": "Profile not found"}
        """.data(using: .utf8)!
        let resp = try decoder.decode(ToolCallResponse.self, from: json)
        #expect(!resp.ok)
        #expect(resp.error == "Profile not found")
    }
}

@Suite("Agent Event Decoding")
struct AgentEventTests {
    private let decoder = JSONDecoder()

    @Test func thinkingEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "thinking", "text": "Analyzing..."}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        #expect(event.runId == "r1")
        if case .thinking(let text) = event.event {
            #expect(text == "Analyzing...")
        } else {
            Issue.record("Expected thinking event")
        }
    }

    @Test func toolCallEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "tool_call", "name": "click", "args": {"selector": "#btn"}}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .toolCall(let name, _) = event.event {
            #expect(name == "click")
        } else {
            Issue.record("Expected tool_call event")
        }
    }

    @Test func toolResultEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "tool_result", "name": "click", "result": "ok", "is_error": false}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .toolResult(let name, let result, let isError) = event.event {
            #expect(name == "click")
            #expect(result == "ok")
            #expect(!isError)
        } else {
            Issue.record("Expected tool_result event")
        }
    }

    @Test func approvalRequiredEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "approval_required", "run_id": "r1", "action": "navigate", "description": "Going to admin page"}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .approvalRequired(let runId, let action, let desc) = event.event {
            #expect(runId == "r1")
            #expect(action == "navigate")
            #expect(desc == "Going to admin page")
        } else {
            Issue.record("Expected approval_required event")
        }
    }

    @Test func doneEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "done", "summary": "Task completed"}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .done(let summary) = event.event {
            #expect(summary == "Task completed")
        } else {
            Issue.record("Expected done event")
        }
    }

    @Test func errorEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "error", "message": "Timeout", "recoverable": false}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .error(let message, let recoverable) = event.event {
            #expect(message == "Timeout")
            #expect(!recoverable)
        } else {
            Issue.record("Expected error event")
        }
    }

    @Test func interruptedEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "interrupted"}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .interrupted = event.event {
            // pass
        } else {
            Issue.record("Expected interrupted event")
        }
    }

    @Test func budgetExceededEvent() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "budget_exceeded", "reason": "Max steps reached"}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .budgetExceeded(let reason) = event.event {
            #expect(reason == "Max steps reached")
        } else {
            Issue.record("Expected budget_exceeded event")
        }
    }

    @Test func unknownEventType() throws {
        let json = """
        {"run_id": "r1", "event": {"type": "future_event"}}
        """.data(using: .utf8)!
        let event = try decoder.decode(AgentEvent.self, from: json)
        if case .unknown(let type) = event.event {
            #expect(type == "future_event")
        } else {
            Issue.record("Expected unknown event")
        }
    }
}

@Suite("AnyCodableValue")
struct AnyCodableValueTests {
    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()

    @Test func decodeString() throws {
        let json = "\"hello\"".data(using: .utf8)!
        let value = try decoder.decode(AnyCodableValue.self, from: json)
        #expect(value.stringValue == "hello")
    }

    @Test func decodeInt() throws {
        let json = "42".data(using: .utf8)!
        let value = try decoder.decode(AnyCodableValue.self, from: json)
        #expect(value.intValue == 42)
    }

    @Test func decodeNull() throws {
        let json = "null".data(using: .utf8)!
        let value = try decoder.decode(AnyCodableValue.self, from: json)
        #expect(value.isNull)
    }

    @Test func decodeObject() throws {
        let json = "{\"key\": \"value\"}".data(using: .utf8)!
        let value = try decoder.decode(AnyCodableValue.self, from: json)
        #expect(value["key"]?.stringValue == "value")
    }

    @Test func decodeArray() throws {
        let json = "[1, 2, 3]".data(using: .utf8)!
        let value = try decoder.decode(AnyCodableValue.self, from: json)
        #expect(value.arrayValue?.count == 3)
        #expect(value[0]?.intValue == 1)
    }

    @Test func roundtrip() throws {
        let original = AnyCodableValue.object([
            "name": .string("test"),
            "count": .int(42),
            "active": .bool(true),
        ])
        let data = try encoder.encode(original)
        let decoded = try decoder.decode(AnyCodableValue.self, from: data)
        #expect(decoded == original)
    }

    @Test func initFromAny() {
        let value = AnyCodableValue(["key": "value", "num": 5] as [String: Any])
        #expect(value["key"]?.stringValue == "value")
        #expect(value["num"]?.intValue == 5)
    }
}

@Suite("WebSocket Message")
struct WSMessageTests {
    private let decoder = JSONDecoder()

    @Test func sessionStatusMessage() throws {
        let json = """
        {"type": "session_status", "data": {"data": []}}
        """.data(using: .utf8)!
        let msg = try decoder.decode(WSMessage.self, from: json)
        #expect(msg.type == .sessionStatus)
    }

    @Test func notificationMessage() throws {
        let json = """
        {"type": "notification", "data": {"id": "n1", "title": "Test", "level": "info", "created_at": 0}}
        """.data(using: .utf8)!
        let msg = try decoder.decode(WSMessage.self, from: json)
        #expect(msg.type == .notification)
    }

    @Test func toolResultMessage() throws {
        let json = """
        {"type": "tool_result", "ok": true, "result": {"session_id": "s1"}}
        """.data(using: .utf8)!
        let msg = try decoder.decode(WSMessage.self, from: json)
        #expect(msg.type == .toolResult)
        #expect(msg.ok == true)
    }
}
