// Pagerunner sidepanel logic.

const statusDot   = document.getElementById("statusDot");
const statusText  = document.getElementById("statusText");
const profileSel  = document.getElementById("profileSelect");
const goalInput   = document.getElementById("goalInput");
const runBtn      = document.getElementById("runBtn");
const runBtnText  = document.getElementById("runBtnText");
const feed        = document.getElementById("feed");
const feedInner   = document.getElementById("feedInner");
const welcome     = document.getElementById("welcome");
const tokenInfo   = document.getElementById("tokenInfo");

let isRunning = false;

// ── Helpers ──────────────────────────────────────────────────────────────────

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
      ? "Connected · 1 session"
      : `Connected · ${sessionCount} sessions`;
  } else {
    statusText.textContent = "Disconnected";
  }
}

function addEvent(html, cls) {
  if (welcome) welcome.hidden = true;
  const el = document.createElement("div");
  el.className = "event-card " + (cls || "");
  el.innerHTML = html;
  feedInner.appendChild(el);
  feed.scrollTop = feed.scrollHeight;
  return el;
}

function clearEvents() {
  // Remove all event cards but keep welcome
  const cards = feedInner.querySelectorAll(".event-card");
  cards.forEach(c => c.remove());
}

function syncRunBtn() {
  const hasGoal    = goalInput.value.trim().length > 0;
  const hasProfile = profileSel.value.length > 0;
  runBtn.disabled  = isRunning ? false : !(hasGoal && hasProfile);
}

function updateRunBtn() {
  if (isRunning) {
    runBtnText.textContent = "Stop";
    runBtn.classList.add("running");
  } else {
    runBtnText.textContent = "Send";
    runBtn.classList.remove("running");
  }
}

// ── Init ─────────────────────────────────────────────────────────────────────

async function init() {
  const status = await sendMsg({ type: "status" });
  setStatus(status.connected, status.sessions || 0);

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

  goalInput.addEventListener("input", syncRunBtn);
  profileSel.addEventListener("change", syncRunBtn);

  // Cmd+Enter to send
  goalInput.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      if (!runBtn.disabled) runBtn.click();
    }
  });

  syncRunBtn();
}

// ── Run / Stop ───────────────────────────────────────────────────────────────

runBtn.addEventListener("click", async () => {
  if (isRunning) {
    isRunning = false;
    updateRunBtn();
    addEvent("Stopped by user", "status");
    return;
  }

  const goal    = goalInput.value.trim();
  const profile = profileSel.value;
  if (!goal || !profile) return;

  isRunning = true;
  updateRunBtn();
  clearEvents();

  // Show the goal as a "user message"
  addEvent(`<strong>Goal:</strong> ${escapeHtml(goal)}`, "thinking");

  // Show working indicator
  const workingEl = addEvent('<span class="spinner"></span> Agent is working…', "status");

  try {
    const resp = await sendMsg({ type: "agent_run", goal, profile });

    // Remove working indicator
    workingEl.remove();

    if (resp.ok) {
      const result = resp.result || {};
      const summary = result.summary || result.data?.summary || "Done.";
      const steps = result.total_steps || result.data?.total_steps || "?";
      const inputTokens = result.input_tokens || result.data?.input_tokens || 0;
      const outputTokens = result.output_tokens || result.data?.output_tokens || 0;
      const outcome = result.outcome || result.data?.outcome || "completed";

      if (outcome === "completed" || outcome === "Completed") {
        // Result card
        addEvent(`
          <div class="result-header">✓ Result</div>
          <div class="result-body">${escapeHtml(summary)}</div>
          <div class="result-footer">
            <span>${steps} steps · ${formatTokens(inputTokens + outputTokens)}</span>
            <button class="copy-btn" onclick="copyText(this, '${escapeAttr(summary)}')">Copy</button>
          </div>
        `, "result");
      } else {
        addEvent(`⚠ ${escapeHtml(outcome)}: ${escapeHtml(summary || "")}`, "error-msg");
      }

      tokenInfo.textContent = `${steps} steps · ${formatTokens(inputTokens + outputTokens)}`;
    } else {
      addEvent(`✗ ${escapeHtml(resp.error || "Unknown error")}`, "error-msg");
    }
  } catch (err) {
    workingEl.remove();
    addEvent(`✗ ${escapeHtml(err.message)}`, "error-msg");
  } finally {
    isRunning = false;
    updateRunBtn();
    syncRunBtn();
    goalInput.value = "";
    goalInput.focus();
  }
});

// ── Utils ────────────────────────────────────────────────────────────────────

function escapeHtml(str) {
  const div = document.createElement("div");
  div.textContent = str;
  return div.innerHTML.replace(/\n/g, "<br>");
}

function escapeAttr(str) {
  return str.replace(/'/g, "\\'").replace(/\n/g, "\\n");
}

function formatTokens(n) {
  return n >= 1000 ? Math.round(n / 1000) + "K tokens" : n + " tokens";
}

// Global: copy button handler
window.copyText = function(btn, text) {
  const decoded = text.replace(/\\n/g, "\n").replace(/\\'/g, "'");
  navigator.clipboard.writeText(decoded).then(() => {
    btn.textContent = "Copied!";
    setTimeout(() => { btn.textContent = "Copy"; }, 1500);
  });
};

// ── Boot ─────────────────────────────────────────────────────────────────────

init();
