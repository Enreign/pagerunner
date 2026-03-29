import Testing
@testable import PagerunnerCore

@Suite("URLHelpers")
struct URLHelpersTests {

    @Test("extracts scheme+host from https URL with path")
    func extractsOriginFromHTTPSWithPath() {
        #expect(originFrom(url: "https://linear.app/pagerunner/issues") == "https://linear.app")
    }

    @Test("extracts scheme+host from URL without path")
    func extractsOriginFromURLNoPath() {
        #expect(originFrom(url: "https://app.growthmate.io") == "https://app.growthmate.io")
    }

    @Test("handles http scheme with port")
    func extractsOriginHTTPWithPort() {
        #expect(originFrom(url: "http://localhost:3000/dashboard") == "http://localhost:3000")
    }

    @Test("returns nil for chrome:// URLs")
    func returnsNilForChromeURLs() {
        #expect(originFrom(url: "chrome://newtab") == nil)
    }

    @Test("returns nil for empty string")
    func returnsNilForEmptyString() {
        #expect(originFrom(url: "") == nil)
    }

    @Test("returns nil for malformed URL")
    func returnsNilForMalformed() {
        #expect(originFrom(url: "not a url") == nil)
    }
}
