import Testing
@testable import PagerunnerCore
import Foundation

// MARK: - MockURLProtocol
//
// Swift 6 note: requestHandler is nonisolated(unsafe) because URLProtocol's
// startLoading() is called from URLSession's internal threads — outside any
// Swift actor. The tests must ensure they set requestHandler before calling
// probe() and that no two tests share the same MockURLProtocol subclass
// simultaneously. Each test creates a fresh subclass via makeMockSession(handler:).

// Per-test mock session factory: each call produces a unique URLProtocol subclass
// with its own handler closure, so concurrent tests never share state.
func makeMockSession(handler: @escaping @Sendable (URLRequest) throws -> (HTTPURLResponse, Data)) -> URLSession {
    // Dynamically create a unique subclass per invocation to avoid shared state
    // between concurrently-running tests.
    final class Holder: @unchecked Sendable {
        nonisolated(unsafe) var handler: (@Sendable (URLRequest) throws -> (HTTPURLResponse, Data))?
    }
    let holder = Holder()
    holder.handler = handler

    final class DynamicMock: URLProtocol {
        nonisolated(unsafe) static var holder: Holder?

        override class func canInit(with request: URLRequest) -> Bool { true }
        override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

        override func startLoading() {
            guard let h = DynamicMock.holder?.handler else {
                client?.urlProtocol(self, didFailWithError: URLError(.badURL))
                return
            }
            do {
                let (response, data) = try h(request)
                client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
                client?.urlProtocol(self, didLoad: data)
                client?.urlProtocolDidFinishLoading(self)
            } catch {
                client?.urlProtocol(self, didFailWithError: error)
            }
        }
        override func stopLoading() {}
    }

    DynamicMock.holder = holder
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [DynamicMock.self]
    return URLSession(configuration: config)
}

// MARK: - Tests

@Suite("DiscoveryService", .serialized)
struct DiscoveryServiceTests {

    @Test("found instance — valid Chrome on port 9300, 2 page tabs")
    func foundInstance() async throws {
        let versionJSON = #"{"Browser":"Chrome/120.0.0.0","Protocol-Version":"1.3"}"#
        let tabsJSON = #"""
        [
          {"type":"page","id":"1","url":"https://example.com","title":"Example"},
          {"type":"page","id":"2","url":"https://other.com","title":"Other"},
          {"type":"browser","id":"3","url":"","title":""}
        ]
        """#

        let session = makeMockSession { request in
            let url = request.url!.absoluteString
            // Only respond with Chrome data for port 9300; other ports return 404
            guard url.contains(":9300/") else {
                let response = HTTPURLResponse(
                    url: request.url!, statusCode: 404,
                    httpVersion: nil, headerFields: nil)!
                return (response, Data())
            }
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil, headerFields: nil)!
            if url.contains("/json/version") {
                return (response, versionJSON.data(using: .utf8)!)
            } else {
                return (response, tabsJSON.data(using: .utf8)!)
            }
        }

        let service = DiscoveryService(portRange: 9300...9302, urlSession: session)
        let instances = await service.probe()

        #expect(instances.count == 1)
        let inst = try #require(instances.first)
        #expect(inst.port == 9300)
        #expect(inst.tabCount == 2)
        #expect(inst.id == "port-9300")
        #expect(inst.isVM == false)
        #expect(inst.attachState == .idle)
    }

    @Test("empty result — all ports return non-200")
    func emptyResult() async {
        let session = makeMockSession { request in
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 503,
                httpVersion: nil, headerFields: nil)!
            return (response, Data())
        }

        let service = DiscoveryService(portRange: 9300...9302, urlSession: session)
        let instances = await service.probe()

        #expect(instances.isEmpty)
    }

    @Test("cache hit — second probe returns cached result without new HTTP calls")
    func cacheHit() async throws {
        actor CallCounter {
            var count = 0
            func increment() { count += 1 }
        }
        let counter = CallCounter()

        let versionJSON = #"{"Browser":"Chrome/120.0.0.0"}"#
        let tabsJSON = #"[{"type":"page","id":"1","url":"https://example.com","title":"Ex"}]"#

        let session = makeMockSession { request in
            let url = request.url!.absoluteString
            Task { await counter.increment() }
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil, headerFields: nil)!
            if url.contains("/json/version") {
                return (response, versionJSON.data(using: .utf8)!)
            } else {
                return (response, tabsJSON.data(using: .utf8)!)
            }
        }

        let service = DiscoveryService(portRange: 9300...9302, urlSession: session)

        // First probe — hits network
        let first = await service.probe()
        #expect(!first.isEmpty)

        // Give counter tasks time to complete
        try await Task.sleep(nanoseconds: 50_000_000)
        let countAfterFirst = await counter.count
        #expect(countAfterFirst > 0)

        // Second probe — should use cache, no additional HTTP calls
        let second = await service.probe()
        #expect(second.count == first.count)
        #expect(second.first?.port == first.first?.port)

        try await Task.sleep(nanoseconds: 50_000_000)
        let countAfterSecond = await counter.count

        // No new HTTP calls on second probe
        #expect(countAfterSecond == countAfterFirst)
    }

    @Test("cache miss after invalidate — second probe fires new HTTP calls")
    func cacheMissAfterInvalidate() async throws {
        actor CallCounter {
            var probeCount = 0
            func increment() { probeCount += 1 }
        }
        let counter = CallCounter()

        let versionJSON = #"{"Browser":"Chrome/120.0.0.0"}"#
        let tabsJSON = #"[{"type":"page","id":"1","url":"https://example.com","title":"Ex"}]"#

        let session = makeMockSession { request in
            let url = request.url!.absoluteString
            if url.contains("/json/version") {
                Task { await counter.increment() }
                let response = HTTPURLResponse(
                    url: request.url!, statusCode: 200,
                    httpVersion: nil, headerFields: nil)!
                return (response, versionJSON.data(using: .utf8)!)
            } else {
                let response = HTTPURLResponse(
                    url: request.url!, statusCode: 200,
                    httpVersion: nil, headerFields: nil)!
                return (response, tabsJSON.data(using: .utf8)!)
            }
        }

        let service = DiscoveryService(portRange: 9300...9302, urlSession: session)

        // First probe
        _ = await service.probe()
        try await Task.sleep(nanoseconds: 50_000_000)
        let countAfterFirst = await counter.probeCount
        #expect(countAfterFirst > 0)

        // Invalidate cache
        await service.invalidateCache()

        // Second probe — cache is invalid, must fire new HTTP calls
        _ = await service.probe()
        try await Task.sleep(nanoseconds: 50_000_000)
        let countAfterSecond = await counter.probeCount

        #expect(countAfterSecond > countAfterFirst)
    }

    @Test("malformed JSON silently skipped — probe returns empty")
    func malformedJSONSkipped() async {
        let session = makeMockSession { request in
            let response = HTTPURLResponse(
                url: request.url!, statusCode: 200,
                httpVersion: nil, headerFields: nil)!
            return (response, "not json".data(using: .utf8)!)
        }

        let service = DiscoveryService(portRange: 9300...9302, urlSession: session)
        let instances = await service.probe()

        #expect(instances.isEmpty)
    }
}
