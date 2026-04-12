import Foundation

// MARK: - WebSocketClient

/// Connects to the daemon's `/ws/events` WebSocket endpoint and dispatches
/// incoming events via callbacks.
///
/// Features:
/// - Automatic reconnection with exponential backoff
/// - Typed event dispatch (agent events, session status, notifications, tool results)
/// - Ability to send tool calls over the WebSocket channel
@MainActor
@Observable
public final class WebSocketClient {

    // MARK: Connection state

    public enum ConnectionState: String, Sendable {
        case disconnected
        case connecting
        case connected
    }

    public private(set) var state: ConnectionState = .disconnected

    // MARK: Callbacks

    /// Called when an agent event is received.
    public var onAgentEvent: (@Sendable (AgentEvent) -> Void)?

    /// Called when session status updates arrive (periodic push from daemon).
    public var onSessionStatus: (@Sendable ([Session]) -> Void)?

    /// Called when a notification is pushed.
    public var onNotification: (@Sendable (DaemonNotification) -> Void)?

    /// Called when a tool result arrives (response to a tool call sent over WS).
    public var onToolResult: (@Sendable (ToolCallResponse) -> Void)?

    // MARK: Configuration

    private let apiClient: APIClient
    private var autoReconnect: Bool = true

    // MARK: Internal state

    private var task: URLSessionWebSocketTask?
    private var urlSession: URLSession?
    private var receiveTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var backoffSeconds: Double = 1.0

    private static let maxBackoff: Double = 30.0
    private static let initialBackoff: Double = 1.0

    private let decoder: JSONDecoder = {
        let d = JSONDecoder()
        return d
    }()

    // MARK: Init

    public init(apiClient: APIClient) {
        self.apiClient = apiClient
    }

    // Note: No explicit deinit needed. When this object is deallocated,
    // the URLSessionWebSocketTask and Task values are released automatically.
    // Callers should invoke disconnect() before dropping their reference.

    // MARK: - Public API

    /// Connect to the daemon WebSocket endpoint.
    public func connect() {
        guard state == .disconnected else { return }
        autoReconnect = true
        performConnect()
    }

    /// Disconnect and stop auto-reconnection.
    public func disconnect() {
        autoReconnect = false
        receiveTask?.cancel()
        receiveTask = nil
        reconnectTask?.cancel()
        reconnectTask = nil
        task?.cancel(with: .normalClosure, reason: nil)
        task = nil
        state = .disconnected
    }

    /// Send a tool call request over the WebSocket.
    /// The result will arrive asynchronously via `onToolResult`.
    public func sendToolCall(_ tool: String, args: [String: Any] = [:]) async throws {
        guard let task, state == .connected else {
            throw PagerunnerError.websocketDisconnected
        }
        let payload: [String: Any] = [
            "tool": tool,
            "args": args,
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        guard let text = String(data: data, encoding: .utf8) else {
            throw PagerunnerError.decodingFailed("Failed to encode tool call as UTF-8")
        }
        try await task.send(.string(text))
    }

    // MARK: - Private

    private func performConnect() {
        state = .connecting

        let urlString = "\(apiClient.wsBaseURL)/ws/events"
        guard let url = URL(string: urlString) else {
            state = .disconnected
            return
        }

        var request = URLRequest(url: url)
        request.setValue("Bearer \(apiClient.token)", forHTTPHeaderField: "Authorization")

        let session = URLSession(configuration: .default)
        self.urlSession = session
        let wsTask = session.webSocketTask(with: request)
        self.task = wsTask
        wsTask.resume()

        state = .connected
        backoffSeconds = Self.initialBackoff

        receiveTask?.cancel()
        receiveTask = Task { [weak self] in
            await self?.receiveLoop()
        }
    }

    private func receiveLoop() async {
        guard let task else { return }

        while !Task.isCancelled {
            do {
                let message = try await task.receive()
                handleMessage(message)
            } catch {
                // WebSocket closed or errored
                break
            }
        }

        // If we get here, the connection dropped
        handleDisconnect()
    }

    @MainActor
    private func handleMessage(_ message: URLSessionWebSocketTask.Message) {
        let data: Data
        switch message {
        case .string(let text):
            guard let d = text.data(using: .utf8) else { return }
            data = d
        case .data(let d):
            data = d
        @unknown default:
            return
        }

        // Decode the outer message to determine the event type
        guard let wsMessage = try? decoder.decode(WSMessage.self, from: data) else {
            return
        }

        switch wsMessage.type {
        case .agentEvent:
            if let eventData = wsMessage.data,
               let reEncoded = try? JSONEncoder().encode(eventData),
               let agentEvent = try? decoder.decode(AgentEvent.self, from: reEncoded) {
                onAgentEvent?(agentEvent)
            }

        case .sessionStatus:
            if let statusData = wsMessage.data,
               let reEncoded = try? JSONEncoder().encode(statusData) {
                // The data field contains the full list_sessions result which
                // is wrapped in {"data": [...]}
                if let envelope = try? decoder.decode(DataEnvelope<[Session]>.self, from: reEncoded) {
                    onSessionStatus?(envelope.data)
                } else if let sessions = try? decoder.decode([Session].self, from: reEncoded) {
                    onSessionStatus?(sessions)
                }
            }

        case .notification:
            if let notifData = wsMessage.data,
               let reEncoded = try? JSONEncoder().encode(notifData),
               let notification = try? decoder.decode(DaemonNotification.self, from: reEncoded) {
                onNotification?(notification)
            }

        case .toolResult:
            let response = ToolCallResponse(
                ok: wsMessage.ok ?? false,
                result: wsMessage.result,
                error: wsMessage.error
            )
            onToolResult?(response)
        }
    }

    @MainActor
    private func handleDisconnect() {
        task = nil
        state = .disconnected

        guard autoReconnect else { return }

        let delay = backoffSeconds
        backoffSeconds = min(backoffSeconds * 2, Self.maxBackoff)

        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(delay))
            guard !Task.isCancelled else { return }
            self?.performConnect()
        }
    }
}
