import Foundation

// MARK: - ConnectionManager

/// Manages connection state, APIClient/WebSocketClient lifecycle, and persistence
/// of connection settings.
///
/// This is the primary entry point for the iOS app to interact with the daemon.
/// It stores connection credentials in UserDefaults and exposes observable
/// connection state for SwiftUI views.
@MainActor
@Observable
public final class ConnectionManager {

    // MARK: Connection settings

    public var host: String
    public var port: Int
    public var token: String
    public var useTLS: Bool

    // MARK: Observable state

    public private(set) var isConnected: Bool = false
    public private(set) var lastError: String?
    public private(set) var daemonVersion: String?
    public private(set) var authMode: AuthMode = .token

    /// Probe the daemon for its auth mode. Useful before showing the token
    /// field — if the daemon uses Tailscale, no token is needed.
    public func probeAuthMode() async -> AuthMode? {
        let client = APIClient(host: host, port: port, token: "", useTLS: useTLS)
        do {
            let info = try await client.authInfo()
            authMode = info.mode
            return info.mode
        } catch {
            lastError = error.localizedDescription
            return nil
        }
    }

    // MARK: Clients

    public private(set) var apiClient: APIClient?
    public private(set) var wsClient: WebSocketClient?

    // MARK: UserDefaults keys

    private enum DefaultsKey {
        static let host = "pagerunner.connection.host"
        static let port = "pagerunner.connection.port"
        static let token = "pagerunner.connection.token"
        static let useTLS = "pagerunner.connection.useTLS"
    }

    // MARK: Init

    /// Create a new ConnectionManager. Call `loadSettings()` to restore persisted
    /// values, or pass explicit values for programmatic use.
    public init(
        host: String = "127.0.0.1",
        port: Int = 9876,
        token: String = "",
        useTLS: Bool = false
    ) {
        self.host = host
        self.port = port
        self.token = token
        self.useTLS = useTLS
    }

    // MARK: - Connection lifecycle

    /// Attempt to connect to the daemon using the current settings.
    /// Updates `isConnected`, `daemonVersion`, and `lastError`.
    public func connect() async {
        lastError = nil

        let client = APIClient(
            host: host,
            port: port,
            token: token,
            useTLS: useTLS
        )

        do {
            let healthResponse = try await client.health()
            daemonVersion = healthResponse.version
        } catch {
            lastError = error.localizedDescription
            isConnected = false
            apiClient = nil
            wsClient = nil
            return
        }

        // Verify auth works by calling an authenticated endpoint
        do {
            _ = try await client.listProfiles()
        } catch let error as PagerunnerError {
            if case .unauthorized = error {
                lastError = "Invalid bearer token"
                isConnected = false
                apiClient = nil
                wsClient = nil
                return
            }
            // Other errors are acceptable — daemon is reachable but maybe
            // profiles aren't configured yet.
        } catch {
            lastError = error.localizedDescription
            isConnected = false
            apiClient = nil
            wsClient = nil
            return
        }

        self.apiClient = client

        // Set up WebSocket client
        let ws = WebSocketClient(apiClient: client)
        self.wsClient = ws
        ws.connect()

        isConnected = true
        saveSettings()
    }

    /// Disconnect from the daemon.
    public func disconnect() {
        wsClient?.disconnect()
        wsClient = nil
        apiClient = nil
        isConnected = false
        daemonVersion = nil
        lastError = nil
    }

    /// Test connectivity without committing to a full connection.
    /// Returns `true` if the daemon is reachable and auth succeeds.
    public func testConnection() async -> Bool {
        let client = APIClient(
            host: host,
            port: port,
            token: token,
            useTLS: useTLS
        )

        do {
            _ = try await client.health()
            _ = try await client.listProfiles()
            return true
        } catch {
            lastError = error.localizedDescription
            return false
        }
    }

    // MARK: - Settings persistence

    /// Save current settings to UserDefaults.
    public func saveSettings() {
        let defaults = UserDefaults.standard
        defaults.set(host, forKey: DefaultsKey.host)
        defaults.set(port, forKey: DefaultsKey.port)
        defaults.set(token, forKey: DefaultsKey.token)
        defaults.set(useTLS, forKey: DefaultsKey.useTLS)
    }

    /// Load settings from UserDefaults. Returns silently if no settings are stored.
    public func loadSettings() {
        let defaults = UserDefaults.standard

        if let savedHost = defaults.string(forKey: DefaultsKey.host) {
            host = savedHost
        }
        let savedPort = defaults.integer(forKey: DefaultsKey.port)
        if savedPort > 0 {
            port = savedPort
        }
        if let savedToken = defaults.string(forKey: DefaultsKey.token) {
            token = savedToken
        }
        useTLS = defaults.bool(forKey: DefaultsKey.useTLS)
    }

    /// Clear persisted settings from UserDefaults.
    public func clearSettings() {
        let defaults = UserDefaults.standard
        defaults.removeObject(forKey: DefaultsKey.host)
        defaults.removeObject(forKey: DefaultsKey.port)
        defaults.removeObject(forKey: DefaultsKey.token)
        defaults.removeObject(forKey: DefaultsKey.useTLS)
    }
}
