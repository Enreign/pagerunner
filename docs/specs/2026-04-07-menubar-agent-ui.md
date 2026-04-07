# Menu Bar Agent UI — Design Spec

> Date: 2026-04-07
> Status: Design approved

## 1. Overview

Add an Agent tab to the existing Pagerunner menu bar app. Users type a goal, the agent browses autonomously using a cheap LLM (Haiku), and the feed narrates what's happening in real-time. Supports quick one-off tasks and longer monitored sessions.

## 2. Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Surface | Inline tab in existing popover | Fits existing pattern, no new windows for v1 |
| Tab icon | 🤖 Agent | Playful, clear branding |
| Empty state | Centered input + recent history | Intentional "start something" surface |
| Running state | Narrated event feed + bottom input | Follow-ups while watching |
| History | Daemon KV store | Persists across restarts, accessible from CLI |
| Approval | Inline + macOS notification | Inline when popover open, notification when closed |
| Branding | "Pagerunner Agent" header | Clearly not Claude Code or other agents |

## 3. Views

### 3.1 Agent Tab in Navigation Strip

The 🤖 tab appears as the last item in the navigation strip, after all profile tabs. It has a subtle pulsing indicator when the agent is running.

```
┌──────┬──────┬──────┬────────┐
│ Over │ 🔵 S │ 🟢 W │ 🤖 Agt │
│ view │ tas  │ ork  │    ◦   │
└──────┴──────┴──────┴────────┘
                         ^
                   pulsing dot
                   when running
```

- Idle: `🤖 Agt` with no indicator
- Running: `🤖` with animated pulsing dot (◦), accent colored
- Waiting for approval: `🤖` with warning-colored dot

### 3.2 Idle State (No Agent Running)

Shown when the agent tab is selected and no task is active.

```
┌─────────────────────────────────────┐
│ ● Pagerunner    2 sessions  5 tabs  │
├──────┬──────┬──────┬────────┬───────┤
│ Over │ 🔵 S │ 🟢 W │ 🤖 Agt │       │
├──────┴──────┴──────┴────────┴───────┤
│                                     │
│                                     │
│              🤖                     │
│     Pagerunner Agent                │
│                                     │
│   ┌───────────────────────────┐     │
│   │                           │     │
│   │ What should I browse?     │     │
│   │                           │     │
│   └───────────────────────────┘     │
│                                     │
│  Profile: [stasshy392 ▾]   [Run ▶] │
│  Mode:    [Supervised ▾]            │
│                                     │
│  ─────────────────────────────      │
│  Recent                             │
│   ↻ Check my AWS bill        0:42  │
│   ↻ HN top stories           0:18  │
│   ↻ Check deploy status      1:05  │
│                                     │
│  Model: claude-haiku-4-5 · 3 steps  │
│                                     │
├─────────────────────────────────────┤
│ ⚙ Settings              ⏻ Quit     │
└─────────────────────────────────────┘
```

**Elements:**

- **Logo + title**: 🤖 icon + "Pagerunner Agent" text, centered
- **Goal input**: Multi-line text field, placeholder "What should I browse?", expands up to 3 lines
- **Profile picker**: Dropdown of available profiles (default: first personal profile or last used)
- **Mode picker**: Dropdown with three approval modes (see §3.7). Default: Supervised
- **Run button**: Enabled when input is non-empty. Keyboard shortcut: Cmd+Enter
- **Recent list**: Last 10 goals from KV store, tapping one fills the input. Shows duration of last run. Clicking the ↻ icon re-runs immediately.
- **Model badge**: Small muted text showing which LLM model + average step count from history

### 3.3 Running State (Agent Active)

Shown while the agent is executing a goal.

```
┌─────────────────────────────────────┐
│ ● Pagerunner    2 sessions  5 tabs  │
├──────┬──────┬──────┬────────┬───────┤
│ Over │ 🔵 S │ 🟢 W │ 🤖  ◦  │       │
├──────┴──────┴──────┴────────┴───────┤
│                                     │
│  🤖 Pagerunner Agent                │
│  Using claude-haiku-4-5             │
│  Profile: stasshy392                │
│  ─────────────────────────────      │
│                                     │
│  💭 I'll navigate to AWS and        │
│     check your billing dashboard.   │
│                                     │
│  ▸ navigate aws.amazon.com          │
│  ✓ Page loaded (1.2s)               │
│                                     │
│  ▸ get_content                      │
│  ✓ Extracted 2.4K chars             │
│                                     │
│  💭 I can see your March bill.      │
│     Total is $142.30. Let me        │
│     get the service breakdown...    │
│                                     │
│  ▸ click "#bill-details"            │
│  ⏳ Running...                      │
│                                     │
├─────────────────────────────────────┤
│  Step 4/15 · 11K tokens     [Stop] │
├─────────────────────────────────────┤
│ ⚙ Settings              ⏻ Quit     │
└─────────────────────────────────────┘
```

