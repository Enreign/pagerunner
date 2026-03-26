//! Integration tests for CLI subcommands — invokes the built binary directly.
//!
//! # Running
//!
//!   cargo test --test cli_tools_integration
//!
//! Chrome tests require a live Chrome installation and a configured profile in
//! ~/.pagerunner/config.toml. They spin up a per-test daemon automatically.
//!
//! # DB isolation
//!
//! Non-Chrome tests set `PAGERUNNER_DB_PATH` to a temp file so they never conflict
//! with a live `pagerunner mcp` process that may already hold `~/.pagerunner/state.db`.
//! Chrome tests use `run_live()` which routes through a fresh test daemon.

use serial_test::serial;
use std::process::Command;

fn bin() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("pagerunner");
    p
}

/// Isolated DB path for integration tests.
fn test_db() -> std::path::PathBuf {
    std::env::temp_dir().join("pagerunner_integration_test.db")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("PAGERUNNER_DB_PATH", test_db())
        .output()
        .expect("failed to run pagerunner")
}

/// Like `run`, but without PAGERUNNER_DB_PATH so calls route through the daemon.
/// Required for Chrome tests: session state lives in the daemon's SessionManager.
fn run_live(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to run pagerunner")
}

/// Run a pagerunner command against the live test daemon and parse stdout as JSON.
/// Panics if command fails or stdout is not valid JSON.
#[allow(dead_code)]
fn run_live_json(args: &[&str]) -> serde_json::Value {
    let out = run_live(args);
    assert!(out.status.success(), "command {:?} failed: {:?}", args, String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("Failed to parse JSON from {:?}: {} — stdout: {}", args, e, String::from_utf8_lossy(&out.stdout)))
}

/// Starts a test daemon using the isolated test DB. Returns a guard that kills the
/// daemon when dropped. The daemon removes any stale socket on startup and starts
/// listening, so run_live() calls will route through it automatically.
struct TestDaemon(std::process::Child);

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait(); // Block until daemon has fully exited and released file locks
    }
}

fn start_daemon_with(binary: &std::path::Path) -> TestDaemon {
    // Kill any leftover daemon, then wait for Chrome to fully release its profile lock.
    std::process::Command::new("pkill")
        .args(&["-f", "pagerunner.*daemon"])
        .output()
        .ok();
    std::thread::sleep(std::time::Duration::from_millis(1000));

    let child = Command::new(binary)
        .args(&["daemon"])
        .env("PAGERUNNER_DB_PATH", test_db())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn test daemon");
    // Poll until the daemon socket is accepting connections (up to 3s).
    let socket = dirs::home_dir().unwrap().join(".pagerunner/daemon.sock");
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            break;
        }
    }
    TestDaemon(child)
}

fn start_test_daemon() -> TestDaemon {
    start_daemon_with(&bin())
}

/// On macOS, if the pagerunner daemon is managed by launchd (KeepAlive=true),
/// it will respawn within seconds of being killed by start_test_daemon(). This
/// causes test failures when the production daemon steals the socket mid-test.
///
/// This guard temporarily unloads the launchd service while it's held, then
/// reloads it on drop. Tests that need a stable test daemon should hold this
/// guard for the duration.
struct LaunchdGuard {
    plist_path: std::path::PathBuf,
    was_loaded: bool,
}

impl LaunchdGuard {
    fn pause_pagerunner_daemon() -> Self {
        let label = "com.pagerunner.daemon";
        let plist_path = dirs::home_dir()
            .unwrap()
            .join("Library/LaunchAgents/com.pagerunner.daemon.plist");

        // Check if the service is currently loaded/running
        let was_loaded = std::process::Command::new("launchctl")
            .args(&["list", label])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if was_loaded && plist_path.exists() {
            // Disable prevents launchd from auto-restarting after we kill the process.
            // Use bootout (macOS 10.10+) which is non-blocking unlike 'unload'.
            let uid = get_uid();
            std::process::Command::new("launchctl")
                .args(&["bootout", &format!("gui/{}", uid), plist_path.to_str().unwrap()])
                .output()
                .ok();
            // Force-kill any remaining daemon process so it doesn't hold the socket.
            std::process::Command::new("pkill")
                .args(&["-9", "-f", "pagerunner.*daemon"])
                .output()
                .ok();
            std::thread::sleep(std::time::Duration::from_millis(300));
        }

        Self { plist_path, was_loaded }
    }
}

fn get_uid() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "501".to_string())
}

impl Drop for LaunchdGuard {
    fn drop(&mut self) {
        if self.was_loaded && self.plist_path.exists() {
            // Bootstrap the service back so launchd manages it again.
            let uid = get_uid();
            std::process::Command::new("launchctl")
                .args(&["bootstrap", &format!("gui/{}", uid), self.plist_path.to_str().unwrap()])
                .output()
                .ok();
        }
    }
}

/// Returns the release binary path (target/release/pagerunner), used for NER tests
/// which require --features ner compiled in.
fn release_bin() -> std::path::PathBuf {
    let mut p = std::env::current_exe().unwrap();
    // current_exe is e.g. target/debug/deps/cli_tools_integration-xxx
    // walk up to target/, then down to release/pagerunner
    while p.file_name().map(|n| n != "target").unwrap_or(false) {
        p.pop();
    }
    p.push("release");
    p.push("pagerunner");
    p
}

