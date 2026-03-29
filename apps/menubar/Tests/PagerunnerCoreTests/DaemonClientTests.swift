import Testing
import Foundation
@testable import PagerunnerCore

/// Spins up a tiny local Unix socket server, handles one request, returns a canned response.
/// Used to test DaemonClient without a running pagerunner daemon.
actor MockSocketServer {
    let socketPath: String
    private var serverTask: Task<Void, Never>?

    init() {
        let tmp = FileManager.default.temporaryDirectory
        socketPath = tmp.appendingPathComponent("test-pagerunner-\(UUID().uuidString).sock").path
    }

    func start(responseJSON: String) {
        serverTask = Task {
            // Minimal POSIX server: bind → listen → accept → read line → write response → close
            let fd = socket(AF_UNIX, SOCK_STREAM, 0)
            guard fd >= 0 else { return }
            defer { Darwin.close(fd) }
            var addr = sockaddr_un()
            addr.sun_family = sa_family_t(AF_UNIX)
            withUnsafeMutablePointer(to: &addr.sun_path) { ptr in
                socketPath.withCString { cstr in
                    _ = Darwin.strcpy(UnsafeMutableRawPointer(ptr).assumingMemoryBound(to: CChar.self), cstr)
                }
            }
            let len = socklen_t(MemoryLayout<sockaddr_un>.size)
            guard Darwin.bind(fd, withUnsafePointer(to: addr) { UnsafeRawPointer($0).assumingMemoryBound(to: sockaddr.self) }, len) == 0 else { return }
            guard Darwin.listen(fd, 1) == 0 else { return }
            let clientFd = Darwin.accept(fd, nil, nil)
            guard clientFd >= 0 else { return }
            defer { Darwin.close(clientFd) }
            // Read until newline (discard request)
            var buf = [UInt8](repeating: 0, count: 4096)
            Darwin.read(clientFd, &buf, buf.count)
            // Write response
            let line = responseJSON + "\n"
            line.withCString { Darwin.write(clientFd, $0, strlen($0)) }
        }
    }

    func stop() {
        serverTask?.cancel()
        try? FileManager.default.removeItem(atPath: socketPath)
    }
}

@Suite("DaemonClient")
struct DaemonClientTests {

    @Test("call() returns parsed inner JSON value")
    func callReturnsInnerValue() async throws {
        let server = MockSocketServer()
        let inner = #"{"ok":true,"data":[]}"#
        // Escape inner for JSON string
        let escaped = inner.replacingOccurrences(of: "\\", with: "\\\\")
                           .replacingOccurrences(of: "\"", with: "\\\"")
        let outerJSON = #"{"id":"req-1","result":"\#(escaped)","error":null}"#
        await server.start(responseJSON: outerJSON)
        // Give the server a moment to start listening
        try await Task.sleep(for: .milliseconds(50))

        let client = DaemonClient(socketPath: await server.socketPath)
        let result = try await client.call(tool: "list_sessions", args: [:])
        #expect(result["ok"]?.boolValue == true)

        await server.stop()
    }

    @Test("call() throws .daemonStopped when socket file absent")
    func callThrowsWhenNoSocket() async {
        let client = DaemonClient(socketPath: "/tmp/nonexistent-\(UUID().uuidString).sock")
        do {
            _ = try await client.call(tool: "list_sessions", args: [:])
            Issue.record("Expected error but got success")
        } catch DaemonError.daemonStopped {
            // Expected
        } catch {
            Issue.record("Expected .daemonStopped but got \(error)")
        }
    }
}