**Elements:**

- **Header**: 🤖 Pagerunner Agent + model name + profile name
- **Event feed**: Auto-scrolling list of events, each styled by type:
  - `💭` **Thinking** — agent's reasoning, regular text, full paragraph
  - `▸` **Tool call** — tool name + key args, muted/monospace
  - `✓` **Tool result (ok)** — green check, brief summary (truncated)
  - `✗` **Tool result (error)** — red X, error message
  - `⏳` **Running** — animated spinner on the current tool call
- **Status bar**: Step count / max, token count, [Stop] button
- **Stop button**: Sends interrupt, agent wraps up gracefully
- **Auto-scroll**: Feed scrolls to bottom as events arrive. Scrolling up pauses auto-scroll; new-event indicator appears at bottom to resume.

### 3.4 Approval State

When the agent needs approval for a tool call, an inline card appears in the feed.

```
│  💭 I need to fill the login form   │
│     with your credentials.          │
│                                     │
│  ┌─────────────────────────────┐    │
│  │ ⚠ Approval Required         │    │
│  │                             │    │
│  │ fill on aws.amazon.com      │    │
│  │ selector: #username         │    │
│  │ value: stas@example.com     │    │
│  │                             │    │
│  │   [Approve]      [Deny]    │    │
│  └─────────────────────────────┘    │
│                                     │
│  ⏸ Waiting for approval...          │
```

- Card has a warning-colored border
- Shows tool name, target site, and key args
- Approve/Deny buttons
- Status bar shows "Waiting for approval..." instead of step count
- If popover closes while waiting, fires macOS notification with Approve/Deny actions

### 3.5 Completed State

After the agent finishes, the feed shows the summary and offers follow-up.

```
│  💭 Your AWS bill for March:        │
│     • EC2: $89.40                   │
│     • S3: $31.20                    │
│     • RDS: $21.70                   │
│     Total: $142.30                  │
│                                     │
│  ─────────────────────────────      │
│  ✓ Done · 5 steps · 28K tokens     │
│                                     │
│  ┌───────────────────────────┐ [▶]  │
│  │ Follow up...              │      │
│  └───────────────────────────┘      │
│                                     │
│  [New Goal]           [Copy Result] │
```

- **Summary**: The agent's final thinking block, displayed prominently
- **Stats line**: Steps, tokens, outcome badge
- **Follow-up input**: Text field at bottom, sends a new goal that continues the session (agent keeps context)
- **New Goal**: Clears feed, returns to idle state
- **Copy Result**: Copies the summary text to clipboard

### 3.6 Error State

```
│  ✗ LLM error: rate limited (429)    │
│                                     │
│  ─────────────────────────────      │
│  ✗ Failed · 2 steps · 8K tokens    │
│                                     │
│  [Retry]              [New Goal]    │
```

- Shows the error message from the agent
- Retry re-sends the same goal
- New Goal clears and returns to idle

### 3.7 Approval Modes

A dropdown in the idle state controls how much autonomy the agent gets. Maps directly to the existing `AutonomyPolicy`.

```
┌─────────────────────────────────────┐
│  ● Full Auto                        │
│    Agent acts freely, no approvals  │
│                                     │
│  ○ Supervised  (default)            │
│    Approve clicks, form fills,      │
│    and code execution               │
│                                     │
│  ○ Step-by-Step                     │
│    Approve every action             │
└─────────────────────────────────────┘
```

**Full Auto** — `auto_approve: ["*"]`. Agent runs without interruption. Best for trusted read-only tasks like "summarize this page."

**Supervised** (default) — Read-only tools auto-approved, interaction tools need approval:
- Auto-approve: `navigate`, `get_content`, `screenshot`, `scroll`, `list_tabs`, `list_sessions`, `list_profiles`, `new_tab`, `close_tab`
- Require approval: `click`, `fill`, `type_text`, `select`, `evaluate`, `open_session`, `close_session`

**Step-by-Step** — `require_approval: ["*"]`. Every tool call pauses for approval. Best for sensitive tasks or learning what the agent does.

The selected mode is remembered per-profile in UserDefaults (`agent.mode.<profileName>`).

## 4. Data Model

### 4.1 Agent State (in AppState)

