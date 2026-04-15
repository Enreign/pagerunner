import Foundation

// MARK: - APIClient

/// HTTP client for the Pagerunner daemon REST API.
///
/// All methods are `async throws` and return decoded Swift types.
/// Uses `URLSession` with bearer token authentication.
///
/// Thread-safety: The class is `Sendable` via `nonisolated(unsafe)` for the
/// immutable configuration stored at init time, and the shared `URLSession` +
/// `JSONDecoder` which are themselves thread-safe.
public final class APIClient: Sendable {

    // MARK: Configuration

    public let host: String
    public let port: Int
    public let token: String
    public let useTLS: Bool

    // MARK: Internal plumbing

    private let session: URLSession
    private let decoder: JSONDecoder

    /// Base URL derived from configuration (e.g. `https://192.168.1.10:9876`).
    public var baseURL: String {
        let scheme = useTLS ? "https" : "http"
        return "\(scheme)://\(host):\(port)"
    }

    /// WebSocket base URL (e.g. `wss://192.168.1.10:9876`).
    public var wsBaseURL: String {
        let scheme = useTLS ? "wss" : "ws"
        return "\(scheme)://\(host):\(port)"
    }

    // MARK: Init

    public init(host: String, port: Int, token: String, useTLS: Bool = false) {
        self.host = host
        self.port = port
        self.token = token
        self.useTLS = useTLS

        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 30
        config.timeoutIntervalForResource = 60
        self.session = URLSession(configuration: config)

        let dec = JSONDecoder()
        // We use explicit CodingKeys with snake_case mapping in models,
        // so we do NOT set .convertFromSnakeCase globally. This avoids
        // double-conversion issues when models already define CodingKeys.
        self.decoder = dec
    }

    // MARK: - Public API

    /// Health check (no auth required by daemon).
    public func health() async throws -> HealthResponse {
        try await get("/health", authenticated: false)
    }

    /// Report which auth mode the daemon expects. Unauthenticated.
    public func authInfo() async throws -> AuthInfoResponse {
        try await get("/auth-info", authenticated: false)
    }

    /// List all configured profiles.
    public func listProfiles() async throws -> [Profile] {
        let envelope: DataEnvelope<[Profile]> = try await get("/api/profiles")
        return envelope.data
    }

    /// List active sessions.
    public func listSessions() async throws -> [Session] {
        let envelope: DataEnvelope<[Session]> = try await get("/api/sessions")
        return envelope.data
    }

    /// List tabs for a session.
    public func listTabs(sessionId: String) async throws -> [Tab] {
        let envelope: DataEnvelope<[Tab]> = try await get("/api/sessions/\(sessionId)/tabs")
        return envelope.data
    }

    /// Take a screenshot. Returns raw base64-encoded PNG (no data-URL prefix).
    public func screenshot(sessionId: String, targetId: String) async throws -> String {
        let raw: AnyCodableValue = try await post(
            "/api/sessions/\(sessionId)/screenshot/\(targetId)",
            body: EmptyBody()
        )
        // The daemon wraps the screenshot as either `{"data": "data:image/png;base64,…"}`
        // or `{"base64": "…"}` depending on how the tool serialised it. Handle both.
        for key in ["data", "base64", "image"] {
            if let s = raw[key]?.stringValue {
                return stripDataURL(s)
            }
        }
        if let s = raw.stringValue {
            return stripDataURL(s)
        }
        throw PagerunnerError.decodingFailed("Expected base64 screenshot data")
    }

    /// Strip the `data:image/png;base64,` prefix if present so callers can
    /// decode the body directly with `Data(base64Encoded:)`.
    private func stripDataURL(_ s: String) -> String {
        if let range = s.range(of: ";base64,") {
            return String(s[range.upperBound...])
        }
        return s
    }

    /// Fetch network log entries for a session.
    public func networkLog(
        sessionId: String,
        limit: Int? = nil,
        targetId: String? = nil
    ) async throws -> NetworkLogResult {
        var query: [String] = []
        if let limit { query.append("limit=\(limit)") }
        if let targetId { query.append("target_id=\(urlEncode(targetId))") }
        let queryString = query.isEmpty ? "" : "?\(query.joined(separator: "&"))"
        return try await get("/api/sessions/\(sessionId)/network-log\(queryString)")
    }

    /// Fetch console log entries for a session.
    public func consoleLog(
        sessionId: String,
        targetId: String? = nil
    ) async throws -> ConsoleLogResult {
        var query: [String] = []
        if let targetId { query.append("target_id=\(urlEncode(targetId))") }
        let queryString = query.isEmpty ? "" : "?\(query.joined(separator: "&"))"
        return try await get("/api/sessions/\(sessionId)/console-log\(queryString)")
    }

    /// Drain pending notifications.
    public func notifications() async throws -> [DaemonNotification] {
        let envelope: NotificationsEnvelope = try await get("/api/notifications")
        return envelope.notifications
    }

