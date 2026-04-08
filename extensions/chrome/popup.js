// Pagerunner popup logic.

const statusDot  = document.getElementById("statusDot");
const statusText = document.getElementById("statusText");
const profileSel = document.getElementById("profileSelect");
const goalInput  = document.getElementById("goalInput");
const runBtn     = document.getElementById("runBtn");
const feed       = document.getElementById("feed");
const feedInner  = document.getElementById("feedInner");

let isRunning = false;

// ── Helpers ───────────────────────────────────────────────────────────────────

function sendMsg(msg) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(msg, (response) => {
      resolve(response || {});
    });
  });
}

function setStatus(connected, sessionCount) {
  statusDot.className = "dot " + (connected ? "connected" : "disconnected");
  if (connected) {
    statusText.textContent = sessionCount === 1
      ? "1 session"
      : `${sessionCount} sessions`;
  } else {
    statusText.textContent = "Disconnected";
  }
}

function feedLine(text, cls) {
  const el = document.createElement("div");
  el.className = "feed-line" + (cls ? " " + cls : "");
  el.textContent = text;
  feedInner.appendChild(el);
  feed.scrollTop = feed.scrollHeight;
}

function clearFeed() {
  feedInner.innerHTML = "";
}

function updateRunBtn() {
  runBtn.textContent = isRunning ? "Stop" : "Run";
  runBtn.classList.toggle("running", isRunning);
}

// ── Init ──────────────────────────────────────────────────────────────────────

async function init() {
  // Check daemon status.
  const status = await sendMsg({ type: "status" });
  setStatus(status.connected, status.sessions || 0);

  // Load profiles.
  const { ok, profiles } = await sendMsg({ type: "list_profiles" });
  if (ok && profiles && profiles.length > 0) {
    profileSel.innerHTML = "";
    for (const p of profiles) {
      const opt = document.createElement("option");
      opt.value = p.name;
      opt.textContent = p.display_name || p.name;
      profileSel.appendChild(opt);
    }
    runBtn.disabled = !status.connected;
  } else {
    profileSel.innerHTML = '<option value="">No profiles found</option>';
    runBtn.disabled = true;
  }

  // Enable run button only when goal is non-empty and daemon is connected.
  goalInput.addEventListener("input", syncRunBtn);
  profileSel.addEventListener("change", syncRunBtn);
  syncRunBtn();
}

function syncRunBtn() {
  const hasGoal    = goalInput.value.trim().length > 0;
  const hasProfile = profileSel.value.length > 0;
  runBtn.disabled  = isRunning ? false : !(hasGoal && hasProfile);
}

// ── Run / Stop ────────────────────────────────────────────────────────────────

runBtn.addEventListener("click", async () => {
  if (isRunning) {
    // v1: no streaming cancel — just reset the UI.
    isRunning = false;
    updateRunBtn();
    feedLine("Stopped.", "error");
    return;
  }

  const goal    = goalInput.value.trim();
  const profile = profileSel.value;
  if (!goal || !profile) return;

  isRunning = true;
  updateRunBtn();
  feed.hidden = false;
  clearFeed();
  feedLine("Starting agent — a browser window will open…", "highlight");

  try {
    const resp = await sendMsg({ type: "agent_run", goal, profile });
    if (resp.ok) {
      const result = resp.result || {};
      const summary = result.summary || result.data?.summary || "Done.";
      const steps = result.total_steps || result.data?.total_steps || "?";
      const tokens = result.input_tokens || result.data?.input_tokens || 0;
      const outcome = result.outcome || result.data?.outcome || "completed";

      if (outcome === "completed" || outcome === "Completed") {
        feedLine("✓ Done in " + steps + " steps", "success");
        feedLine("", "");
        // Split summary into lines for better readability
        summary.split("\n").forEach(line => {
          if (line.trim()) feedLine(line, "highlight");
        });
      } else {
        feedLine("⚠ " + outcome + " (" + steps + " steps)", "error");
        if (summary) feedLine(summary, "");
      }
    } else {
      feedLine("✗ " + (resp.error || "Unknown error"), "error");
    }
  } catch (err) {
    feedLine("✗ " + err.message, "error");
  } finally {
    isRunning = false;
    updateRunBtn();
    syncRunBtn();
  }
});

// ── Boot ──────────────────────────────────────────────────────────────────────

init();
