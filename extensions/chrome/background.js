// Pagerunner Chrome Extension — Service Worker
// Manages the native messaging connection to the Pagerunner daemon.

const HOST_NAME = "com.pagerunner.host";

let port = null;
let connected = false;
let pendingCallbacks = new Map(); // requestId → { resolve, reject }
let nextRequestId = 1;

// ── Connection management ─────────────────────────────────────────────────────

function connect() {
  if (port) return;

  try {
    port = chrome.runtime.connectNative(HOST_NAME);
  } catch (err) {
    connected = false;
    port = null;
    return;
  }

  port.onMessage.addListener(onNativeMessage);

  port.onDisconnect.addListener(() => {
    connected = false;
    port = null;
    // Reject all pending calls — the host went away.
    for (const [id, cb] of pendingCallbacks) {
      cb.reject(new Error("Native host disconnected"));
    }
    pendingCallbacks.clear();
  });

  // Probe the daemon immediately after connecting.
  sendRequest({ tool: "list_sessions", args: {} })
    .then(() => { connected = true; })
    .catch(() => { connected = false; });
}

function ensureConnected() {
  if (!port) connect();
}

// ── Native messaging protocol ─────────────────────────────────────────────────
// The native host uses a simple JSON-lines protocol over the Chrome native
// messaging framing (4-byte LE length prefix + JSON payload).
//
// Request format:  { id, tool, args }
// Response format: { id, result?, error? }  — result is a JSON string (double-serialised)

function sendRequest(msg) {
  return new Promise((resolve, reject) => {
    ensureConnected();
    if (!port) {
      reject(new Error("Native host not available"));
      return;
    }

    const id = String(nextRequestId++);
    pendingCallbacks.set(id, { resolve, reject });
    port.postMessage({ id, tool: msg.tool, args: msg.args || {} });

    // 30-second timeout per request.
    setTimeout(() => {
      if (pendingCallbacks.has(id)) {
        pendingCallbacks.delete(id);
        reject(new Error("Request timed out"));
      }
    }, 30000);
  });
}

function onNativeMessage(msg) {
  if (!msg || !msg.id) return;

  const cb = pendingCallbacks.get(msg.id);
  if (!cb) return;
  pendingCallbacks.delete(msg.id);

  if (msg.error) {
    cb.reject(new Error(msg.error));
  } else {
    // Parse the double-serialised inner result.
    try {
      const inner = typeof msg.result === "string"
        ? JSON.parse(msg.result)
        : msg.result;
      cb.resolve(inner);
    } catch (e) {
      cb.reject(new Error("Malformed response from host"));
    }
  }
}

// ── Message handler (from popup) ─────────────────────────────────────────────

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  switch (message.type) {
    case "status":
      handleStatus(sendResponse);
      return true; // keep channel open for async response

    case "list_profiles":
      handleListProfiles(sendResponse);
      return true;

    case "agent_run":
      handleAgentRun(message, sendResponse);
      return true;

    default:
      sendResponse({ error: "Unknown message type" });
  }
});

async function handleStatus(sendResponse) {
  ensureConnected();
  try {
    const result = await sendRequest({ tool: "list_sessions", args: {} });
    connected = true;
    const sessions = Array.isArray(result.data) ? result.data : [];
    const alive = sessions.filter(
      s => s.status === "alive" || s.status === "reconnecting" || s.status === "recovering"
    );
    sendResponse({ connected: true, sessions: alive.length });
  } catch {
    connected = false;
    sendResponse({ connected: false, sessions: 0 });
  }
}

async function handleListProfiles(sendResponse) {
  ensureConnected();
  try {
    const result = await sendRequest({ tool: "list_profiles", args: {} });
    const profiles = Array.isArray(result.data) ? result.data : [];
    sendResponse({ ok: true, profiles });
  } catch (err) {
    sendResponse({ ok: false, profiles: [], error: err.message });
  }
}

async function handleAgentRun(message, sendResponse) {
  const { goal, profile } = message;
  if (!goal || !profile) {
    sendResponse({ ok: false, error: "goal and profile are required" });
    return;
  }

  ensureConnected();
  try {
    // agent_run handles session management internally — just pass goal + profile.
    const agentResult = await sendRequest({
      tool: "agent_run",
      args: { goal, profile }
    });
    sendResponse({ ok: true, result: agentResult });
  } catch (err) {
    sendResponse({ ok: false, error: err.message });
  }
}

// ── Startup ───────────────────────────────────────────────────────────────────

connect();
