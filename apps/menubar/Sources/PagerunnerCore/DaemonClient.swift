import Foundation

public enum DaemonError: Error, Sendable {
    case daemonStopped       // ENOENT or ECONNREFUSED
    case malformedResponse   // can't parse outer JSON
    case daemonError(String) // error field non-nil in response
}

/// Thin wrapper around a Unix domain socket connection to ~/.pagerunner/daemon.sock.
/// One fresh connection per call — no pooling. Matches the Rust DaemonClient pattern.
public struct DaemonClient: Sendable {
    public let socketPath: String

    public init(socketPath: String = {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return "\(home)/.pagerunner/daemon.sock"
    }()) {
        self.socketPath = socketPath
    }

    /// Call a pagerunner tool and return the parsed inner JSON.
    /// - throws: `DaemonError.daemonStopped` if the socket is not reachable
    /// - throws: `DaemonError.daemonError` if the daemon returned an error
    ///
    /// The synchronous POSIX socket I/O is wrapped in withCheckedThrowingContinuation
    /// + Task.detached so it runs on the cooperative thread pool without blocking the
    /// calling actor. This satisfies Swift 6 strict concurrency.
    public func call(tool: String, args: [String: Any] = [:]) async throws -> [String: JSONValue] {
        let socketPath = self.socketPath
        // Serialize args to Data here (on the calling isolation domain) so the
        // Task.detached closure only captures Sendable types (String, Data).
        let requestId = UUID().uuidString
        let request: [String: Any] = ["id": requestId, "tool": tool, "args": args]
        let requestData = try JSONSerialization.data(withJSONObject: request)
        var requestLineVar = requestData
        requestLineVar.append(0x0A) // newline
        let requestLine = requestLineVar // immutable copy for capture

        return try await withCheckedThrowingContinuation { continuation in
            Task.detached(priority: .utility) {
                do {
                    let result = try Self.performBlockingCall(socketPath: socketPath, requestLine: requestLine)
                    continuation.resume(returning: result)
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Synchronous implementation — runs on a background thread via withCheckedThrowingContinuation.
    /// Accepts a pre-serialized newline-terminated JSON request line so the closure
    /// passed to Task.detached only captures Sendable types (String, Data).
    private static func performBlockingCall(
        socketPath: String,
        requestLine: Data
    ) throws -> [String: JSONValue] {
        // Open fresh connection
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonError.daemonStopped }
        defer { Darwin.close(fd) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            socketPath.withCString { cstr in
                _ = Darwin.strcpy(
                    UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self),
                    cstr
                )
            }
        }

        let connectResult = withUnsafePointer(to: addr) { ptr in
            Darwin.connect(fd, UnsafeRawPointer(ptr).assumingMemoryBound(to: sockaddr.self), socklen_t(MemoryLayout<sockaddr_un>.size))
        }
        guard connectResult == 0 else { throw DaemonError.daemonStopped }

        // Write request line
        _ = requestLine.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }

        // Read response line
        var responseBytes = [UInt8]()
        var byte = UInt8(0)
        while Darwin.read(fd, &byte, 1) > 0 && byte != 0x0A {
            responseBytes.append(byte)
        }

        guard let outerJSON = try? JSONSerialization.jsonObject(with: Data(responseBytes)) as? [String: Any] else {
            throw DaemonError.malformedResponse
        }
        if let error = outerJSON["error"] as? String, !error.isEmpty {
            throw DaemonError.daemonError(error)
        }
        guard let resultStr = outerJSON["result"] as? String,
              let innerData = resultStr.data(using: .utf8),
              let innerJSON = try? JSONSerialization.jsonObject(with: innerData) as? [String: Any] else {
            throw DaemonError.malformedResponse
        }
        return innerJSON.mapValues { JSONValue($0) }
    }

    // MARK: - Agent streaming

    /// Enum for events received during an agent stream.
    public enum AgentStreamEvent: Sendable {
        case event(DaemonEventWire)
        case result(AgentRunResult)
        case error(String)
    }

    /// Start an agent run and stream events back.
    ///
    /// Opens a long-lived socket connection, sends the AgentRun message,
    /// then reads lines until the final DaemonResponse arrives.
    /// The caller receives an AsyncThrowingStream of AgentStreamEvent.
    public func streamAgentRun(
        goal: String,
        profile: String?,
        model: String?,
        maxSteps: Int?,
        mode: AgentMode
    ) -> AsyncThrowingStream<AgentStreamEvent, Error> {
        let socketPath = self.socketPath
        let requestId = UUID().uuidString

        // Build the agent config
        var config: [String: Any] = [:]
        config["autonomy"] = mode.autonomyArgs
        if let profile { config["session_profile"] = profile }
        if let model { config["model"] = model }
        if let maxSteps { config["budget"] = ["max_steps": maxSteps] }

        let message: [String: Any] = [
            "type": "agent_run",
            "id": requestId,
            "goal": goal,
            "config": config
        ]

        // Pre-serialize to Data so the detached closure only captures Sendable types
        let messageData: Data
        do {
            var serialized = try JSONSerialization.data(withJSONObject: message)
            serialized.append(0x0A)
            messageData = serialized
        } catch {
            return AsyncThrowingStream { $0.finish(throwing: error) }
        }

        return AsyncThrowingStream { continuation in
            Task.detached(priority: .utility) {
                do {
                    try Self.performStreamingRun(
                        socketPath: socketPath,
                        messageData: messageData,
                        continuation: continuation
                    )
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    /// Send an approval response on a fresh socket connection.
    public func sendApproval(runId: String, approved: Bool) async throws {
        let socketPath = self.socketPath
        let message: [String: Any] = [
            "type": "agent_approve",
            "id": UUID().uuidString,
            "run_id": runId,
            "approved": approved
        ]
        var lineVar = try JSONSerialization.data(withJSONObject: message)
        lineVar.append(0x0A)
        let line = lineVar // immutable copy for capture

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .utility) {
                do {
                    try Self.sendOneShotMessage(socketPath: socketPath, line: line)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    /// Send an interrupt on a fresh socket connection.
    public func sendInterrupt(runId: String) async throws {
        let socketPath = self.socketPath
        let message: [String: Any] = [
            "type": "agent_interrupt",
            "id": UUID().uuidString,
            "run_id": runId
        ]
        var lineVar = try JSONSerialization.data(withJSONObject: message)
        lineVar.append(0x0A)
        let line = lineVar // immutable copy for capture

        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            Task.detached(priority: .utility) {
                do {
                    try Self.sendOneShotMessage(socketPath: socketPath, line: line)
                    continuation.resume()
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    // MARK: - Shared helpers

    /// Connect to socket, send a single line, close. Used by sendApproval/sendInterrupt.
    private static func sendOneShotMessage(socketPath: String, line: Data) throws {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonError.daemonStopped }
        defer { Darwin.close(fd) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            socketPath.withCString { cstr in
                _ = Darwin.strcpy(
                    UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self),
                    cstr
                )
            }
        }
        let connectResult = withUnsafePointer(to: addr) { ptr in
            Darwin.connect(fd, UnsafeRawPointer(ptr).assumingMemoryBound(to: sockaddr.self), socklen_t(MemoryLayout<sockaddr_un>.size))
        }
        guard connectResult == 0 else { throw DaemonError.daemonStopped }
        _ = line.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }
    }

    // MARK: - Streaming internals

    /// Accepts pre-serialized newline-terminated message Data so the closure
    /// passed to Task.detached only captures Sendable types.
    private static func performStreamingRun(
        socketPath: String,
        messageData: Data,
        continuation: AsyncThrowingStream<AgentStreamEvent, Error>.Continuation
    ) throws {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonError.daemonStopped }
        defer { Darwin.close(fd) }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
            socketPath.withCString { cstr in
                _ = Darwin.strcpy(
                    UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self),
                    cstr
                )
            }
        }
        let connectResult = withUnsafePointer(to: addr) { ptr in
            Darwin.connect(fd, UnsafeRawPointer(ptr).assumingMemoryBound(to: sockaddr.self), socklen_t(MemoryLayout<sockaddr_un>.size))
        }
        guard connectResult == 0 else { throw DaemonError.daemonStopped }

        // Send the pre-serialized agent_run message
        _ = messageData.withUnsafeBytes { Darwin.write(fd, $0.baseAddress!, $0.count) }

        // Read lines until socket closes or we get a DaemonResponse
        var lineBuffer = [UInt8]()
        var byte = UInt8(0)
        while Darwin.read(fd, &byte, 1) > 0 {
            if byte == 0x0A {
                guard !lineBuffer.isEmpty else { continue }
                let data = Data(lineBuffer)
                lineBuffer.removeAll(keepingCapacity: true)

                // Try as DaemonEventWire first
                if let event = try? JSONDecoder().decode(DaemonEventWire.self, from: data) {
                    continuation.yield(.event(event))
                    continue
                }

                // Try as final DaemonResponse (has "id" + "result"/"error" fields)
                if let outerJSON = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                    if let errorStr = outerJSON["error"] as? String, !errorStr.isEmpty {
                        continuation.yield(.error(errorStr))
                        continuation.finish()
                        return
                    }
                    if let resultStr = outerJSON["result"] as? String,
                       let resultData = resultStr.data(using: .utf8),
                       let result = try? JSONDecoder().decode(AgentRunResult.self, from: resultData) {
                        continuation.yield(.result(result))
                        continuation.finish()
                        return
                    }
                }

                // Unknown line — skip
            } else {
                lineBuffer.append(byte)
            }
        }
        continuation.finish()
    }
}

// MARK: - JSONValue helper (lightweight typed wrapper for inner JSON)

public enum JSONValue: Sendable {
    case bool(Bool)
    case int(Int)
    case double(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])
    case null

    init(_ any: Any) {
        switch any {
        case let b as Bool:             self = .bool(b)
        case let i as Int:              self = .int(i)
        case let d as Double:           self = .double(d)
        case let s as String:           self = .string(s)
        case let a as [Any]:            self = .array(a.map { JSONValue($0) })
        case let o as [String: Any]:    self = .object(o.mapValues { JSONValue($0) })
        // JSONSerialization returns NSNumber for all numeric + bool JSON values.
        // Must come after Bool/Int/Double checks (Swift bridges those first).
        case let n as NSNumber:
            if n === kCFBooleanTrue as AnyObject  { self = .bool(true) }
            else if n === kCFBooleanFalse as AnyObject { self = .bool(false) }
            else { self = .double(n.doubleValue) }
        default:                        self = .null
        }
    }

    public var boolValue: Bool? {
        if case .bool(let b) = self { return b }
        return nil
    }
    public var stringValue: String? {
        if case .string(let s) = self { return s }
        return nil
    }
    public var arrayValue: [JSONValue]? {
        if case .array(let a) = self { return a }
        return nil
    }
    public var objectValue: [String: JSONValue]? {
        if case .object(let o) = self { return o }
        return nil
    }
}
