import Foundation

public actor DiscoveryService {
    private var cache: [DiscoveredInstance] = []
    private var lastProbeAt: Date?
    private let cacheTTL: TimeInterval = 30
    private let portRange: ClosedRange<Int>
    private let urlSession: URLSession

    public init(portRange: ClosedRange<Int> = 9222...9239, urlSession: URLSession? = nil) {
        self.portRange = portRange
        if let session = urlSession {
            self.urlSession = session
        } else {
            let config = URLSessionConfiguration.ephemeral
            config.timeoutIntervalForRequest = 0.4
            self.urlSession = URLSession(configuration: config)
        }
    }

    public func probe() async -> [DiscoveredInstance] {
        // Cache hit: return if probed within TTL
        if let last = lastProbeAt, Date().timeIntervalSince(last) < cacheTTL {
            return cache
        }

        // Probe all ports concurrently
        let session = urlSession
        let ports = Array(portRange)

        let results: [DiscoveredInstance] = await withTaskGroup(of: DiscoveredInstance?.self) { group in
            for port in ports {
                group.addTask {
                    await Self.probePort(port, session: session)
                }
            }
            var found: [DiscoveredInstance] = []
            for await result in group {
                if let instance = result {
                    found.append(instance)
                }
            }
            return found.sorted { $0.port < $1.port }
        }

        cache = results
        lastProbeAt = Date()
        return results
    }

    public func invalidateCache() {
        lastProbeAt = nil
    }

    // MARK: - Private

    private static func probePort(_ port: Int, session: URLSession) async -> DiscoveredInstance? {
        // Step 1: GET /json/version — must return 200 with "Browser" key
        guard let versionURL = URL(string: "http://localhost:\(port)/json/version") else {
            return nil
        }
        do {
            let (versionData, versionResponse) = try await session.data(from: versionURL)
            guard let http = versionResponse as? HTTPURLResponse, http.statusCode == 200 else {
                return nil
            }
            guard let json = try? JSONSerialization.jsonObject(with: versionData) as? [String: Any],
                  json["Browser"] != nil else {
                return nil
            }
        } catch {
            return nil
        }

        // Step 2: GET /json — count page targets
        guard let tabsURL = URL(string: "http://localhost:\(port)/json") else {
            return nil
        }
        let tabCount: Int
        do {
            let (tabsData, tabsResponse) = try await session.data(from: tabsURL)
            guard let http = tabsResponse as? HTTPURLResponse, http.statusCode == 200 else {
                return nil
            }
            guard let targets = try? JSONSerialization.jsonObject(with: tabsData) as? [[String: Any]] else {
                return nil
            }
            tabCount = targets.filter { ($0["type"] as? String) == "page" }.count
        } catch {
            return nil
        }

        return DiscoveredInstance(
            id: "port-\(port)",
            port: port,
            tabCount: tabCount,
            isVM: false,
            attachState: .idle
        )
    }
}
