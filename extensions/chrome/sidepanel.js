// Pagerunner sidepanel logic.

const statusDot   = document.getElementById("statusDot");
const statusText  = document.getElementById("statusText");
const profileSel  = document.getElementById("profileSelect");
const goalInput   = document.getElementById("goalInput");
const runBtn      = document.getElementById("runBtn");
const runBtnIcon  = document.getElementById("runBtnIcon");
const feed        = document.getElementById("feed");
const feedInner   = document.getElementById("feedInner");
const welcome     = document.getElementById("welcome");
const inputMeta   = document.getElementById("inputMeta");

let isRunning = false;

// ── Helpers ──────────────────────────────────────────────────────────────────

function sendMsg(msg) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(msg, (r) => resolve(r || {}));
  });
}

function setStatus(connected, count) {
  statusDot.className = "dot " + (connected ? "connected" : "disconnected");
  statusText.textContent = connected
    ? (count === 1 ? "1 session" : `${count} sessions`)
    : "Disconnected";
}

function ev(html, cls) {
  if (welcome) welcome.hidden = true;
  const el = document.createElement("div");
  el.className = "ev " + (cls || "");
  el.innerHTML = html;
  feedInner.appendChild(el);
  feed.scrollTop = feed.scrollHeight;
  return el;
}

function clearFeed() {
  feedInner.querySelectorAll(".ev, .result-card").forEach(e => e.remove());
}

function syncBtn() {
  const ok = goalInput.value.trim().length > 0 && profileSel.value.length > 0;
  runBtn.disabled = isRunning ? false : !ok;
}

// Auto-resize textarea
goalInput.addEventListener("input", () => {
  goalInput.style.height = "auto";
  goalInput.style.height = Math.min(goalInput.scrollHeight, 80) + "px";
  syncBtn();
});

// Cmd/Ctrl+Enter to send
goalInput.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
    e.preventDefault();
    if (!runBtn.disabled) runBtn.click();
  }
});

// ── Init ─────────────────────────────────────────────────────────────────────

async function init() {
  const status = await sendMsg({ type: "status" });
  setStatus(status.connected, status.sessions || 0);

  const { ok, profiles } = await sendMsg({ type: "list_profiles" });
  if (ok && profiles?.length) {
    profileSel.innerHTML = "";
    for (const p of profiles) {
      const opt = document.createElement("option");
      opt.value = p.name;
      opt.textContent = p.display_name || p.name;
      profileSel.appendChild(opt);
    }
  } else {
    profileSel.innerHTML = '<option value="">No profiles</option>';
  }
  syncBtn();
}

// ── Run ──────────────────────────────────────────────────────────────────────

runBtn.addEventListener("click", async () => {
  if (isRunning) {
    isRunning = false;
    runBtnIcon.textContent = "↑";
    runBtn.classList.remove("running");
    ev("Stopped", "status");
    return;
  }

  const goal = goalInput.value.trim();
  const profile = profileSel.value;
  if (!goal || !profile) return;

  isRunning = true;
  runBtnIcon.textContent = "■";
  runBtn.classList.add("running");
  clearFeed();

  ev(`<strong>Goal:</strong> ${esc(goal)}`, "goal");
  const spinner = ev('<span class="spinner"></span> Working…', "status");

  goalInput.value = "";
  goalInput.style.height = "auto";
  syncBtn();

  try {
    const resp = await sendMsg({ type: "agent_run", goal, profile });
    spinner.remove();

    if (resp.ok) {
      const r = resp.result || {};
      const summary = r.summary || "Done.";
      const steps = r.total_steps || "?";
      const tokens = (r.input_tokens || 0) + (r.output_tokens || 0);
      const outcome = r.outcome || "completed";

      if (outcome.toLowerCase().includes("completed")) {
        const card = document.createElement("div");
        card.className = "result-card";
        card.innerHTML = `
          <div class="result-head">✓ Result</div>
          <div class="result-body">${esc(summary)}</div>
          <div class="result-foot">
            <span>${steps} steps · ${fmtTok(tokens)}</span>
            <button class="copy-btn" onclick="doCopy(this)">Copy</button>
          </div>
        `;
        card.dataset.summary = summary;
        feedInner.appendChild(card);
        feed.scrollTop = feed.scrollHeight;
        inputMeta.textContent = `${steps} steps · ${fmtTok(tokens)}`;
      } else {
        ev(`⚠ ${esc(outcome)}${summary ? ": " + esc(summary) : ""}`, "error");
      }
    } else {
      ev(`✗ ${esc(resp.error || "Unknown error")}`, "error");
    }
  } catch (err) {
    spinner.remove();
    ev(`✗ ${esc(err.message)}`, "error");
  } finally {
    isRunning = false;
    runBtnIcon.textContent = "↑";
    runBtn.classList.remove("running");
    syncBtn();
    goalInput.focus();
  }
});

// ── Utils ────────────────────────────────────────────────────────────────────

function esc(s) {
  const d = document.createElement("div");
  d.textContent = s;
  return d.innerHTML.replace(/\n/g, "<br>");
}

function fmtTok(n) {
  return n >= 1000 ? Math.round(n / 1000) + "K tokens" : n + " tokens";
}

window.doCopy = function(btn) {
  const card = btn.closest(".result-card");
  const text = card?.dataset?.summary || "";
  navigator.clipboard.writeText(text).then(() => {
    btn.textContent = "Copied!";
    setTimeout(() => { btn.textContent = "Copy"; }, 1500);
  });
};

// ── Boot ─────────────────────────────────────────────────────────────────────

init();