/// Starts a test daemon using the NER-enabled release binary (--features ner).
/// Falls back to the debug binary if the release binary doesn't exist.
fn start_ner_test_daemon() -> TestDaemon {
    let rb = release_bin();
    if rb.exists() {
        start_daemon_with(&rb)
    } else {
        start_daemon_with(&bin())
    }
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// ─────────────────────────────────────────────────────────────
// These test arg parsing, DB operations, and error handling.
// ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn test_list_profiles_exits_ok() {
    let out = run(&["list-profiles"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    // Must be valid JSON. CLI may wrap with metadata: {"result":{...},"_metadata":{...}}
    let v: serde_json::Value = serde_json::from_str(&s)
        .unwrap_or_else(|_| panic!("expected JSON, got: {}", s));
    // Extract the result envelope (may be nested under "result" if metadata present)
    let envelope = if v["result"].is_object() { &v["result"] } else { &v };
    assert_eq!(envelope["ok"], serde_json::json!(true), "expected ok:true in: {}", s);
    assert!(
        envelope["data"].is_array(),
        "expected data array in: {}",
    // Either a JSON array/object or the "No profiles" hint
    assert!(
        serde_json::from_str::<serde_json::Value>(s.trim()).is_ok() || s.contains("No profiles"),
        "unexpected output: {}",
        s
    );
}

#[test]
#[serial]
fn test_list_sessions_returns_json() {
    let out = run(&["list-sessions"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // No Chrome sessions open, but must be valid JSON (likely empty array or object)
    let s = stdout(&out);
    assert!(!s.is_empty(), "expected some output from list-sessions");
}

/// list_sessions output includes a "status" field for each session
#[test]
#[serial]
fn test_list_sessions_has_status_field() {
    // list_sessions on empty DB should return [] (empty array) in "result"
    // No Chrome needed — just check the response shape is valid
    let out = run(&["list-sessions"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(s.trim()).expect("must be JSON");
    // Response is either a raw JSON array or wrapped in {"result": [...], "_metadata": {...}}
    let arr = if v.is_array() {
        v.as_array().unwrap().clone()
    } else {
        v["result"]
            .as_array()
            .expect("expected array at 'result' key")
            .clone()
    };
    // If the array has entries, each must have a "status" field
    for entry in &arr {
        assert!(
            entry.get("status").is_some(),
            "each session must have status field: {}",
            entry
        );
    }
}

#[test]
#[serial]
fn test_list_snapshots_returns_json() {
    let out = run(&["list-snapshots"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    // Must be a JSON envelope with ok:true and data array
    let v: serde_json::Value = serde_json::from_str(&s)
        .unwrap_or_else(|_| panic!("expected JSON, got: {}", s));
    assert_eq!(v["ok"], serde_json::json!(true), "expected ok:true in: {}", s);
    assert!(v["data"].is_array(), "expected data array in: {}", s);
    // JSON array or object (may be empty)
    assert!(
        serde_json::from_str::<serde_json::Value>(s.trim()).is_ok(),
        "expected JSON, got: {}",
        s
    );
}

#[test]
#[serial]
fn test_list_snapshots_all_flag() {
    let out = run(&["list-snapshots", "--all"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let s = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&s)
        .unwrap_or_else(|_| panic!("expected JSON, got: {}", s));
    assert_eq!(v["ok"], serde_json::json!(true), "expected ok:true in: {}", s);
    assert!(v["data"].is_array(), "expected data array in: {}", s);
    assert!(
        serde_json::from_str::<serde_json::Value>(s.trim()).is_ok(),
        "expected JSON, got: {}",
        s
    );
}

#[test]
#[serial]
fn test_kv_set_then_get() {
    let ns = "cli_test_set_get";
    let set = run(&["kv-set", ns, "mykey", "myvalue"]);
    assert!(set.status.success(), "kv-set failed: {}", stderr(&set));

    let get = run(&["kv-get", ns, "mykey"]);
    assert!(get.status.success(), "kv-get failed: {}", stderr(&get));
    assert!(
        stdout(&get).contains("myvalue"),
        "expected 'myvalue' in: {}",
        stdout(&get)
    );

    // Cleanup
    run(&["kv-clear", ns]);
}

#[test]
#[serial]
fn test_kv_list_shows_keys() {
    let ns = "cli_test_list";
    run(&["kv-set", ns, "alpha", "1"]);
    run(&["kv-set", ns, "beta", "2"]);
    run(&["kv-set", ns, "gamma", "3"]);

    let list = run(&["kv-list", ns]);
    assert!(list.status.success(), "kv-list failed: {}", stderr(&list));
    let s = stdout(&list);
    assert!(
        s.contains("alpha") && s.contains("beta") && s.contains("gamma"),
        "expected all keys in: {}",
        s
    );

    run(&["kv-clear", ns]);
}

#[test]
#[serial]
fn test_kv_list_prefix_filter() {
    let ns = "cli_test_prefix";
    run(&["kv-set", ns, "foo-1", "a"]);
    run(&["kv-set", ns, "foo-2", "b"]);
    run(&["kv-set", ns, "bar-1", "c"]);

    let list = run(&["kv-list", ns, "--prefix", "foo"]);
    assert!(list.status.success());
    let s = stdout(&list);
    assert!(
        s.contains("foo-1") && s.contains("foo-2"),
        "expected foo-* keys in: {}",
        s
    );
    assert!(
        !s.contains("bar-1"),
        "unexpected bar-1 in prefix-filtered output: {}",
        s
    );

    run(&["kv-clear", ns]);
}

#[test]
#[serial]
fn test_kv_list_keys_only_flag() {
    let ns = "cli_test_keys_only";
    run(&["kv-set", ns, "k1", "v1"]);

    let list = run(&["kv-list", ns, "--keys-only"]);
    assert!(list.status.success());
    let s = stdout(&list);
    assert!(s.contains("k1"), "expected key in: {}", s);
    // Values should not appear (include_values: false)
    assert!(
        !s.contains("v1"),
        "value should not appear in keys-only output: {}",
        s
    );

    run(&["kv-clear", ns]);
}

#[test]
#[serial]
fn test_kv_delete_removes_key() {
    let ns = "cli_test_delete";
    run(&["kv-set", ns, "todelete", "gone"]);

    let del = run(&["kv-delete", ns, "todelete"]);
    assert!(del.status.success(), "kv-delete failed: {}", stderr(&del));

    let get = run(&["kv-get", ns, "todelete"]);
    let s = stdout(&get);
    // Should return null or empty (key no longer exists)
    assert!(
        s.contains("null") || s.trim().is_empty(),
        "expected null/empty for deleted key, got: {}",
        s
    );

    run(&["kv-clear", ns]);
}

#[test]
#[serial]
fn test_kv_clear_removes_all_keys() {
    let ns = "cli_test_clear";
    run(&["kv-set", ns, "a", "1"]);
    run(&["kv-set", ns, "b", "2"]);

    let clear = run(&["kv-clear", ns]);
    assert!(
        clear.status.success(),
        "kv-clear failed: {}",
        stderr(&clear)
    );

    let list = run(&["kv-list", ns]);
    let s = stdout(&list);
    // Namespace should now be empty — response is {"ok":true,"data":[]}
    let v: serde_json::Value = serde_json::from_str(&s)
        .unwrap_or_else(|_| panic!("expected JSON, got: {}", s));
    let arr = v["data"].as_array().unwrap_or_else(|| {
        panic!("expected data array in kv-list response, got: {}", s)
    });
    assert!(
        arr.is_empty(),
        "expected empty namespace after kv-clear, got: {}",
        s
    );
}

#[test]
#[serial]
fn test_open_session_unknown_profile_exits_nonzero() {
    let out = run(&["open-session", "nonexistent-profile-xyz"]);
    assert!(
        !out.status.success(),
        "expected non-zero exit for unknown profile"
    );
    let err = stderr(&out);
    // Error should mention the profile name
    assert!(
        err.contains("nonexistent-profile-xyz") || err.contains("not found"),
        "expected error mentioning profile, got: {}",
        err
    );
}

#[test]
#[serial]
fn test_get_content_invalid_session_exits_nonzero() {
    let out = run(&["get-content", "no-such-session", "no-such-target"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_navigate_invalid_session_exits_nonzero() {
    let out = run(&[
        "navigate",
        "no-such-session",
        "no-such-target",
        "https://example.com",
    ]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_click_invalid_session_exits_nonzero() {
    let out = run(&["click", "no-such-session", "no-such-target", "button"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_screenshot_invalid_session_exits_nonzero() {
    let out = run(&["screenshot", "no-such-session", "no-such-target"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_evaluate_invalid_session_exits_nonzero() {
    let out = run(&[
        "evaluate",
        "no-such-session",
        "no-such-target",
        "document.title",
    ]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_list_tabs_invalid_session_exits_nonzero() {
    let out = run(&["list-tabs", "no-such-session"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_save_tab_state_invalid_session_exits_nonzero() {
    let out = run(&["save-tab-state", "no-such-session"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
#[serial]
fn test_wait_for_invalid_session_exits_nonzero() {
    let out = run(&["wait-for", "no-such-session", "no-such-target", "--ms", "1"]);
    assert!(!out.status.success(), "expected non-zero exit");
    assert!(!stderr(&out).is_empty(), "expected error on stderr");
}

#[test]
fn test_missing_required_arg_exits_nonzero_with_clap_error() {
    // `navigate` requires session_id, target_id, url — omit all
    let out = run(&["navigate"]);
    assert!(
        !out.status.success(),
        "expected clap error for missing args"
    );
    // clap writes to stderr
    let err = stderr(&out);
    assert!(!err.is_empty(), "expected clap error on stderr");
}

#[test]
fn test_screenshot_help_includes_base64_flag() {
    let out = run(&["screenshot", "--help"]);
    // --help exits 0
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(
        s.contains("base64"),
        "expected --base64 flag in screenshot help: {}",
        s
    );
}

#[test]
fn test_open_session_help_includes_anonymization_flags() {
    let out = run(&["open-session", "--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(
        s.contains("anonymize"),
        "expected --anonymize in open-session help: {}",
        s
    );
    assert!(
        s.contains("stealth"),
        "expected --stealth in open-session help: {}",
        s
    );
}

#[test]
fn test_wait_for_help_shows_all_wait_modes() {
    let out = run(&["wait-for", "--help"]);
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(
        s.contains("selector"),
        "expected --selector in wait-for help: {}",
        s
    );
    assert!(s.contains("url"), "expected --url in wait-for help: {}", s);
    assert!(s.contains("ms"), "expected --ms in wait-for help: {}", s);
}

// ─────────────────────────────────────────────────────────────
// Chrome tests
// ─────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_full_session_lifecycle() {
    // open-session → new-tab → navigate → get-content → close-session
    // Requires the first configured profile to exist in ~/.pagerunner/config.toml
    // Uses run_live (daemon) so session state persists across CLI invocations.
    let _daemon = start_test_daemon();
    let profiles_out = run_live(&["list-profiles"]);
    let s = stdout(&profiles_out);
    let profiles: serde_json::Value = serde_json::from_str(&s).unwrap();
    let profile = profiles["data"][0]["name"].as_str().unwrap().to_string();

    let open = run_live(&["open-session", &profile]);
    assert!(
        open.status.success(),
        "open-session failed: {}",
        stderr(&open)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&open)).unwrap();
    let sid = v["session_id"].as_str().unwrap().to_string();

    let tab = run_live(&["new-tab", &sid]);
    assert!(tab.status.success(), "new-tab failed: {}", stderr(&tab));
    let v: serde_json::Value = serde_json::from_str(&stdout(&tab)).unwrap();
    let tid = v["target_id"].as_str().unwrap().to_string();

    let nav = run_live(&["navigate", &sid, &tid, "https://example.com"]);
    assert!(nav.status.success(), "navigate failed: {}", stderr(&nav));

    let content = run_live(&["get-content", &sid, &tid]);
    assert!(
        content.status.success(),
        "get-content failed: {}",
        stderr(&content)
    );
    assert!(
        stdout(&content).contains("Example Domain"),
        "expected page content, got: {}",
        stdout(&content)
    );

    let close = run_live(&["close-session", &sid]);
    assert!(
        close.status.success(),
        "close-session failed: {}",
        stderr(&close)
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_screenshot_file_mode_writes_png() {
    let _daemon = start_test_daemon();
    let profiles_out = run_live(&["list-profiles"]);
    let profiles: serde_json::Value = serde_json::from_str(&stdout(&profiles_out)).unwrap();
    let profile = profiles["data"][0]["name"].as_str().unwrap().to_string();

    let open = run_live(&["open-session", &profile]);
    let sid = serde_json::from_str::<serde_json::Value>(&stdout(&open)).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let tab = run_live(&["new-tab", &sid]);
    let tid = serde_json::from_str::<serde_json::Value>(&stdout(&tab)).unwrap()["target_id"]
        .as_str()
        .unwrap()
        .to_string();

    let shot = run_live(&["screenshot", &sid, &tid]);
    assert!(
        shot.status.success(),
        "screenshot failed: {}",
        stderr(&shot)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&shot)).unwrap();
    let path = v["file"]
        .as_str()
        .expect("expected file key in screenshot output");
    assert!(
        std::path::Path::new(path).exists(),
        "PNG file not found at {}",
        path
    );
    assert!(
        std::fs::metadata(path).unwrap().len() > 0,
        "PNG file is empty"
    );
    std::fs::remove_file(path).ok();

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_screenshot_base64_mode_returns_inline() {
    let _daemon = start_test_daemon();
    let profiles_out = run_live(&["list-profiles"]);
    let profiles: serde_json::Value = serde_json::from_str(&stdout(&profiles_out)).unwrap();
    let profile = profiles["data"][0]["name"].as_str().unwrap().to_string();

    let open = run_live(&["open-session", &profile]);
    let sid = serde_json::from_str::<serde_json::Value>(&stdout(&open)).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let tab = run_live(&["new-tab", &sid]);
    let tid = serde_json::from_str::<serde_json::Value>(&stdout(&tab)).unwrap()["target_id"]
        .as_str()
        .unwrap()
        .to_string();

    let shot = run_live(&["screenshot", &sid, &tid, "--base64"]);
    assert!(shot.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&shot)).unwrap();
    assert!(
        v["base64"].as_str().is_some(),
        "expected base64 key in: {}",
        stdout(&shot)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_evaluate_returns_json() {
    let _daemon = start_test_daemon();
    let profiles_out = run_live(&["list-profiles"]);
    let profiles: serde_json::Value = serde_json::from_str(&stdout(&profiles_out)).unwrap();
    let profile = profiles["data"][0]["name"].as_str().unwrap().to_string();

    let open = run_live(&["open-session", &profile]);
    let sid = serde_json::from_str::<serde_json::Value>(&stdout(&open)).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let tab = run_live(&["new-tab", &sid]);
    let tid = serde_json::from_str::<serde_json::Value>(&stdout(&tab)).unwrap()["target_id"]
        .as_str()
        .unwrap()
        .to_string();

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    let eval = run_live(&["evaluate", &sid, &tid, "1 + 1"]);
    assert!(eval.status.success(), "evaluate failed: {}", stderr(&eval));
    assert!(
        stdout(&eval).contains("2"),
        "expected '2' in evaluate output: {}",
        stdout(&eval)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_kv_roundtrip_with_live_session() {
    let set = run(&["kv-set", "test_cli_chrome", "hello", "world"]);
    assert!(set.status.success());

    let get = run(&["kv-get", "test_cli_chrome", "hello"]);
    assert!(get.status.success());
    assert!(stdout(&get).contains("world"));

    run(&["kv-clear", "test_cli_chrome"]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_list_tabs_shows_open_tab() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    let list = run_live(&["list-tabs", &sid]);
    assert!(list.status.success(), "list-tabs failed: {}", stderr(&list));
    let s = stdout(&list);
    assert!(
        s.contains(&tid),
        "expected target_id in list-tabs output: {}",
        s
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_wait_for_ms() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    let wait = run_live(&["wait-for", &sid, &tid, "--ms", "100"]);
    assert!(
        wait.status.success(),
        "wait-for --ms failed: {}",
        stderr(&wait)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_snapshot_save_list_delete() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    // Save snapshot
    let save = run_live(&["save-snapshot", &sid, &tid]);
    assert!(
        save.status.success(),
        "save-snapshot failed: {}",
        stderr(&save)
    );

    // List snapshots via daemon
    let list = run_live(&["list-snapshots"]);
    assert!(
        list.status.success(),
        "list-snapshots failed: {}",
        stderr(&list)
    );
    let s = stdout(&list);
    let v: serde_json::Value = serde_json::from_str(&s)
        .unwrap_or_else(|_| panic!("expected JSON from list-snapshots, got: {}", s));
    assert!(v["data"].is_array(), "expected data array in list-snapshots: {}", s);
    assert!(
        serde_json::from_str::<serde_json::Value>(s.trim()).is_ok(),
        "expected JSON: {}",
        s
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_tab_state_save_restore() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let _tid = parse_json_field(&stdout(&tab), "target_id");

    // Save tab state
    let save = run_live(&["save-tab-state", &sid]);
    assert!(
        save.status.success(),
        "save-tab-state failed: {}",
        stderr(&save)
    );

    // Restore tab state
    let restore = run_live(&["restore-tab-state", &sid]);
    assert!(
        restore.status.success(),
        "restore-tab-state failed: {}",
        stderr(&restore)
    );

    run_live(&["close-session", &sid]);
}

// ─────────────────────────────────────────────────────────────
// Chrome tests — interactions
// ─────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_click() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&[
        "navigate",
        &sid,
        &tid,
        "https://the-internet.herokuapp.com/checkboxes",
    ]);

    // Click the first checkbox
    let click = run_live(&["click", &sid, &tid, "input[type=checkbox]"]);
    assert!(click.status.success(), "click failed: {}", stderr(&click));

    // Verify the checkbox is now checked
    let eval = run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.querySelector('input[type=checkbox]').checked",
    ]);
    assert!(
        eval.status.success(),
        "evaluate after click failed: {}",
        stderr(&eval)
    );
    assert!(
        stdout(&eval).contains("true"),
        "expected checkbox to be checked: {}",
        stdout(&eval)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_fill_input() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&[
        "navigate",
        &sid,
        &tid,
        "https://the-internet.herokuapp.com/login",
    ]);
    run_live(&["wait-for", &sid, &tid, "--selector", "#username"]);

    // Fill the username input
    let fill = run_live(&["fill", &sid, &tid, "#username", "tomsmith"]);
    assert!(fill.status.success(), "fill failed: {}", stderr(&fill));

    // Verify value was set
    let eval = run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.querySelector('#username').value",
    ]);
    assert!(
        eval.status.success(),
        "evaluate after fill failed: {}",
        stderr(&eval)
    );
    assert!(
        stdout(&eval).contains("tomsmith"),
        "expected 'tomsmith' in input value: {}",
        stdout(&eval)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_fill_textarea() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&[
        "navigate",
        &sid,
        &tid,
        "https://the-internet.herokuapp.com/login",
    ]);
    run_live(&["wait-for", &sid, &tid, "--selector", "form"]);

    // Inject a textarea into the page
    run_live(&["evaluate", &sid, &tid,
        "const ta = document.createElement('textarea'); ta.id = 'test-ta'; document.body.appendChild(ta);"]);

    // Fill the textarea
    let fill = run_live(&["fill", &sid, &tid, "#test-ta", "hello textarea"]);
    assert!(
        fill.status.success(),
        "fill on textarea failed: {}",
        stderr(&fill)
    );

    // Verify value was set
    let eval = run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.querySelector('#test-ta').value",
    ]);
    assert!(
        eval.status.success(),
        "evaluate after fill on textarea failed: {}",
        stderr(&eval)
    );
    assert!(
        stdout(&eval).contains("hello textarea"),
        "expected 'hello textarea' in textarea value: {}",
        stdout(&eval)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_type_text() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&[
        "navigate",
        &sid,
        &tid,
        "https://the-internet.herokuapp.com/login",
    ]);
    run_live(&["wait-for", &sid, &tid, "--selector", "#username"]);

    // Click to focus the username input, then type into it
    run_live(&["click", &sid, &tid, "#username"]);
    let type_out = run_live(&["type-text", &sid, &tid, "tomsmith"]);
    assert!(
        type_out.status.success(),
        "type-text failed: {}",
        stderr(&type_out)
    );

    // Verify value was typed
    let eval = run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.querySelector('#username').value",
    ]);
    assert!(
        eval.status.success(),
        "evaluate after type-text failed: {}",
        stderr(&eval)
    );
    assert!(
        stdout(&eval).contains("tomsmith"),
        "expected 'tomsmith' in input value: {}",
        stdout(&eval)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_select() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&[
        "navigate",
        &sid,
        &tid,
        "https://the-internet.herokuapp.com/dropdown",
    ]);

    // Select option 2 by its value attribute ("2"), not its display text
    let sel = run_live(&["select", &sid, &tid, "#dropdown", "2"]);
    assert!(sel.status.success(), "select failed: {}", stderr(&sel));

    // Verify selectedIndex changed (option with value="2" is at index 2)
    let eval = run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.querySelector('#dropdown').selectedIndex",
    ]);
    assert!(
        eval.status.success(),
        "evaluate after select failed: {}",
        stderr(&eval)
    );
    assert!(
        stdout(&eval).contains("2"),
        "expected selectedIndex 2: {}",
        stdout(&eval)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_scroll_y() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    // Scroll — just assert no error (example.com may not be scrollable)
    let scroll = run_live(&["scroll", &sid, &tid, "--y", "100"]);
    assert!(
        scroll.status.success(),
        "scroll failed: {}",
        stderr(&scroll)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_invalid_selector_error() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    // Click a nonexistent selector — should fail
    let click = run_live(&["click", &sid, &tid, "#nonexistent-selector-xyz"]);
    assert!(
        !click.status.success(),
        "expected non-zero exit for invalid selector"
    );
    assert!(!stderr(&click).is_empty(), "expected error on stderr");

    run_live(&["close-session", &sid]);
}

// ─────────────────────────────────────────────────────────────
// Chrome tests — navigation waits
// ─────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_wait_for_selector() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    let wait = run_live(&["wait-for", &sid, &tid, "--selector", "h1"]);
    assert!(
        wait.status.success(),
        "wait-for --selector h1 failed: {}",
        stderr(&wait)
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_wait_for_url_substring() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    let wait = run_live(&["wait-for", &sid, &tid, "--url", "example"]);
    assert!(
        wait.status.success(),
        "wait-for --url 'example' failed: {}",
        stderr(&wait)
    );

    run_live(&["close-session", &sid]);
}

/// wait-for --selector returns stability_ms in JSON response.
/// Uses data: URL with setTimeout to simulate JS-rendered content (SPA pattern).
#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_wait_for_selector_returns_stability_ms() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    // Navigate to a minimal blank page
    run_live(&[
        "navigate",
        &sid,
        &tid,
        "data:text/html,<html><body></body></html>",
    ]);

    // Inject an element after 300ms via evaluate (simulates JS-rendered SPA content)
    run_live(&[
        "evaluate",
        &sid,
        &tid,
        "setTimeout(() => { document.body.innerHTML = '<h1 id=\"spa-loaded\">Done</h1>'; }, 300); null",
    ]);

    // wait-for should find the element and report stability_ms
    let wait = run_live(&[
        "wait-for",
        &sid,
        &tid,
        "--selector",
        "#spa-loaded",
        "--timeout-ms",
        "5000",
    ]);
    assert!(
        wait.status.success(),
        "wait-for --selector failed: {}",
        stderr(&wait)
    );

    let v: serde_json::Value = serde_json::from_str(&stdout(&wait))
        .expect("wait-for output is not valid JSON");
    assert_eq!(v["ok"], true, "expected ok=true: {}", stdout(&wait));
    assert!(
        v["stability_ms"].is_number(),
        "stability_ms must be present as a number: {}",
        stdout(&wait)
    );
    let ms = v["stability_ms"].as_u64().unwrap_or(0);
    assert!(
        ms >= 200,
        "stability_ms should be >= 200ms (element added after 300ms), got {ms}"
    );
    assert!(
        ms < 5000,
        "stability_ms should be < 5000ms (timeout), got {ms}"
    );

    run_live(&["close-session", &sid]);
}

/// wait-for --url returns stability_ms in JSON response.
#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_wait_for_url_returns_stability_ms() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    let wait = run_live(&["wait-for", &sid, &tid, "--url", "example.com"]);
    assert!(
        wait.status.success(),
        "wait-for --url failed: {}",
        stderr(&wait)
    );

    let v: serde_json::Value = serde_json::from_str(&stdout(&wait))
        .expect("wait-for output is not valid JSON");
    assert_eq!(v["ok"], true, "expected ok=true: {}", stdout(&wait));
    assert!(
        v["stability_ms"].is_number(),
        "stability_ms must be present as a number: {}",
        stdout(&wait)
    );

    run_live(&["close-session", &sid]);
}

// ─────────────────────────────────────────────────────────────
// Chrome tests — anonymization
// ─────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_anonymize_get_content() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile, "--anonymize"]);
    assert!(
        open.status.success(),
        "open-session --anonymize failed: {}",
        stderr(&open)
    );
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    // Inject PII into the page body
    run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.body.innerHTML = '<p>Email: test@example.com Phone: 555-867-5309</p>'",
    ]);

    // get-content should return anonymized output
    let content = run_live(&["get-content", &sid, &tid]);
    assert!(
        content.status.success(),
        "get-content failed: {}",
        stderr(&content)
    );
    let s = stdout(&content);
    assert!(
        s.contains("[EMAIL:"),
        "expected [EMAIL: token in anonymized content: {}",
        s
    );
    assert!(
        !s.contains("test@example.com"),
        "raw email must not appear in anonymized content: {}",
        s
    );

    run_live(&["close-session", &sid]);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_anonymize_screenshot_blocked() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile, "--anonymize"]);
    assert!(
        open.status.success(),
        "open-session --anonymize failed: {}",
        stderr(&open)
    );
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    // Screenshot is blocked in anonymize mode: call_tool returns a JSON error object
    // (not a process exit code), so check stdout for the error message.
    let shot = run_live(&["screenshot", &sid, &tid]);
    let out = stdout(&shot);
    assert!(
        out.contains("AnonymizationActive") || out.contains("blocked"),
        "expected screenshot-blocked error in output, got: {}",
        out
    );

    run_live(&["close-session", &sid]);
}

// ─────────────────────────────────────────────────────────────
// Chrome tests — security
// ─────────────────────────────────────────────────────────────

#[test]
#[cfg_attr(not(target_os = "macos"), ignore)]
#[serial]
fn test_cli_allowed_domains_blocks_nav() {
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile, "--allowed-domains", "example.com"]);
    assert!(
        open.status.success(),
        "open-session --allowed-domains failed: {}",
        stderr(&open)
    );
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    // Navigate to allowed domain — should succeed
    let nav_ok = run_live(&["navigate", &sid, &tid, "https://example.com"]);
    assert!(
        nav_ok.status.success(),
        "navigate to allowed domain failed: {}",
        stderr(&nav_ok)
    );

    // Navigate to disallowed domain — should fail
    let nav_blocked = run_live(&["navigate", &sid, &tid, "https://httpbin.org"]);
    assert!(
        !nav_blocked.status.success(),
        "expected navigation to httpbin.org to be blocked"
    );
    let err = stderr(&nav_blocked);
    assert!(
        err.contains("domain") || err.contains("allow") || err.contains("not permitted"),
        "expected domain restriction error: {}",
        err
    );

    run_live(&["close-session", &sid]);
}

// ─────────────────────────────────────────────────────────────
// Chrome tests — NER anonymization (requires --features ner build)
// ─────────────────────────────────────────────────────────────

/// Requires: `cargo build --release --features ner` + model at ~/.pagerunner/models/ner.onnx
/// Run locally with: cargo test --test cli_tools_integration test_cli_ner_anonymize_person_masked -- --ignored
#[test]
#[ignore] // requires `cargo build --release --features ner` + model at ~/.pagerunner/models/ner.onnx
#[serial]
fn test_cli_ner_anonymize_person_masked() {
    // Uses the NER-enabled release binary as the test daemon so that PERSON/ORG
    // detection is active. Falls back to debug binary if release binary is absent.
    let _daemon = start_ner_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile, "--anonymize"]);
    assert!(
        open.status.success(),
        "open-session --anonymize failed: {}",
        stderr(&open)
    );
    let sid = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &sid]);
    let tid = parse_json_field(&stdout(&tab), "target_id");

    run_live(&["navigate", &sid, &tid, "https://example.com"]);

    // Inject content with named persons and organisations
    run_live(&[
        "evaluate",
        &sid,
        &tid,
        "document.body.innerHTML = '<p>Alice Smith is CEO of Acme Corp.</p>'",
    ]);

    // get-content should tokenize PERSON and ORG with NER build
    let content = run_live(&["get-content", &sid, &tid]);
    assert!(
        content.status.success(),
        "get-content failed: {}",
        stderr(&content)
    );
    let s = stdout(&content);
    assert!(
        s.contains("[PERSON:"),
        "expected [PERSON: token in NER-anonymized content: {}",
        s
    );

    run_live(&["close-session", &sid]);
}

// ─────────────────────────────────────────────────────────────
// get_network_log tests
// ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn test_get_network_log_invalid_session() {
    let output = run(&["get-network-log", "invalid-session-id", "--target-id", "tab1"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found") || stderr.contains("SessionNotFound") || stderr.contains("session"));
}

#[cfg_attr(not(target_os = "macos"), ignore)]
#[test]
#[serial]
fn test_get_network_log_captures_requests() {
    // Pause the launchd-managed production daemon so it doesn't steal the socket.
    let _launchd = LaunchdGuard::pause_pagerunner_daemon();
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    assert!(open.status.success(), "open-session failed: {}", stderr(&open));
    let session_id = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &session_id]);
    assert!(tab.status.success(), "new-tab failed: {}", stderr(&tab));
    let target_id = parse_json_field(&stdout(&tab), "target_id");

    let nav = run_live(&["navigate", &session_id, &target_id, "https://httpbin.org/get"]);
    assert!(nav.status.success(), "navigate failed: {}", stderr(&nav));

    // get-content waits for DOM to be ready, ensuring the page loaded and network
    // events for the navigation request have been captured.
    run_live(&["get-content", &session_id, &target_id]);

    let log_out = run_live(&[
        "get-network-log", &session_id,
        "--target-id", &target_id,
        "--limit", "100",
    ]);
    assert!(log_out.status.success(), "get-network-log failed: {}", stderr(&log_out));
    let output: serde_json::Value = serde_json::from_str(&stdout(&log_out))
        .unwrap_or_else(|e| panic!("get-network-log output not JSON: {} — {}", e, stdout(&log_out)));

    assert_eq!(output["ok"], true);
    let entries = output["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "should have captured at least one request");

    let httpbin_entry = entries.iter().find(|e| {
        e["url"].as_str().unwrap_or("").contains("httpbin.org")
    });
    assert!(httpbin_entry.is_some(), "httpbin.org request should be captured");
    assert_eq!(httpbin_entry.unwrap()["status"], 200);

    run_live(&["close-session", &session_id]);
}

#[cfg_attr(not(target_os = "macos"), ignore)]
#[test]
#[serial]
fn test_get_network_log_url_filter() {
    // Pause the launchd-managed production daemon so it doesn't steal the socket.
    let _launchd = LaunchdGuard::pause_pagerunner_daemon();
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    assert!(open.status.success(), "open-session failed: {}", stderr(&open));
    let session_id = parse_json_field(&stdout(&open), "session_id");

    let tab = run_live(&["new-tab", &session_id]);
    assert!(tab.status.success(), "new-tab failed: {}", stderr(&tab));
    let target_id = parse_json_field(&stdout(&tab), "target_id");

    let nav = run_live(&["navigate", &session_id, &target_id, "https://httpbin.org/get"]);
    assert!(nav.status.success(), "navigate failed: {}", stderr(&nav));

    // get-content waits for DOM to be ready, ensuring the page loaded and network
    // events for the navigation request have been captured.
    run_live(&["get-content", &session_id, &target_id]);

    let log_out = run_live(&[
        "get-network-log", &session_id,
        "--target-id", &target_id,
        "--url-pattern", "httpbin.org",
    ]);
    assert!(log_out.status.success(), "get-network-log failed: {}", stderr(&log_out));
    let output: serde_json::Value = serde_json::from_str(&stdout(&log_out))
        .unwrap_or_else(|e| panic!("get-network-log output not JSON: {} — {}", e, stdout(&log_out)));

    assert_eq!(output["ok"], true);
    let entries = output["entries"].as_array().unwrap();
    for e in entries {
        assert!(e["url"].as_str().unwrap().contains("httpbin.org"),
            "all entries should match url filter");
    }

    run_live(&["close-session", &session_id]);
}

#[cfg_attr(not(target_os = "macos"), ignore)]
#[test]
#[serial]
fn test_get_network_log_validation_error() {
    // Pause the launchd-managed production daemon so it doesn't steal the socket.
    let _launchd = LaunchdGuard::pause_pagerunner_daemon();
    let _daemon = start_test_daemon();
    let profile = first_profile();

    let open = run_live(&["open-session", &profile]);
    assert!(open.status.success(), "open-session failed: {}", stderr(&open));
    let session_id = parse_json_field(&stdout(&open), "session_id");

    // No --target-id and no --all-tabs: expect VALIDATION_ERROR
    let log_out = run_live(&["get-network-log", &session_id]);
    assert!(log_out.status.success(), "get-network-log should succeed (returns JSON error): {}", stderr(&log_out));
    let output: serde_json::Value = serde_json::from_str(&stdout(&log_out))
        .unwrap_or_else(|e| panic!("get-network-log output not JSON: {} — {}", e, stdout(&log_out)));
    assert_eq!(output["ok"], false);
    assert_eq!(output["error_type"], "VALIDATION_ERROR");

    run_live(&["close-session", &session_id]);
}

// ─────────────────────────────────────────────────────────────
// Helpers used by Chrome tests
// ─────────────────────────────────────────────────────────────

/// Extract a string field from a JSON object string.
fn parse_json_field(json_str: &str, field: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(json_str)
        .unwrap_or_else(|e| panic!("parse_json_field: invalid JSON {:?}: {}", json_str, e));
    v[field]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "parse_json_field: field '{}' not found in {}",
                field, json_str
            )
        })
        .to_string()
}

/// Returns the name of the first configured profile.
fn first_profile() -> String {
    let out = run_live(&["list-profiles"]);
    let profiles: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("list-profiles did not return JSON");
    profiles["data"][0]["name"]
        .as_str()
        .expect("no profiles configured")
        .to_string()
}

// ---------------------------------------------------------------------------
// init --json tests (LNY-171)
// ---------------------------------------------------------------------------

#[test]
fn test_init_json_flag_accepted() {
    // Verify --json flag is accepted (not "unexpected argument")
    // Chrome may not be available in test env so we just check the flag is valid
    let tmp = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(bin())
        .args(["init", "--json", "--force"])
        .current_dir(tmp.path())
        .env("PAGERUNNER_DB_PATH", "/tmp/pagerunner_integration_test.db")
        .output()
        .expect("binary should run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unexpected argument"),
        "--json must be a valid flag; stderr: {stderr}"
    );
}

#[test]
fn test_init_json_with_claude_md_returns_snippet() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("CLAUDE.md"), "# My Project\n").unwrap();
    // Create fake home with pre-existing config so Chrome detection is bypassed
    let fake_home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fake_home.path().join(".pagerunner")).unwrap();
    std::fs::write(
        fake_home.path().join(".pagerunner/config.toml"),
        "# pagerunner config\n",
    )
    .unwrap();
    let out = std::process::Command::new(bin())
        .args(["init", "--json"])
        .current_dir(tmp.path())
        .env("HOME", fake_home.path())
        .env("PAGERUNNER_DB_PATH", "/tmp/pagerunner_integration_test.db")
        .output()
        .expect("binary should run");
    assert!(
        out.status.success(),
        "exit non-zero; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("must be valid JSON");
    assert_eq!(v["ok"], true, "ok must be true: {v}");
    assert!(v["snippet"].is_string(), "snippet field missing: {v}");
    assert!(
        !v["snippet"].as_str().unwrap_or("").is_empty(),
        "snippet must not be empty: {v}"
    );
    assert_eq!(
        v["project_file"].as_str().unwrap_or(""),
        "CLAUDE.md",
        "project_file must be CLAUDE.md: {v}"
    );
}

#[test]
fn test_init_json_with_agents_md_returns_snippet() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("AGENTS.md"), "# My Project\n").unwrap();
    let fake_home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(fake_home.path().join(".pagerunner")).unwrap();
    std::fs::write(
        fake_home.path().join(".pagerunner/config.toml"),
        "# pagerunner config\n",
    )
    .unwrap();
    let out = std::process::Command::new(bin())
        .args(["init", "--json"])
        .current_dir(tmp.path())
        .env("HOME", fake_home.path())
        .env("PAGERUNNER_DB_PATH", "/tmp/pagerunner_integration_test.db")
        .output()
        .expect("binary should run");
    assert!(
        out.status.success(),
        "exit non-zero; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("must be valid JSON");
    assert_eq!(v["ok"], true, "ok must be true: {v}");
    assert!(v["snippet"].is_string(), "snippet field missing: {v}");
    assert_eq!(
        v["project_file"].as_str().unwrap_or(""),
        "AGENTS.md",
        "project_file must be AGENTS.md: {v}"
    );
}
