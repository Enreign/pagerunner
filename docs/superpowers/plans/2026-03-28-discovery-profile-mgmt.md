# Auto-Discovery + Profile Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add rename/remove for existing profiles + auto-discovery of unmanaged Chrome instances with one-click attach, inline in the macOS menu bar panel.

**Architecture:** Two parallel tracks — (1) Rust `src/config.rs` gains optional `user_data_dir` and `debug_port` to support "attached" profile kind; (2) Swift app gains `ConfigEditor` (TOML file editing), `DiscoveryService` (async port probe), and updated views. AppState tracks `remoteSessions` (Set<String>) as client-side state. Discovery runs on panel open, results cached 30s, rendered inline below profile list at 70% opacity.

**Tech Stack:** Rust (serde/toml), Swift 6, SwiftUI, macOS 14+, `@Observable`, `URLSession`, `withTaskGroup`, `NSAlert`-style sheets.

**Spec:** `docs/superpowers/specs/2026-03-28-discovery-profile-mgmt-design.md`

---

## File Map

| File | Status | Responsibility |
|------|--------|----------------|
| `src/config.rs` | Modify | Make `user_data_dir` optional, add `debug_port` |
| `src/mcp_server.rs` | Modify | Route `open_session` for `kind="attached"` profiles |
| `apps/menubar/Sources/PagerunnerCore/ConfigEditor.swift` | Create | Read/write `~/.pagerunner/config.toml` (rename, remove, addAttached) — in Core (no UI deps, testable from PagerunnerCoreTests) |
| `apps/menubar/Sources/PagerunnerCore/Models.swift` | Modify | Add `DiscoveredInstance`, `AttachState`; `Profile` gains `debugPort` |
| `apps/menubar/Sources/PagerunnerCore/DiscoveryService.swift` | Create | Async port probe 9222–9239, gvproxy detection, 30s cache |
| `apps/menubar/Sources/PagerunnerBar/Views/RenameSheet.swift` | Create | Reusable sheet with text input + OK/Cancel |
| `apps/menubar/Sources/PagerunnerBar/Views/ProfileRowView.swift` | Create | Extract `ProfileRow` from OverviewView, add right-click Rename/Remove |
| `apps/menubar/Sources/PagerunnerBar/AppState.swift` | Modify | Add `discoveredInstances`, `remoteSessions`, panel-open trigger |
| `apps/menubar/Sources/PagerunnerBar/Views/OverviewView.swift` | Modify | Render discovered rows below profile list, hairline separator |
| `apps/menubar/Sources/PagerunnerBar/Views/SettingsView.swift` | Modify | Add Profiles section with Rename/Remove per row |
| `apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift` | Modify | Hide "Focus tab" context menu item for remote sessions |
| `apps/menubar/Tests/PagerunnerCoreTests/ConfigEditorTests.swift` | Create | Unit tests for ConfigEditor (temp file, never real config) |
| `apps/menubar/Tests/PagerunnerCoreTests/DiscoveryServiceTests.swift` | Create | Unit tests for DiscoveryService with mock URLSession |

---

## Task 1: Rust — Make `ChromeProfile` support attached profiles

**Context:** `src/config.rs` has `user_data_dir: String` (required). Profiles with `kind = "attached"` have no `user_data_dir`, only `debug_port`. This must be backward-compatible — existing profiles still parse correctly with `user_data_dir: Option<String>`.

**Files:**
- Modify: `src/config.rs`
- Modify: `src/mcp_server.rs` (open_session handler)

- [ ] **Step 1: Write the failing test in `src/config.rs`**

Add to the `#[cfg(test)]` block at the bottom of `src/config.rs`:

```rust
#[test]
fn test_attached_profile_parses_without_user_data_dir() {
    let toml = r#"
[[profiles]]
name = "chrome-9225"
display_name = "Chrome :9225"
kind = "attached"
debug_port = 9225
"#;
    let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.profiles[0].name, "chrome-9225");
    assert_eq!(cfg.profiles[0].kind.as_deref(), Some("attached"));
    assert_eq!(cfg.profiles[0].debug_port, Some(9225u16));
    assert!(cfg.profiles[0].user_data_dir.is_none());
}

#[test]
fn test_existing_profile_still_parses_with_user_data_dir() {
    let toml = r#"
[[profiles]]
name = "personal"
display_name = "Personal"
user_data_dir = "/tmp/chrome"
"#;
    let cfg: PagerunnerConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.profiles[0].user_data_dir.as_deref(), Some("/tmp/chrome"));
    assert!(cfg.profiles[0].debug_port.is_none());
}
```

- [ ] **Step 2: Run test to confirm it fails**

```bash
cargo test test_attached_profile_parses_without_user_data_dir
```

Expected: FAIL — `error[E0560]` or deserialize error because `user_data_dir` is required.

- [ ] **Step 3: Change `ChromeProfile` struct**

In `src/config.rs`, replace the struct:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChromeProfile {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub user_data_dir: Option<String>,
    #[serde(default)]
    pub debug_port: Option<u16>,
    #[serde(default)]
    pub kind: Option<String>,
}
```

- [ ] **Step 4: Fix all compilation errors from `user_data_dir` now being `Option<String>`**

Search for `.user_data_dir` across the codebase:

```bash
grep -rn "\.user_data_dir" src/
```

For each usage that assumed `String`, add `.as_deref().unwrap_or("")` or a match. The primary usage is in `open_session` in `src/mcp_server.rs` where it launches Chrome. Wrap in:

```rust
let user_data_dir = profile.user_data_dir.as_deref()
    .ok_or_else(|| PagerunnerError::Config("Profile has no user_data_dir".into()))?;