```swift
// Add to AppState
enum AgentRunState {
    case idle
    case running(RunContext)
    case waitingApproval(RunContext, ApprovalRequest)
    case completed(RunContext, AgentSummary)
    case error(RunContext, String)
}

struct RunContext {
    let runId: String
    let goal: String
    let profile: String
    let model: String
    var events: [AgentEventItem]
    var steps: Int
    var tokens: Int
}

struct AgentEventItem: Identifiable {
    let id: UUID
    let timestamp: Date
    let kind: AgentEventKind
}

enum AgentEventKind {
    case thinking(String)
    case toolCall(name: String, args: String)
    case toolResult(name: String, ok: Bool, summary: String)
    case progress(String)
    case done(String)
    case error(String)
}

struct ApprovalRequest {
    let action: String
    let description: String
}

struct AgentSummary {
    let text: String
    let steps: Int
    let inputTokens: Int
    let outputTokens: Int
    let outcome: String
}

enum AgentMode: String, Codable, CaseIterable {
    case fullAuto = "full_auto"
    case supervised = "supervised"
    case stepByStep = "step_by_step"
}

struct RecentGoal: Codable {
    let goal: String
    let profile: String
    let timestamp: Date
    let duration: TimeInterval
    let steps: Int
    let outcome: String
}
```

### 4.2 History Storage

Uses the daemon's KV store with namespace `agent-history`:

- Key: `recent` — JSON array of last 20 `RecentGoal` entries
- Written after each completed/errored run
- Read on agent tab appear

```
pagerunner kv-get agent-history recent
```

### 4.3 Navigation

Add `.agent` case to `PanelNavigation`:

```swift
enum PanelNavigation {
    case overview
    case profile(name: String)
    case settings
    case addProfile
    case agent  // new
}
```

## 5. Daemon Communication

### 5.1 Starting a Run

The menu bar sends a `DaemonMessage::AgentRun` over the socket and reads streaming `DaemonEvent` lines until the final `DaemonResponse`:

```
→ {"type":"agent_run","id":"...","goal":"Check AWS bill","config":{...}}
← {"run_id":"abc","event":{"type":"thinking","text":"I'll navigate..."}}
← {"run_id":"abc","event":{"type":"tool_call","name":"navigate","args":{...}}}
← {"run_id":"abc","event":{"type":"tool_result","name":"navigate","result":"...","is_error":false}}
← {"run_id":"abc","event":{"type":"done","summary":"Your bill is..."}}
← {"id":"...","result":"{\"outcome\":\"completed\",...}"}
```

This requires a **long-lived socket connection** for the duration of the run — unlike the current fire-and-forget `DaemonClient.call()`. Add a new method:

```swift
// DaemonClient
func streamAgentRun(
    goal: String,
    config: AgentConfig
) -> AsyncThrowingStream<AgentStreamEvent, Error>
```

This opens a socket, sends the `AgentRun` message, then yields `AgentStreamEvent` values as lines arrive. The stream completes when the final `DaemonResponse` arrives.

### 5.2 Approval

When an `ApprovalRequired` event arrives:

```
→ {"type":"agent_approve","id":"...","run_id":"abc","approved":true}
```

Sent on the same socket connection.

### 5.3 Interrupt

```
→ {"type":"agent_interrupt","id":"...","run_id":"abc"}
```

## 6. Notifications

Uses the existing `NotificationService` infrastructure.

### New notification categories:

| Category | When | Actions |
|----------|------|---------|
| `AGENT_APPROVAL` | Agent needs approval + popover closed | Approve, Deny |
| `AGENT_DONE` | Agent completed + popover closed | View Result |
| `AGENT_ERROR` | Agent failed + popover closed | View, Retry |

Approval notification response triggers `DaemonMessage::AgentApprove` via a separate socket connection.

## 7. Voice Future-Proofing

The design supports voice integration without changes:

- **AgentEventKind** maps directly to narration (Thinking → speak, ToolCall → "Navigating to...", Done → speak summary)
- **RunContext** is a clean data model that voice can consume alongside the UI
- **The event feed is already a transcript** — voice just adds audio on top
- **Approval** works via voice ("Should I fill the form?" → "Yes") by mapping to the same approve/deny flow

When voice comes, add a 🎤 toggle to the agent header. The feed still renders; voice narrates the same events.

## 8. Implementation Scope

### In scope (v1):
- Agent tab in navigation strip with running indicator
- Idle state with goal input, profile picker, recent history
- Running state with narrated event feed
- Completed state with summary, follow-up, copy
- Error state with retry
- Approval inline cards
- Approval macOS notifications (popover closed)
- History persistence in KV store
- Stop/interrupt support
- Streaming daemon connection

### Out of scope (v2+):
- Voice toggle / narration
- Multiple concurrent agent runs
- Goal templates / saved goals
- Agent settings (model picker, autonomy config) in UI
- Keyboard shortcut to open agent directly (global hotkey)