    /// List checkpoints for a profile.
    public func checkpoints(profile: String) async throws -> [Checkpoint] {
        let envelope: DataEnvelope<[Checkpoint]> = try await get(
            "/api/checkpoints/\(urlEncode(profile))"
        )
        return envelope.data
    }

    /// List recordings.
    public func recordings() async throws -> [Recording] {
        let envelope: DataEnvelope<[Recording]> = try await get("/api/recordings")
        return envelope.data
    }

    // MARK: - Generic tool call

    /// Execute any tool through the generic `/api/tool` endpoint.
    public func callTool(_ tool: String, args: [String: Any] = [:]) async throws -> ToolCallResponse {
        let body = ToolCallRequest(tool: tool, args: AnyCodableValue(args as Any))
        return try await post("/api/tool", body: body)
    }

    /// Execute any tool with a pre-built `AnyCodableValue` args payload.
    /// Prefer this overload when building args from `@MainActor` context
    /// to avoid Sendable violations with `[String: Any]`.
    public func callTool(_ tool: String, codableArgs: AnyCodableValue) async throws -> ToolCallResponse {
        let body = ToolCallRequest(tool: tool, args: codableArgs)
        return try await post("/api/tool", body: body)
    }

    // MARK: - Convenience action methods

    /// Open a new session on the given profile.
    public func openSession(profile: String, stealth: Bool = false) async throws -> ToolCallResponse {
        try await callTool("open_session", args: [
            "profile": profile,
            "stealth": stealth,
        ])
    }

    /// Close a session.
    public func closeSession(sessionId: String) async throws -> ToolCallResponse {
        try await callTool("close_session", args: ["session_id": sessionId])
    }

    /// Open a new tab in a session.
    public func newTab(sessionId: String, url: String? = nil) async throws -> ToolCallResponse {
        var args: [String: Any] = ["session_id": sessionId]
        if let url { args["url"] = url }
        return try await callTool("new_tab", args: args)
    }

    /// Close a tab.
    public func closeTab(sessionId: String, targetId: String) async throws -> ToolCallResponse {
        try await callTool("close_tab", args: [
            "session_id": sessionId,
            "target_id": targetId,
        ])
    }

    /// Navigate a tab to a URL.
    public func navigate(
        sessionId: String,
        targetId: String,
        url: String
    ) async throws -> ToolCallResponse {
        try await callTool("navigate", args: [
            "session_id": sessionId,
            "target_id": targetId,
            "url": url,
        ])
    }

    /// Save a session checkpoint.
    public func saveCheckpoint(
        sessionId: String,
        name: String? = nil
    ) async throws -> ToolCallResponse {
        var args: [String: Any] = ["session_id": sessionId]
        if let name { args["name"] = name }
        return try await callTool("save_session_checkpoint", args: args)
    }

    /// Restore a session checkpoint.
    public func restoreCheckpoint(
        sessionId: String,
        checkpointId: String
    ) async throws -> ToolCallResponse {
        try await callTool("restore_session_checkpoint", args: [
            "session_id": sessionId,
            "checkpoint_id": checkpointId,
        ])
    }

    // MARK: - Private helpers

    private func get<T: Decodable>(
        _ path: String,
        authenticated: Bool = true
    ) async throws -> T {
        guard let url = URL(string: "\(baseURL)\(path)") else {
            throw PagerunnerError.invalidURL("\(baseURL)\(path)")
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        if authenticated {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        return try await perform(request)
    }

    private func post<T: Decodable>(
        _ path: String,
        body: some Encodable,
        authenticated: Bool = true
    ) async throws -> T {
        guard let url = URL(string: "\(baseURL)\(path)") else {
            throw PagerunnerError.invalidURL("\(baseURL)\(path)")
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if authenticated {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        let encoder = JSONEncoder()
        request.httpBody = try encoder.encode(body)
        return try await perform(request)
    }

    private func perform<T: Decodable>(_ request: URLRequest) async throws -> T {
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw PagerunnerError.connectionFailed(error.localizedDescription)
        }

        guard let httpResponse = response as? HTTPURLResponse else {
            throw PagerunnerError.connectionFailed("Non-HTTP response received")
        }

        if httpResponse.statusCode == 401 {
            throw PagerunnerError.unauthorized
        }

        if httpResponse.statusCode >= 400 {
            // Try to extract error message from response body
            let message: String
            if let errorBody = try? decoder.decode(ErrorEnvelope.self, from: data) {
                message = errorBody.error
            } else if let text = String(data: data, encoding: .utf8) {
                message = text
            } else {
                message = "Unknown error"
            }
            throw PagerunnerError.requestFailed(
                statusCode: httpResponse.statusCode,
                message: message
            )
        }

        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            throw PagerunnerError.decodingFailed(
                "\(error.localizedDescription) — body: \(String(data: data.prefix(500), encoding: .utf8) ?? "<binary>")"
            )
        }
    }

    private func urlEncode(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? value
    }
}

// MARK: - Internal request/response types

private struct ToolCallRequest: Encodable {
    let tool: String
    let args: AnyCodableValue
}

private struct EmptyBody: Encodable {}

private struct ErrorEnvelope: Decodable {
    let error: String
}