```

- [ ] **Step 5: Add open_session routing for `kind = "attached"` in `src/mcp_server.rs`**

Find the `open_session` handler in `src/mcp_server.rs`. It calls `browser::launch_chrome` or similar. Before that, add:

```rust
// Attached profiles connect to an already-running Chrome via debug port
if profile.kind.as_deref() == Some("attached") {
    let port = profile.debug_port
        .ok_or_else(|| PagerunnerError::Config("Attached profile missing debug_port".into()))?;
    // Reuse attach_session logic with the profile's debug_port
    let args_with_port = serde_json::json!({
        "debug_port": port,
        "profile": profile.name
    });
    return self.attach_session(args_with_port, caller).await;
}
```

The exact call depends on how `attach_session` is implemented in `mcp_server.rs` — check how it's currently called and mirror the same pattern.

- [ ] **Step 6: Run tests**

```bash
cargo test config
cargo build
```

Expected: all config tests pass, compilation succeeds.

- [ ] **Step 7: Commit**

```bash
git add src/config.rs src/mcp_server.rs
git commit -m "feat(config): make user_data_dir optional, add debug_port for attached profiles"
```

---

## Task 2: ConfigEditor — TOML file editing for rename/remove/addAttached

**Context:** `AddProfileView.swift` already has `appendProfileToConfig` and `restartDaemon` as private methods. We extract the config-writing logic into a new public `ConfigEditor` struct in `PagerunnerCore` (no AppKit/SwiftUI deps — Foundation only, cleanly testable from `PagerunnerCoreTests` via `@testable import PagerunnerCore`). Tests use temp files — never `~/.pagerunner/config.toml`.

**Files:**
- Create: `apps/menubar/Sources/PagerunnerCore/ConfigEditor.swift`
- Create: `apps/menubar/Tests/PagerunnerCoreTests/ConfigEditorTests.swift`

- [ ] **Step 1: Write the failing tests**

Create `apps/menubar/Tests/PagerunnerCoreTests/ConfigEditorTests.swift`:

```swift
import Foundation
import Testing
@testable import PagerunnerCore

@Suite("ConfigEditor")
struct ConfigEditorTests {

    private func makeTempConfig(_ content: String) throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("test-config-\(UUID().uuidString).toml")
        try content.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    @Test("renameProfile updates display_name, leaves other profiles untouched")
    func renameProfile() throws {
        let initial = """
        [[profiles]]
        name = "personal"
        display_name = "Old Name"
        user_data_dir = "/tmp/a"

        [[profiles]]
        name = "agent"
        display_name = "My Agent"
        user_data_dir = "/tmp/b"
        kind = "agent"
        """
        let url = try makeTempConfig(initial)
        try ConfigEditor.renameProfile(name: "personal", newDisplayName: "New Name", configURL: url)
        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(result.contains(#"display_name = "New Name""#))
        #expect(result.contains(#"display_name = "My Agent""#))
        #expect(!result.contains(#"display_name = "Old Name""#))
    }

    @Test("renameProfile throws profileNotFound for unknown name")
    func renameProfileNotFound() throws {
        let url = try makeTempConfig("""
        [[profiles]]
        name = "personal"
        display_name = "Stas"
        user_data_dir = "/tmp/a"
        """)
        #expect(throws: ConfigEditor.Error.profileNotFound("ghost")) {
            try ConfigEditor.renameProfile(name: "ghost", newDisplayName: "X", configURL: url)
        }
    }

    @Test("removeProfile removes correct block, leaves others")
    func removeProfile() throws {
        let initial = """
        [[profiles]]
        name = "personal"
        display_name = "Stas"
        user_data_dir = "/tmp/a"

        [[profiles]]
        name = "work"
        display_name = "Work"
        user_data_dir = "/tmp/b"
        """
        let url = try makeTempConfig(initial)
        try ConfigEditor.removeProfile(name: "personal", configURL: url)
        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(!result.contains(#"name = "personal""#))
        #expect(result.contains(#"name = "work""#))
    }

    @Test("removeProfile throws profileNotFound for unknown name")
    func removeProfileNotFound() throws {
        let url = try makeTempConfig("[[profiles]]\nname = \"a\"\ndisplay_name = \"A\"\nuser_data_dir = \"/tmp/a\"\n")
        #expect(throws: ConfigEditor.Error.profileNotFound("ghost")) {
            try ConfigEditor.removeProfile(name: "ghost", configURL: url)
        }
    }

    @Test("addAttachedProfile appends block with kind=attached and debug_port")
    func addAttachedProfile() throws {
        let url = try makeTempConfig("[[profiles]]\nname = \"personal\"\ndisplay_name = \"P\"\nuser_data_dir = \"/tmp/a\"\n")
        try ConfigEditor.addAttachedProfile(name: "chrome-9225", displayName: "Chrome :9225", port: 9225, configURL: url)
        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(result.contains(#"name = "chrome-9225""#))
        #expect(result.contains(#"kind = "attached""#))
        #expect(result.contains("debug_port = 9225"))
        // original profile still present
        #expect(result.contains(#"name = "personal""#))
    }

    @Test("addAttachedProfile creates file if missing")
    func addAttachedProfileCreatesFile() throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("nonexistent-\(UUID().uuidString).toml")
        try ConfigEditor.addAttachedProfile(name: "chrome-9222", displayName: "Chrome :9222", port: 9222, configURL: url)
        let result = try String(contentsOf: url, encoding: .utf8)
        #expect(result.contains("debug_port = 9222"))
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd apps/menubar && swift test --filter ConfigEditorTests 2>&1 | head -30
```

Expected: compile error — `ConfigEditor` not found.

- [ ] **Step 3: Create `ConfigEditor.swift`**

Create `apps/menubar/Sources/PagerunnerCore/ConfigEditor.swift`:

```swift
import Foundation

public struct ConfigEditor {

    public enum Error: Swift.Error, Equatable {
        case profileNotFound(String)
    }

    // MARK: - Public API

    public static func renameProfile(name: String, newDisplayName: String, configURL: URL = .pagerunnerConfig) throws {
        let content = try String(contentsOf: configURL, encoding: .utf8)
        var (preamble, blocks) = splitBlocks(content)
        guard let idx = blocks.firstIndex(where: { blockMatchesName($0, name: name) }) else {
            throw Error.profileNotFound(name)
        }
        blocks[idx] = replaceLine(in: blocks[idx], key: "display_name", newValue: newDisplayName)
        try writeAtomically(preamble + blocks.joined(), to: configURL)
    }

    public static func removeProfile(name: String, configURL: URL = .pagerunnerConfig) throws {
        let content = try String(contentsOf: configURL, encoding: .utf8)
        var (preamble, blocks) = splitBlocks(content)
        let before = blocks.count
        blocks.removeAll { blockMatchesName($0, name: name) }
        guard blocks.count < before else { throw Error.profileNotFound(name) }
        try writeAtomically(preamble + blocks.joined(), to: configURL)
    }

    public static func addAttachedProfile(name: String, displayName: String, port: Int, configURL: URL = .pagerunnerConfig) throws {
        if !FileManager.default.fileExists(atPath: configURL.path) {
            try FileManager.default.createDirectory(
                at: configURL.deletingLastPathComponent(),
                withIntermediateDirectories: true)
            try "".write(to: configURL, atomically: true, encoding: .utf8)
        }
        var existing = try String(contentsOf: configURL, encoding: .utf8)
        if !existing.isEmpty && !existing.hasSuffix("\n") { existing += "\n" }
        let block = """

        [[profiles]]
        name = "\(name)"
        display_name = "\(displayName)"
        kind = "attached"
        debug_port = \(port)
        """
        try writeAtomically(existing + block, to: configURL)
    }

    // MARK: - Block parsing internals (internal for tests)

    static func splitBlocks(_ content: String) -> (String, [String]) {
        // Normalize: ensure [[profiles]] always appears after a newline for uniform splitting
        var normalized = content
        if normalized.hasPrefix("[[profiles]]") { normalized = "\n" + normalized }
        let parts = normalized.components(separatedBy: "\n[[profiles]]")
        let preamble = parts[0]
        let blocks = parts.dropFirst().map { "\n[[profiles]]" + $0 }
        return (preamble, Array(blocks))
    }

    static func blockMatchesName(_ block: String, name: String) -> Bool {
        block.components(separatedBy: "\n").contains { line in
            line.trimmingCharacters(in: .whitespaces) == "name = \"\(name)\""
        }
    }

    private static func replaceLine(in block: String, key: String, newValue: String) -> String {
        block.components(separatedBy: "\n").map { line in
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("\(key) =") || trimmed.hasPrefix("\(key)=") {
                return "\(key) = \"\(newValue)\""
            }
            return line
        }.joined(separator: "\n")
    }

    private static func writeAtomically(_ content: String, to url: URL) throws {
        let tmp = url.deletingLastPathComponent()
            .appendingPathComponent("." + url.lastPathComponent + ".tmp")
        try content.write(to: tmp, atomically: false, encoding: .utf8)
        _ = try FileManager.default.replaceItemAt(url, withItemAt: tmp)
    }
}

public extension URL {
    static let pagerunnerConfig = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent(".pagerunner/config.toml")
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cd apps/menubar && swift test --filter ConfigEditorTests
```

Expected: 5/5 PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/menubar/Sources/PagerunnerCore/ConfigEditor.swift \
        apps/menubar/Tests/PagerunnerCoreTests/ConfigEditorTests.swift
git commit -m "feat: add ConfigEditor for TOML profile rename/remove/addAttached"
```

---

## Task 3: Models — Add `DiscoveredInstance`, `AttachState`, `Profile.debugPort`

**Context:** `Models.swift` in `PagerunnerCore` holds wire types. We add `DiscoveredInstance` + `AttachState` here (pure data, no UI). `Profile` already has `userDataDir: String?` — we add `debugPort: Int?` to decode port-based profiles. `Session` doesn't change — `remoteSessions` tracking lives in `AppState` as `Set<String>`.

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerCore/Models.swift`
- Modify: `apps/menubar/Tests/PagerunnerCoreTests/ModelsTests.swift`

- [ ] **Step 1: Write the failing tests**

Add to the `ModelsTests.swift` file:

```swift
@Test("Profile decodes debug_port for attached kind")
func profileDecodesDebugPort() throws {
    let json = """
    {"ok":true,"data":[
        {"name":"chrome-9225","display_name":"Chrome :9225","kind":"attached","debug_port":9225}
    ]}
    """
    let resp = try JSONDecoder().decode(ListProfilesResponse.self, from: Data(json.utf8))
    #expect(resp.data[0].kind == "attached")
    #expect(resp.data[0].debugPort == 9225)
}

@Test("AttachState transitions")
func attachStateTransitions() {
    var state = AttachState.idle
    #expect(state == .idle)
    state = .attaching
    #expect(state == .attaching)
    state = .attached
    #expect(state == .attached)
    state = .failed("timeout")
    if case .failed(let msg) = state {
        #expect(msg == "timeout")
    } else {
        Issue.record("Expected .failed")
    }
}

@Test("DiscoveredInstance has correct id format")
func discoveredInstanceId() {
    let instance = DiscoveredInstance(id: "port-9225", port: 9225, tabCount: 3, isVM: false, attachState: .idle)
    #expect(instance.id == "port-9225")
    #expect(instance.port == 9225)
    #expect(!instance.isVM)
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd apps/menubar && swift test --filter ModelsTests 2>&1 | head -20
```

Expected: compile errors — `debugPort`, `AttachState`, `DiscoveredInstance` not found.

- [ ] **Step 3: Update `Models.swift`**

In `apps/menubar/Sources/PagerunnerCore/Models.swift`:

1. Add `debugPort` to `Profile`:

```swift
public struct Profile: Codable, Identifiable, Sendable {
    public var id: String { name }
    public let name: String
    public let displayName: String
    public let kind: String
    public let userDataDir: String?
    public let debugPort: Int?

    enum CodingKeys: String, CodingKey {
        case name, kind
        case displayName = "display_name"
        case userDataDir = "user_data_dir"
        case debugPort = "debug_port"
    }
}
```

2. Add `AttachState` enum and `DiscoveredInstance` struct after the `Checkpoint` struct:

```swift
public enum AttachState: Equatable, Sendable {
    case idle
    case attaching
    case attached
    case failed(String)
}

public struct DiscoveredInstance: Identifiable, Sendable {
    public let id: String         // "port-9225"
    public let port: Int
    public let tabCount: Int
    public let isVM: Bool         // true if owning process is gvproxy
    public var attachState: AttachState

    public init(id: String, port: Int, tabCount: Int, isVM: Bool, attachState: AttachState) {
        self.id = id
        self.port = port
        self.tabCount = tabCount
        self.isVM = isVM
        self.attachState = attachState
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cd apps/menubar && swift test --filter ModelsTests
```

Expected: all pass (including the 5 existing tests + 3 new ones).

- [ ] **Step 5: Commit**

```bash
git add apps/menubar/Sources/PagerunnerCore/Models.swift \
        apps/menubar/Tests/PagerunnerCoreTests/ModelsTests.swift
git commit -m "feat: add DiscoveredInstance, AttachState models; Profile gains debugPort"
```

---

## Task 4: DiscoveryService — Port probe with cache and gvproxy detection

**Context:** Pure async actor in `PagerunnerCore`. Probes ports 9222–9239 concurrently with 400ms timeout. Returns `[DiscoveredInstance]`. Tests use a mock `URLSession` via `URLProtocol`.

**Files:**
- Create: `apps/menubar/Sources/PagerunnerCore/DiscoveryService.swift`
- Create: `apps/menubar/Tests/PagerunnerCoreTests/DiscoveryServiceTests.swift`

- [ ] **Step 1: Write the failing tests**

Create `apps/menubar/Tests/PagerunnerCoreTests/DiscoveryServiceTests.swift`:

```swift
import Foundation
import Testing
@testable import PagerunnerCore

// MARK: - Mock URLProtocol

final class MockURLProtocol: URLProtocol, @unchecked Sendable {
    nonisolated(unsafe) static var handlers: [URL: (Data, HTTPURLResponse)] = [:]
    nonisolated(unsafe) static var shouldTimeout: Set<URL> = []

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url else {
            client?.urlProtocolDidFinishLoading(self)
            return
        }
        if MockURLProtocol.shouldTimeout.contains(url) {
            let error = URLError(.timedOut)
            client?.urlProtocol(self, didFailWithError: error)
            return
        }
        if let (data, response) = MockURLProtocol.handlers[url] {
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } else {
            client?.urlProtocol(self, didFailWithError: URLError(.connectionRefused))
        }
    }
    override func stopLoading() {}
}

private func mockSession() -> URLSession {
    let config = URLSessionConfiguration.ephemeral
    config.protocolClasses = [MockURLProtocol.self]
    return URLSession(configuration: config)
}

private func makeResponse(port: Int, path: String, status: Int = 200) -> HTTPURLResponse {
    HTTPURLResponse(url: URL(string: "http://127.0.0.1:\(port)\(path)")!,
                    statusCode: status, httpVersion: nil, headerFields: nil)!
}

@Suite("DiscoveryService")
struct DiscoveryServiceTests {

    @Test("returns instance when port responds with valid JSON")
    func probeFindsInstance() async throws {
        // Use port outside 9222-9239 probe range to avoid interference from real Chrome
        let port = 9300
        let versionURL = URL(string: "http://127.0.0.1:\(port)/json/version")!
        let tabsURL    = URL(string: "http://127.0.0.1:\(port)/json")!
        MockURLProtocol.handlers[versionURL] = (
            #"{"Browser":"Chrome/120"}"#.data(using: .utf8)!,
            makeResponse(port: port, path: "/json/version")
        )
        MockURLProtocol.handlers[tabsURL] = (
            #"[{"id":"1"},{"id":"2"}]"#.data(using: .utf8)!,
            makeResponse(port: port, path: "/json")
        )
        defer {
            MockURLProtocol.handlers.removeValue(forKey: versionURL)
            MockURLProtocol.handlers.removeValue(forKey: tabsURL)
        }

        // Inject portRange to probe only port 9300 (avoids probing real Chrome ports 9222-9239)
        let svc = DiscoveryService(portRange: port...port)
        let results = await svc.probe(session: mockSession())
        let found = results.first { $0.port == port }
        #expect(found != nil)
        #expect(found?.tabCount == 2)
        #expect(found?.isVM == false)
    }

    @Test("returns empty when no ports respond")
    func probeEmpty() async {
        let svc = DiscoveryService(portRange: 9300...9300)
        // No handlers registered — port 9300 fails with connectionRefused
        let results = await svc.probe(session: mockSession())
        #expect(results.isEmpty)
    }

    @Test("cache returns same result within TTL")
    func cacheHit() async {
        let port = 9301
        let versionURL = URL(string: "http://127.0.0.1:\(port)/json/version")!
        let tabsURL    = URL(string: "http://127.0.0.1:\(port)/json")!
        MockURLProtocol.handlers[versionURL] = (
            #"{"Browser":"Chrome"}"#.data(using: .utf8)!,
            makeResponse(port: port, path: "/json/version")
        )
        MockURLProtocol.handlers[tabsURL] = (
            #"[{"id":"1"}]"#.data(using: .utf8)!,
            makeResponse(port: port, path: "/json")
        )
        defer {
            MockURLProtocol.handlers.removeValue(forKey: versionURL)
            MockURLProtocol.handlers.removeValue(forKey: tabsURL)
        }

        let svc = DiscoveryService(portRange: port...port)
        let first = await svc.probe(session: mockSession())
        // Remove handler — second probe should use cache
        MockURLProtocol.handlers.removeValue(forKey: versionURL)
        let second = await svc.probe(session: mockSession())
        #expect(first.count == second.count)
    }

    @Test("invalidateCache forces re-probe")
    func cacheInvalidation() async {
        let svc = DiscoveryService(portRange: 9300...9300)
        _ = await svc.probe(session: mockSession())  // prime cache (empty)
        await svc.invalidateCache()
        // After invalidation, probe runs again — still empty but re-ran
        let results = await svc.probe(session: mockSession())
        #expect(results.isEmpty)  // just confirms it ran without crashing
    }

    @Test("non-200 response is skipped")
    func skipNon200() async {
        let port = 9302
        let versionURL = URL(string: "http://127.0.0.1:\(port)/json/version")!
        MockURLProtocol.handlers[versionURL] = (
            Data(),
            makeResponse(port: port, path: "/json/version", status: 503)
        )
        defer { MockURLProtocol.handlers.removeValue(forKey: versionURL) }

        let svc = DiscoveryService(portRange: port...port)
        let results = await svc.probe(session: mockSession())
        #expect(results.first { $0.port == port } == nil)
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd apps/menubar && swift test --filter DiscoveryServiceTests 2>&1 | head -20
```

Expected: compile error — `DiscoveryService` not found.

- [ ] **Step 3: Create `DiscoveryService.swift`**

Create `apps/menubar/Sources/PagerunnerCore/DiscoveryService.swift`:

```swift
import Foundation

public actor DiscoveryService {
    private var cache: [DiscoveredInstance] = []
    private var lastProbeAt: Date?
    private let cacheTTL: TimeInterval = 30
    // portRange is injectable for testing (tests use ports outside 9222-9239)
    private let portRange: ClosedRange<Int>

    public init(portRange: ClosedRange<Int> = 9222...9239) {
        self.portRange = portRange
    }

    public func probe(session: URLSession = .discoverySession) async -> [DiscoveredInstance] {
        if let last = lastProbeAt, Date().timeIntervalSince(last) < cacheTTL {
            return cache
        }
        let instances = await withTaskGroup(of: DiscoveredInstance?.self) { group in
            for port in portRange {
                group.addTask { await Self.probePort(port, session: session) }
            }
            var results: [DiscoveredInstance] = []
            for await result in group {
                if let r = result { results.append(r) }
            }
            return results.sorted { $0.port < $1.port }
        }
        cache = instances
        lastProbeAt = Date()
        return instances
    }

    public func invalidateCache() {
        lastProbeAt = nil
        cache = []
    }

    // MARK: - Per-port probe

    private static func probePort(_ port: Int, session: URLSession) async -> DiscoveredInstance? {
        // Step 1: check /json/version
        guard let versionURL = URL(string: "http://127.0.0.1:\(port)/json/version"),
              let (_, versionResp) = try? await session.data(from: versionURL),
              let httpResp = versionResp as? HTTPURLResponse,
              httpResp.statusCode == 200
        else { return nil }

        // Step 2: count open tabs via /json
        let tabCount = await fetchTabCount(port: port, session: session) ?? 0

        // Step 3: detect gvproxy — synchronous lsof, offloaded to background thread
        // to avoid blocking the cooperative thread pool (Swift 6 requirement).
        let isVM = await Task(priority: .background) { detectGvproxy(port: port) }.value

        return DiscoveredInstance(
            id: "port-\(port)",
            port: port,
            tabCount: tabCount,
            isVM: isVM,
            attachState: .idle
        )
    }

    private static func fetchTabCount(port: Int, session: URLSession) async -> Int? {
        guard let url = URL(string: "http://127.0.0.1:\(port)/json"),
              let (data, response) = try? await session.data(from: url),
              let httpResp = response as? HTTPURLResponse,
              httpResp.statusCode == 200,
              let json = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return nil }
        return json.count
    }

    private static func detectGvproxy(port: Int) -> Bool {
        let proc = Process()
        proc.launchPath = "/usr/sbin/lsof"
        proc.arguments = ["-i", "tcp:\(port)", "-n", "-P"]
        let pipe = Pipe()
        proc.standardOutput = pipe
        proc.standardError = Pipe()
        guard (try? proc.run()) != nil else { return false }
        proc.waitUntilExit()
        let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        return output.lowercased().contains("gvproxy")
    }
}

public extension URLSession {
    static let discoverySession: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 0.4
        config.timeoutIntervalForResource = 0.4
        return URLSession(configuration: config)
    }()
}
```

- [ ] **Step 4: Run tests**

```bash
cd apps/menubar && swift test --filter DiscoveryServiceTests
```

Expected: 5/5 PASS.

- [ ] **Step 5: Run all Swift tests**

```bash
cd apps/menubar && swift test
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add apps/menubar/Sources/PagerunnerCore/DiscoveryService.swift \
        apps/menubar/Tests/PagerunnerCoreTests/DiscoveryServiceTests.swift
git commit -m "feat: add DiscoveryService — async port probe with 30s cache and gvproxy detection"
```

---

## Task 5: RenameSheet — Reusable rename/input sheet

**Context:** Used by ProfileRowView (rename profile) and in the "Add to Profiles" flow. A SwiftUI sheet with a pre-filled `TextField`, OK disabled when empty, submitted on Return key. No unit test needed — this is pure UI.

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/Views/RenameSheet.swift`

- [ ] **Step 1: Create `RenameSheet.swift`**

```swift
import SwiftUI

/// Reusable sheet for single-field text input with OK/Cancel.
/// Usage:
///   .sheet(isPresented: $showRename) {
///       RenameSheet(title: "Rename Profile", prompt: "Display name",
///                   initialValue: profile.displayName) { newName in
///           // handle confirm
///       }
///   }
struct RenameSheet: View {
    let title: String
    let prompt: String
    let initialValue: String
    let onConfirm: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var text: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.system(size: 13, weight: .semibold))

            VStack(alignment: .leading, spacing: 4) {
                Text(prompt)
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                TextField(prompt, text: $text)
                    .textFieldStyle(.plain)
                    .font(.system(size: 12))
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background(Color.primary.opacity(0.06))
                    .cornerRadius(5)
                    .overlay(RoundedRectangle(cornerRadius: 5)
                        .stroke(Color.primary.opacity(0.15), lineWidth: 0.5))
                    .onSubmit { confirm() }
            }

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("OK") { confirm() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(text.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(16)
        .frame(width: 280)
        .onAppear { text = initialValue }
    }

    private func confirm() {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        onConfirm(trimmed)
        dismiss()
    }
}
```

- [ ] **Step 2: Build to verify compilation**

```bash
cd apps/menubar && swift build 2>&1 | grep -E "error:|warning:" | head -20
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/RenameSheet.swift
git commit -m "feat: add RenameSheet — reusable text-input sheet with OK/Cancel"
```

---

## Task 6: ProfileRowView — Extract + add right-click context menu (Rename/Remove)

**Context:** `ProfileRow` is currently defined inline in `OverviewView.swift`. Extract it to `ProfileRowView.swift`, then add a `.contextMenu` with Rename and Remove actions. Rename opens `RenameSheet` as a sheet. Remove shows an `NSAlert` confirmation. Both call `ConfigEditor` + `restartDaemon`.

`restartDaemon` is currently private in `AddProfileView`. Move it to `AppState` as a method so it's reachable from ProfileRowView and SettingsView.

**Files:**
- Create: `apps/menubar/Sources/PagerunnerBar/Views/ProfileRowView.swift`
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/OverviewView.swift` (remove inline `ProfileRow`)
- Modify: `apps/menubar/Sources/PagerunnerBar/AppState.swift` (add `restartDaemon`, `performRename`, `performRemove`)
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/AddProfileView.swift` (call `appState.restartDaemon()`)

- [ ] **Step 1: Move `restartDaemon` to `AppState`**

In `apps/menubar/Sources/PagerunnerBar/AppState.swift`, add these methods (at the bottom of the class, before the closing `}`):

```swift
// MARK: - Daemon lifecycle

func restartDaemon() {
    guard let binary = binaryPath else { return }
    let kill = Process()
    kill.launchPath = "/usr/bin/pkill"
    kill.arguments = ["-f", "pagerunner daemon"]
    try? kill.run()
    kill.waitUntilExit()
    Task {
        try? await Task.sleep(for: .milliseconds(300))
        let proc = Process()
        proc.launchPath = binary
        proc.arguments = ["daemon"]
        try? proc.run()
        transition = .starting
    }
}

// MARK: - Profile management actions

func renameProfile(_ profile: Profile, newDisplayName: String) {
    do {
        try ConfigEditor.renameProfile(name: profile.name, newDisplayName: newDisplayName)
        restartDaemon()
    } catch {
        // Silently log — UI caller handles error display if needed
        print("[AppState] renameProfile failed: \(error)")
    }
}

func removeProfile(_ profile: Profile, daemon: DaemonClient) async {
    // Close live sessions for this profile
    for session in sessions where session.profile == profile.name {
        _ = try? await daemon.call(tool: "close_session", args: ["session_id": session.id])
    }
    do {
        try ConfigEditor.removeProfile(name: profile.name)
        restartDaemon()
    } catch {
        print("[AppState] removeProfile failed: \(error)")
    }
}
```

- [ ] **Step 2: Update `AddProfileView` to use `appState.restartDaemon()`**

In `AddProfileView.swift`, replace the private `restartDaemon()` method body:

```swift
// Old: private func restartDaemon() { ... }
// New: delegate to AppState
private func restartDaemon() {
    appState.restartDaemon()
}
```

- [ ] **Step 3: Create `ProfileRowView.swift`**

Cut the `ProfileRow` struct from `OverviewView.swift` and paste it into `ProfileRowView.swift`, then add context menu and rename sheet state. The file should be:

```swift
import SwiftUI
import PagerunnerCore

struct ProfileRow: View {
    let profile: Profile
    let index: Int
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var daemon
    @State private var isHovered = false
    @State private var showRenameSheet = false
    @State private var showRemoveConfirm = false

    private var sessions: [Session] { appState.sessionsFor(profile: profile.name) }
    private var aliveSessions: [Session] { sessions.filter { $0.status == .alive } }

    private var profileName: String {
        if let parenStart = profile.displayName.firstIndex(of: "(") {
            return String(profile.displayName[..<parenStart]).trimmingCharacters(in: .whitespaces)
        }
        return profile.displayName
    }
    private var profileEmail: String? {
        guard let parenStart = profile.displayName.firstIndex(of: "("),
              let parenEnd = profile.displayName.lastIndex(of: ")") else { return nil }
        let start = profile.displayName.index(after: parenStart)
        return String(profile.displayName[start..<parenEnd])
    }

    var body: some View {
        Button {
            appState.navigation = .profile(profile.name)
        } label: {
            HStack(spacing: 9) {
                ProfileIcon(profile: profile, index: index, size: 32)
                    .overlay(alignment: .bottomTrailing) {
                        Circle()
                            .fill(aliveSessions.isEmpty
                                  ? Color(white: 0.33)
                                  : Color(red: 0.133, green: 0.773, blue: 0.369))
                            .frame(width: 7, height: 7)
                            .overlay(Circle().stroke(Color(red: 228/255, green: 228/255, blue: 228/255), lineWidth: 1.5))
                            .offset(x: 1, y: 1)
                    }

                VStack(alignment: .leading, spacing: 1) {
                    Text(profileName)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))
                    if let email = profileEmail {
                        Text(email)
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                            .lineLimit(1)
                    }
                }

                Spacer()

                HStack(spacing: 6) {
                    if !aliveSessions.isEmpty {
                        Text("\(aliveSessions.count) window\(aliveSessions.count == 1 ? "" : "s")")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.086, green: 0.396, blue: 0.204))
                            .padding(.horizontal, 7)
                            .padding(.vertical, 1)
                            .background(Color(red: 0.133, green: 0.773, blue: 0.369).opacity(0.12))
                            .cornerRadius(10)
                    } else {
                        Text("idle")
                            .font(.system(size: 11))
                            .foregroundColor(Color(red: 0.33, green: 0.33, blue: 0.33))
                            .padding(.horizontal, 7)
                            .padding(.vertical, 1)
                            .background(Color.black.opacity(0.08))
                            .cornerRadius(10)
                    }
                    Text("›")
                        .font(.system(size: 11))
                        .foregroundColor(Color(white: 0.733))
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(
                RoundedRectangle(cornerRadius: 4)
                    .fill(isHovered ? Color.black.opacity(0.04) : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
        .contextMenu {
            Button("Rename…") { showRenameSheet = true }
            Divider()
            Button("Remove…", role: .destructive) { showRemoveConfirm = true }
        }
        .sheet(isPresented: $showRenameSheet) {
            RenameSheet(
                title: "Rename Profile",
                prompt: "Display name",
                initialValue: profile.displayName
            ) { newName in
                appState.renameProfile(profile, newDisplayName: newName)
            }
        }
        .confirmationDialog(
            "Remove \"\(profileName)\"?",
            isPresented: $showRemoveConfirm,
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                Task { await appState.removeProfile(profile, daemon: daemon) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Any active sessions for this profile will be closed.")
        }
    }
}
```

- [ ] **Step 4: Remove inline `ProfileRow` from `OverviewView.swift`**

Delete the `struct ProfileRow: View { ... }` block from `OverviewView.swift` (lines 84–175 in the current file). The `ProfileIcon` struct (lines 177–212) stays in `OverviewView.swift`.

- [ ] **Step 5: Build to verify**

```bash
cd apps/menubar && swift build 2>&1 | grep "error:" | head -20
```

Expected: no errors.

- [ ] **Step 6: Run tests**

```bash
cd apps/menubar && swift test
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/ProfileRowView.swift \
        apps/menubar/Sources/PagerunnerBar/Views/OverviewView.swift \
        apps/menubar/Sources/PagerunnerBar/AppState.swift \
        apps/menubar/Sources/PagerunnerBar/Views/AddProfileView.swift
git commit -m "feat: extract ProfileRowView with right-click Rename/Remove context menu"
```

---

## Task 7: AppState — Discovery integration

**Context:** Add `discoveredInstances: [DiscoveredInstance]` and `remoteSessions: Set<String>`. Trigger `DiscoveryService.probe()` on panel open. Post-filter to exclude instances whose WebSocket URL matches a live pagerunner session. `remoteSessions` is populated when an attach succeeds with `isVM = true`.

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/AppState.swift`
- Modify: `apps/menubar/Sources/PagerunnerBar/StatusItemController.swift` (panel open hook)

- [ ] **Step 1: Add discovery state and methods to `AppState`**

In `AppState.swift`, add to the `// MARK: - Data` section:

```swift
// Discovery
var discoveredInstances: [DiscoveredInstance] = []
var remoteSessions: Set<String> = []          // session IDs attached from VM ports
private let discoveryService = DiscoveryService()
```

Add this method at the bottom of the class:

```swift
// MARK: - Discovery

/// Called when the panel opens. Returns immediately; updates discoveredInstances async.
func triggerDiscovery(daemon: DaemonClient) {
    Task { @MainActor in
        var raw = await discoveryService.probe()
        // Post-filter: exclude instances already represented by a live session.
        // We do a lightweight check: if any session's profile name matches the port label,
        // it was attached from this port and is already tracked.
        let attachedPorts = Set(
            sessions.compactMap { s -> Int? in
                // Sessions attached via attach_session have profile name like "chrome-9222"
                guard s.profile.hasPrefix("chrome-") else { return nil }
                return Int(s.profile.dropFirst("chrome-".count))
            }
        )
        raw = raw.filter { !attachedPorts.contains($0.port) }
        discoveredInstances = raw
    }
}

func attachDiscovered(_ instance: DiscoveredInstance, daemon: DaemonClient) {
    // Update attach state to .attaching
    if let idx = discoveredInstances.firstIndex(where: { $0.id == instance.id }) {
        discoveredInstances[idx].attachState = .attaching
    }
    Task { @MainActor in
        do {
            _ = try await daemon.call(tool: "attach_session", args: [
                "debug_port": instance.port,
                "profile": "chrome-\(instance.port)"
            ])
            if let idx = discoveredInstances.firstIndex(where: { $0.id == instance.id }) {
                discoveredInstances[idx].attachState = .attached
                if instance.isVM {
                    // Will be populated after next poll refreshes sessions list
                    // We mark the profile name as remote so we can flag it then
                    remoteSessions.insert("chrome-\(instance.port)")
                }
            }
        } catch {
            if let idx = discoveredInstances.firstIndex(where: { $0.id == instance.id }) {
                discoveredInstances[idx].attachState = .failed("Could not connect")
            }
        }
    }
}
```

- [ ] **Step 2: Wire discovery trigger to panel open via `.onAppear` in `OverviewView`**

`DiscoveryService` needs a `DaemonClient`, which is injected as `@Environment(\.daemonClient)` in SwiftUI views. The correct place to trigger discovery is in `OverviewView`, not `StatusItemController` (which doesn't hold a `DaemonClient`).

In `OverviewView.swift`, add to the `body` `VStack`:

```swift
// At the end of the VStack body
.onAppear {
    appState.triggerDiscovery(daemon: daemon)
}
```

And add the environment property at the top of `OverviewView`:

```swift
@Environment(\.daemonClient) private var daemon
```

This fires every time the Overview panel becomes visible, which is the correct trigger per the spec.

- [ ] **Step 3: Build**

```bash
cd apps/menubar && swift build 2>&1 | grep "error:" | head -20
```

Expected: no errors.

- [ ] **Step 4: Run tests**

```bash
cd apps/menubar && swift test
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/AppState.swift \
        apps/menubar/Sources/PagerunnerBar/StatusItemController.swift
git commit -m "feat: AppState gains discoveredInstances, remoteSessions, discovery trigger on panel open"
```

---

## Task 8: OverviewView — Render discovered instances inline

**Context:** After the last profile section, if `appState.discoveredInstances` is non-empty, render a hairline separator + rows at 70% opacity. Each row shows port, tab count, VM badge, and Attach button. Calls `appState.attachDiscovered`. Add-to-profiles flow via right-click on attached session.

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/OverviewView.swift`

- [ ] **Step 1: Add discovered rows to `OverviewView.body`**

In `OverviewView.swift`, after the `if appState.profiles.isEmpty { ... }` block, add:

```swift
// Discovered instances (inline, below profiles, macOS "Other Networks" pattern)
if !appState.discoveredInstances.isEmpty {
    Rectangle()
        .fill(Color.primary.opacity(0.06))
        .frame(height: 0.5)
        .padding(.horizontal, 12)
        .padding(.top, appState.profiles.isEmpty ? 8 : 4)

    ForEach($appState.discoveredInstances) { $instance in
        DiscoveredInstanceRow(instance: $instance, appState: appState)
    }
}
```

- [ ] **Step 2: Add `DiscoveredInstanceRow` view to `OverviewView.swift`**

Add at the bottom of `OverviewView.swift` (after `ProfileIcon`):

```swift
struct DiscoveredInstanceRow: View {
    @Binding var instance: DiscoveredInstance
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var daemon
    @State private var isHovered = false
    @State private var showAddToProfiles = false

    var body: some View {
        HStack(spacing: 9) {
            // Browser icon (plain circle, no gradient)
            ZStack {
                Circle()
                    .fill(Color(white: 0.75))
                    .frame(width: 32, height: 32)
                Image(systemName: "globe")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundColor(Color(white: 0.4))
            }

            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 4) {
                    Text(":\(instance.port)")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))
                    if instance.isVM {
                        Text("VM")
                            .font(.system(size: 9, weight: .bold))
                            .foregroundColor(Color(red: 0.2, green: 0.5, blue: 0.9))
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(Color(red: 0.2, green: 0.5, blue: 0.9).opacity(0.12))
                            .cornerRadius(3)
                    }
                }
                Text("\(instance.tabCount) tab\(instance.tabCount == 1 ? "" : "s")")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
            }

            Spacer()

            attachControl
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .opacity(0.7)
        .background(isHovered ? Color.black.opacity(0.03) : Color.clear)
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .contextMenu {
            if instance.attachState == .attached {
                Button("Add to Profiles…") { showAddToProfiles = true }
            }
        }
        .sheet(isPresented: $showAddToProfiles) {
            RenameSheet(
                title: "Save as Profile",
                prompt: "Profile name",
                initialValue: "chrome-\(instance.port)"
            ) { name in
                do {
                    try ConfigEditor.addAttachedProfile(
                        name: name.lowercased().replacingOccurrences(of: " ", with: "-"),
                        displayName: "Chrome :\(instance.port)",
                        port: instance.port
                    )
                    appState.restartDaemon()
                } catch {
                    // TODO: surface error to user — for v1, log only
                    print("[DiscoveredInstanceRow] addAttachedProfile failed: \(error)")
                }
            }
        }
    }

    @ViewBuilder
    private var attachControl: some View {
        switch instance.attachState {
        case .idle:
            Button("Attach") {
                appState.attachDiscovered(instance, daemon: daemon)
            }
            .font(.system(size: 11))
            .foregroundColor(.white)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Color(red: 0, green: 0.478, blue: 1))
            .cornerRadius(5)
            .buttonStyle(.plain)
        case .attaching:
            ProgressView().scaleEffect(0.6).frame(width: 24, height: 24)
        case .attached:
            Text("Attached")
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
        case .failed(let msg):
            Text(msg)
                .font(.system(size: 10))
                .foregroundColor(.red)
                .lineLimit(1)
        }
    }
}
```

- [ ] **Step 3: Build**

```bash
cd apps/menubar && swift build 2>&1 | grep "error:" | head -20
```

Expected: no errors.

- [ ] **Step 4: Run tests**

```bash
cd apps/menubar && swift test
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/OverviewView.swift
git commit -m "feat: render discovered Chrome instances inline in Overview with Attach button"
```

---

## Task 9: SettingsView — Add Profiles section with Rename/Remove

**Context:** Add a "PROFILES" section at the top of Settings (above Launch at login). Each row shows the profile name + Rename/Remove buttons. Rename opens `RenameSheet`. Remove shows `.confirmationDialog`. Both delegate to `AppState.renameProfile`/`removeProfile`.

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/SettingsView.swift`

- [ ] **Step 1: Add profiles section to `SettingsView.body`**

In `SettingsView.swift`, inside the `VStack(alignment: .leading, spacing: 16)` (the main content area), add the Profiles section **before** the `Toggle(isOn: $appState.launchAtLogin)` line:

```swift
// MARK: Profiles section

if !appState.profiles.isEmpty {
    VStack(alignment: .leading, spacing: 8) {
        Text("Profiles")
            .font(.system(size: 10, weight: .semibold))
            .foregroundColor(.secondary)
            .textCase(.uppercase)
            .tracking(0.5)

        ForEach(appState.profiles) { profile in
            SettingsProfileRow(profile: profile, appState: appState)
        }
    }

    Divider()
}
```

- [ ] **Step 2: Add `SettingsProfileRow` at the bottom of `SettingsView.swift`**

```swift
private struct SettingsProfileRow: View {
    let profile: Profile
    @Bindable var appState: AppState
    @Environment(\.daemonClient) private var daemon
    @State private var showRenameSheet = false
    @State private var showRemoveConfirm = false

    private var profileName: String {
        if let parenStart = profile.displayName.firstIndex(of: "(") {
            return String(profile.displayName[..<parenStart]).trimmingCharacters(in: .whitespaces)
        }
        return profile.displayName
    }

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(profile.kind == "agent" ? Color(white: 0.82) : profileGradient(index: abs(profile.name.hashValue) % 5))
                .frame(width: 14, height: 14)

            Text(profileName)
                .font(.system(size: 12))
                .lineLimit(1)

            Spacer()

            Button("Rename") { showRenameSheet = true }
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                .buttonStyle(.plain)

            Button("Remove") { showRemoveConfirm = true }
                .font(.system(size: 11))
                .foregroundColor(.red)
                .buttonStyle(.plain)
        }
        .sheet(isPresented: $showRenameSheet) {
            RenameSheet(
                title: "Rename Profile",
                prompt: "Display name",
                initialValue: profile.displayName
            ) { newName in
                appState.renameProfile(profile, newDisplayName: newName)
            }
        }
        .confirmationDialog(
            "Remove \"\(profileName)\"?",
            isPresented: $showRemoveConfirm,
            titleVisibility: .visible
        ) {
            Button("Remove", role: .destructive) {
                Task { await appState.removeProfile(profile, daemon: daemon) }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Any active sessions for this profile will be closed.")
        }
    }
}
```

Note: `profileGradient(index:)` is already defined in `OverviewView.swift` — ensure it's accessible (move to a shared file or duplicate the small function).

- [ ] **Step 3: Check `profileGradient` accessibility**

```bash
grep -rn "func profileGradient" apps/menubar/Sources/
```

If it's `private` or only in `OverviewView.swift`, make it `internal` (remove `private`) so `SettingsView.swift` can use it. Both files are in the same module.

- [ ] **Step 4: Build**

```bash
cd apps/menubar && swift build 2>&1 | grep "error:" | head -20
```

Expected: no errors.

- [ ] **Step 5: Run all tests**

```bash
cd apps/menubar && swift test
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/SettingsView.swift
git commit -m "feat: add Profiles section to Settings with Rename/Remove per row"
```

---

## Task 10: Hide Focus Tab for remote (VM) sessions

**Context:** `remoteSessions` in AppState tracks profile names of VM-attached sessions (e.g. `"chrome-9222"`). In `SessionBlockView.swift`, tabs have a "Focus tab" context menu item using AppleScript. AppleScript can't cross VM boundaries, so we hide it for remote sessions.

**Files:**
- Modify: `apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift`

- [ ] **Step 1: Find the Focus Tab context menu item**

```bash
grep -n "Focus tab\|focusTab\|focus.*tab" apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift -i
```

Find the exact line. It will be a `Button` inside a `.contextMenu` with a label like `"Focus tab"`.

- [ ] **Step 2: Wrap the Focus Tab button in a condition**

In `SessionBlockView.swift`, the `TabRowView` (or similar) has a context menu with a Focus Tab action. Wrap it:

```swift
// Before (example — match actual code):
Button("Focus tab") {
    // AppleScript focus logic
}

// After:
if !appState.remoteSessions.contains(session.profile) {
    Button("Focus tab") {
        // AppleScript focus logic
    }
}
```

`TabRowView` will need access to `appState` and `session.profile`. Check if it already has these via `@Bindable var appState` and a `session: Session` prop — if not, thread them through from `SessionBlockView`.

- [ ] **Step 3: Build**

```bash
cd apps/menubar && swift build 2>&1 | grep "error:" | head -20
```

Expected: no errors.

- [ ] **Step 4: Run tests**

```bash
cd apps/menubar && swift test
```

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add apps/menubar/Sources/PagerunnerBar/Views/SessionBlockView.swift
git commit -m "feat: hide Focus Tab for VM/remote sessions (AppleScript can't cross VM boundary)"
```

---

## Manual QA Checklist

Run these after all tasks are complete with a live daemon:

```bash
cd apps/menubar
swift build -c release
pagerunner daemon &
./.build/release/PagerunnerBar
```

- [ ] Right-click profile row → Rename → sheet appears pre-filled, confirm → name updates in Overview + daemon restart
- [ ] Right-click profile row → Remove → confirmation appears → profile gone, daemon restarts
- [ ] Settings → Profiles section lists all profiles
- [ ] Settings Rename → same sheet flow
- [ ] Settings Remove → same confirmation flow
- [ ] Panel open with Chrome on port 9225: discovered row appears (hairline separator + muted styling)
- [ ] VM badge shown for port owned by gvproxy
- [ ] Attach button → attaches → row shows "Attached", session appears in session list
- [ ] Attached session from VM port hides Focus Tab action
- [ ] Right-click "Attached" row → Add to Profiles → RenameSheet → confirm → profile appears in Overview on next launch
- [ ] Cache: close + reopen panel within 30s → no delay (cached), second open within 30s returns same results
- [ ] Close + reopen panel after 30s+ → fresh probe runs
- [ ] Non-HTTP port in range: panel opens instantly (400ms timeout, no hang)
- [ ] No discovered section when nothing found
