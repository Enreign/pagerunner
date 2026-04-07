# Raycast Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Raycast extension that wraps the `pagerunner` CLI to manage browser sessions, tabs, KV store, secrets, site knowledge, and checkpoints from a keyboard-driven launcher.

**Architecture:** TypeScript/React extension using `child_process.execFile` to call the `pagerunner` binary. All commands return JSON to stdout; errors go to stderr with exit 1. Raycast's `useCachedPromise` handles caching and auto-refresh.

**Tech Stack:** TypeScript, React, `@raycast/api` ^1.93.0, `@raycast/utils` ^1.19.0, Node.js `child_process`

---

## File Map

```
extensions/raycast/
├── package.json              # manifest: commands, preferences, deps
├── tsconfig.json             # TypeScript config
├── src/
│   ├── types.ts              # shared TypeScript interfaces for CLI responses
│   ├── pagerunner.ts         # execCommand wrapper, binary resolution, error handling
│   ├── sessions.tsx          # Sessions list command
│   ├── new-session.tsx       # New Session command (profile picker + form)
│   ├── tabs.tsx              # Tabs list command (with screenshot detail)
│   ├── kv-browser.tsx        # KV Browser command
│   ├── secrets.tsx           # Secrets list command
│   ├── site-knowledge.tsx    # Site Knowledge command
│   └── checkpoints.tsx       # Checkpoints list/restore command
└── assets/
    └── icon.png              # 512x512 extension icon (placeholder)
```

---

### Task 1: Project Scaffolding

**Files:**
- Create: `extensions/raycast/package.json`
- Create: `extensions/raycast/tsconfig.json`
- Create: `extensions/raycast/assets/icon.png`

- [ ] **Step 1: Create the extensions/raycast directory**

```bash
mkdir -p extensions/raycast/src extensions/raycast/assets
```

- [ ] **Step 2: Write package.json**

Create `extensions/raycast/package.json`:

```json
{
  "$schema": "https://www.raycast.com/schemas/extension.json",
  "name": "pagerunner",
  "title": "Pagerunner",
  "description": "Manage Chrome browser sessions, tabs, KV store, and secrets from Raycast",
  "icon": "icon.png",
  "author": "enreign",
  "platforms": ["macOS"],
  "categories": ["Developer Tools"],
  "license": "Apache-2.0",
  "commands": [
    {
      "name": "sessions",
      "title": "Sessions",
      "description": "List and manage open browser sessions",
      "mode": "view"
    },
    {
      "name": "new-session",
      "title": "New Session",
      "description": "Open a new browser session from a profile",
      "mode": "view"
    },
    {
      "name": "tabs",
      "title": "Tabs",
      "description": "Browse and manage tabs in a session",
      "mode": "view"
    },
    {
      "name": "kv-browser",
      "title": "KV Browser",
      "description": "Browse, search, and edit key-value store entries",
      "mode": "view"
    },
    {
      "name": "secrets",
      "title": "Secrets",
      "description": "List and manage stored secrets",
      "mode": "view"
    },
    {
      "name": "site-knowledge",
      "title": "Site Knowledge",
      "description": "View site knowledge for an origin",
      "mode": "view"
    },
    {
      "name": "checkpoints",
      "title": "Checkpoints",
      "description": "List and restore session checkpoints",
      "mode": "view"
    }
  ],
  "preferences": [
    {
      "name": "pagerunnerPath",
      "title": "Pagerunner Binary",
      "description": "Path to pagerunner binary (leave blank to auto-detect)",
      "type": "textfield",
      "required": false,
      "placeholder": "/usr/local/bin/pagerunner"
    },
    {
      "name": "daemonAutoStart",
      "title": "Auto-start Daemon",
      "description": "Start daemon automatically if not running",
      "type": "checkbox",
      "label": "Auto-start daemon",
      "required": false,
      "default": true
    }
  ],
  "dependencies": {
    "@raycast/api": "^1.93.0",
    "@raycast/utils": "^1.19.0"
  },
  "devDependencies": {
    "@raycast/eslint-config": "^1.0.11",
    "@types/node": "^22.13.14",
    "@types/react": "^19.0.12",
    "eslint": "^9.23.0",
    "typescript": "^5.8.2"
  },
  "scripts": {
    "build": "ray build",
    "dev": "ray develop",
    "lint": "ray lint"
  }
}
```

- [ ] **Step 3: Write tsconfig.json**

Create `extensions/raycast/tsconfig.json`:

```json
{
  "$schema": "https://json.schemastore.org/tsconfig",
  "display": "Raycast Extension",
  "compilerOptions": {
    "lib": ["ES2023"],
    "module": "commonjs",
    "target": "ES2023",
    "moduleResolution": "node",
    "jsx": "react-jsx",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "resolveJsonModule": true,
    "outDir": "./dist"
  },
  "include": ["src/**/*"]
}
```

- [ ] **Step 4: Create a placeholder icon**

Create a 512x512 placeholder PNG at `extensions/raycast/assets/icon.png`. Use a simple solid-color square:

```bash
cd extensions/raycast
# Create a 1x1 magenta pixel PNG as placeholder (will be replaced with real icon)
printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\x0f\x00\x00\x01\x01\x00\x05\x18\xd8N\x00\x00\x00\x00IEND\xaeB`\x82' > assets/icon.png
```

- [ ] **Step 5: Install dependencies**

```bash
cd extensions/raycast && npm install
```

- [ ] **Step 6: Verify build setup**

```bash
cd extensions/raycast && npx ray build
```

Expected: Build succeeds (with warnings about missing command files — that's fine, we'll add them next).

- [ ] **Step 7: Commit**

```bash
git add extensions/raycast/package.json extensions/raycast/tsconfig.json extensions/raycast/assets/icon.png
git commit -m "feat(raycast): scaffold extension project"
```

---

### Task 2: Types and CLI Wrapper

**Files:**
- Create: `extensions/raycast/src/types.ts`
- Create: `extensions/raycast/src/pagerunner.ts`

- [ ] **Step 1: Write types.ts**

Create `extensions/raycast/src/types.ts`:

```typescript
// --- Preferences ---

export interface Preferences {
  pagerunnerPath: string;
  daemonAutoStart: boolean;
}

// --- CLI Response Types ---

export interface Session {
  id: string;
  profile: string;
  display_name: string;
  stealth: boolean;
  status: "alive" | "crashed";
}

export interface Profile {
  name: string;
  display_name: string;
  kind: string;
  user_data_dir: string;
}

export interface Tab {
  target_id: string;
  url: string;
  title: string;
}

export interface KvEntry {
  key: string;
  value?: string;
}

export interface Checkpoint {
  checkpoint_id: string;
  name: string;
  saved_at: number;
  profile: string;
  tab_count: number;
  origins: string[];
}

export interface SiteKnowledge {
  origin: string;
  adapters: Record<string, {
    description: string;
    trusted: boolean;
    js_code: string;
    params_schema: unknown;
    last_used: number;
  }>;
  selectors: Array<{
    selector: string;
    successes: number;
    failures: number;
    reliability: number;
  }>;
  auth_tokens: Record<string, string>;
  endpoints: Array<{
    key: string;
    method: string;
    path_pattern: string;
    api_kind: string;
    crud_op: string | null;
    observations: number;
    has_schema: boolean;
  }>;
}

// --- CLI Response Envelopes ---

export interface OkResponse<T> {
  ok: true;
  data: T;
}

export interface SecretsResponse {
  ok: true;
  secrets: string[];
}

export interface KvGetResponse {
  ok: true;
  key: string;
  value: string | null;
}

export interface OpenSessionResponse {
  ok: true;
  session_id: string;
  stealth: boolean;
}

export interface CloseSessionResponse {
  ok: true;
  session_id: string;
}

export interface ScreenshotResponse {
  base64: string;
}
```

- [ ] **Step 2: Write pagerunner.ts**

Create `extensions/raycast/src/pagerunner.ts`:

```typescript
import { execFile } from "child_process";
import { promisify } from "util";
import { getPreferenceValues, showToast, Toast } from "@raycast/api";
import type { Preferences } from "./types";

const execFileAsync = promisify(execFile);

function getBinaryPath(): string {
  const prefs = getPreferenceValues<Preferences>();
  return prefs.pagerunnerPath || "pagerunner";
}

export async function execCommand(subcommand: string, args: string[] = []): Promise<string> {
  const bin = getBinaryPath();
  try {
    const { stdout } = await execFileAsync(bin, [subcommand, ...args], {
      timeout: 30_000,
      maxBuffer: 10 * 1024 * 1024,
    });
    return stdout;
  } catch (error: unknown) {
    const err = error as { stderr?: string; message?: string };
    const message = err.stderr?.trim() || err.message || "Unknown error";
    await showToast({ style: Toast.Style.Failure, title: `pagerunner ${subcommand} failed`, message });
    throw new Error(message);
  }
}

export async function execCommandJson<T>(subcommand: string, args: string[] = []): Promise<T> {
  const stdout = await execCommand(subcommand, args);
  return JSON.parse(stdout) as T;
}

export async function ensureDaemon(): Promise<void> {
  const prefs = getPreferenceValues<Preferences>();
  if (!prefs.daemonAutoStart) return;

  try {
    // list-sessions succeeds if daemon is running
    await execCommand("list-sessions");
  } catch {
    const bin = getBinaryPath();
    // Start daemon in background
    const { spawn } = await import("child_process");
    const child = spawn(bin, ["daemon"], {
      detached: true,
      stdio: "ignore",
    });
    child.unref();
    // Give daemon a moment to start
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}
```

- [ ] **Step 3: Verify the files compile**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Type-checks pass (may warn about unused exports — that's fine).

- [ ] **Step 4: Commit**

```bash
git add extensions/raycast/src/types.ts extensions/raycast/src/pagerunner.ts
git commit -m "feat(raycast): add types and CLI wrapper"
```

---

### Task 3: Sessions Command

**Files:**
- Create: `extensions/raycast/src/sessions.tsx`

- [ ] **Step 1: Write sessions.tsx**

Create `extensions/raycast/src/sessions.tsx`:

```typescript
import { List, ActionPanel, Action, Icon, Color, showToast, Toast } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
import type { Session, OkResponse } from "./types";
import TabsCommand from "./tabs";

export default function SessionsCommand() {
  const { data, isLoading, revalidate, mutate } = useCachedPromise(
    async () => {
      await ensureDaemon();
      const resp = await execCommandJson<OkResponse<Session[]>>("list-sessions");
      return resp.data;
    },
    [],
    { failureToastOptions: { title: "Failed to list sessions" } },
  );

  return (
    <List isLoading={isLoading} searchBarPlaceholder="Search sessions...">
      <List.EmptyView title="No Sessions" description="Open a new session to get started" icon={Icon.Globe} />
      {data?.map((session) => (
        <List.Item
          key={session.id}
          title={session.display_name || session.profile}
          subtitle={session.id}
          icon={session.status === "alive" ? { source: Icon.Circle, tintColor: Color.Green } : { source: Icon.Circle, tintColor: Color.Red }}
          accessories={[
            session.stealth ? { icon: Icon.EyeDisabled, tooltip: "Stealth" } : {},
            { text: session.status },
          ]}
          actions={
            <ActionPanel>
              <Action.Push title="View Tabs" icon={Icon.List} target={<TabsCommand sessionId={session.id} />} />
              <Action
                title="Save Checkpoint"
                icon={Icon.Download}
                shortcut={{ modifiers: ["cmd"], key: "s" }}
                onAction={async () => {
                  await execCommand("save-session-checkpoint", [session.id]);
                  await showToast({ style: Toast.Style.Success, title: "Checkpoint saved" });
                }}
              />
              <Action.CopyToClipboard title="Copy Session ID" content={session.id} shortcut={{ modifiers: ["cmd", "shift"], key: "c" }} />
              <Action
                title="Close Session"
                icon={Icon.Trash}
                style={Action.Style.Destructive}
                shortcut={{ modifiers: ["ctrl"], key: "x" }}
                onAction={async () => {
                  await mutate(execCommand("close-session", [session.id]), {
                    optimisticUpdate: (current) => current?.filter((s) => s.id !== session.id),
                  });
                  await showToast({ style: Toast.Style.Success, title: "Session closed" });
                }}
              />
            </ActionPanel>
          }
        />
      ))}
    </List>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Fails because `tabs.tsx` doesn't exist yet. That's expected — we'll create a stub.

- [ ] **Step 3: Create a minimal tabs.tsx stub so sessions.tsx compiles**

Create `extensions/raycast/src/tabs.tsx`:

```typescript
import { List } from "@raycast/api";

export default function TabsCommand({ sessionId }: { sessionId?: string }) {
  return <List><List.EmptyView title={`Tabs for ${sessionId || "..."}`} /></List>;
}
```

- [ ] **Step 4: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass (with stubs for remaining commands needed).

- [ ] **Step 5: Create stubs for remaining commands so the build passes**

Create each of these minimal stub files so the overall build works. Each exports a default component returning an empty `<List>`:

`extensions/raycast/src/new-session.tsx`:
```typescript
import { List } from "@raycast/api";
export default function NewSessionCommand() {
  return <List><List.EmptyView title="New Session" /></List>;
}
```

`extensions/raycast/src/kv-browser.tsx`:
```typescript
import { List } from "@raycast/api";
export default function KvBrowserCommand() {
  return <List><List.EmptyView title="KV Browser" /></List>;
}
```

`extensions/raycast/src/secrets.tsx`:
```typescript
import { List } from "@raycast/api";
export default function SecretsCommand() {
  return <List><List.EmptyView title="Secrets" /></List>;
}
```

`extensions/raycast/src/site-knowledge.tsx`:
```typescript
import { List } from "@raycast/api";
export default function SiteKnowledgeCommand() {
  return <List><List.EmptyView title="Site Knowledge" /></List>;
}
```

`extensions/raycast/src/checkpoints.tsx`:
```typescript
import { List } from "@raycast/api";
export default function CheckpointsCommand() {
  return <List><List.EmptyView title="Checkpoints" /></List>;
}
```

- [ ] **Step 6: Verify full build**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 7: Commit**

```bash
git add extensions/raycast/src/
git commit -m "feat(raycast): add sessions command + stubs for all commands"
```

---

### Task 4: New Session Command

**Files:**
- Modify: `extensions/raycast/src/new-session.tsx`

- [ ] **Step 1: Implement new-session.tsx**

Replace `extensions/raycast/src/new-session.tsx` with:

```typescript
import { List, ActionPanel, Action, Icon, showToast, Toast, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
import type { Profile, OkResponse, OpenSessionResponse } from "./types";
import TabsCommand from "./tabs";

export default function NewSessionCommand() {
  const { push } = useNavigation();

  const { data, isLoading } = useCachedPromise(
    async () => {
      await ensureDaemon();
      const resp = await execCommandJson<OkResponse<Profile[]>>("list-profiles");
      return resp.data;
    },
    [],
    { failureToastOptions: { title: "Failed to list profiles" } },
  );

  async function openSession(profileName: string, stealth: boolean, anonymize: boolean) {
    const args = [profileName];
    if (stealth) args.push("--stealth");
    if (anonymize) args.push("--anonymize");

    const toast = await showToast({ style: Toast.Style.Animated, title: "Opening session..." });
    const resp = await execCommandJson<OpenSessionResponse>("open-session", args);
    toast.style = Toast.Style.Success;
    toast.title = "Session opened";
    toast.message = resp.session_id;

    push(<TabsCommand sessionId={resp.session_id} />);
  }

  return (
    <List isLoading={isLoading} searchBarPlaceholder="Select a profile...">
      <List.EmptyView title="No Profiles" description="Add profiles to ~/.pagerunner/config.toml" icon={Icon.Person} />
      {data?.map((profile) => (
        <List.Item
          key={profile.name}
          title={profile.display_name || profile.name}
          subtitle={profile.kind}
          icon={Icon.Person}
          accessories={[{ text: profile.kind }]}
          actions={
            <ActionPanel>
              <Action title="Open Session" icon={Icon.Globe} onAction={() => openSession(profile.name, false, false)} />
              <Action
                title="Open Stealth Session"
                icon={Icon.EyeDisabled}
                shortcut={{ modifiers: ["cmd"], key: "s" }}
                onAction={() => openSession(profile.name, true, false)}
              />
              <Action
                title="Open Anonymized Session"
                icon={Icon.Shield}
                shortcut={{ modifiers: ["cmd"], key: "a" }}
                onAction={() => openSession(profile.name, false, true)}
              />
            </ActionPanel>
          }
        />
      ))}
    </List>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add extensions/raycast/src/new-session.tsx
git commit -m "feat(raycast): implement new session command"
```

---

### Task 5: Tabs Command with Screenshot

**Files:**
- Modify: `extensions/raycast/src/tabs.tsx`

- [ ] **Step 1: Implement tabs.tsx**

Replace `extensions/raycast/src/tabs.tsx` with:

```typescript
import { List, ActionPanel, Action, Icon, Detail, Form, showToast, Toast, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand } from "./pagerunner";
import type { Tab, Session, OkResponse, ScreenshotResponse } from "./types";
import { environment } from "@raycast/api";
import { writeFileSync } from "fs";
import path from "path";
import { useState } from "react";

function NavigateForm({ sessionId, targetId, onDone }: { sessionId: string; targetId: string; onDone: () => void }) {
  const { pop } = useNavigation();
  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Navigate"
            onSubmit={async (values: { url: string }) => {
              await execCommand("navigate", [sessionId, targetId, values.url]);
              await showToast({ style: Toast.Style.Success, title: "Navigated" });
              onDone();
              pop();
            }}
          />
        </ActionPanel>
      }
    >
      <Form.TextField id="url" title="URL" placeholder="https://example.com" />
    </Form>
  );
}

function ScreenshotView({ sessionId, targetId }: { sessionId: string; targetId: string }) {
  const { data, isLoading } = useCachedPromise(
    async (sid: string, tid: string) => {
      const resp = await execCommandJson<ScreenshotResponse>("screenshot", [sid, tid, "--base64"]);
      const buffer = Buffer.from(resp.base64, "base64");
      const filePath = path.join(environment.supportPath, `screenshot-${tid}.png`);
      writeFileSync(filePath, buffer);
      return filePath;
    },
    [sessionId, targetId],
  );

  if (isLoading) {
    return <Detail isLoading markdown="" />;
  }

  return (
    <Detail
      markdown={data ? `![Screenshot](file://${data})` : "Failed to take screenshot"}
      actions={
        <ActionPanel>
          {data && <Action.Open title="Open in Preview" target={data} />}
        </ActionPanel>
      }
    />
  );
}

function ContentView({ sessionId, targetId }: { sessionId: string; targetId: string }) {
  const { data, isLoading } = useCachedPromise(
    async (sid: string, tid: string) => {
      const resp = await execCommandJson<OkResponse<string>>("get-content", [sid, tid]);
      return resp.data;
    },
    [sessionId, targetId],
  );

  return (
    <Detail
      isLoading={isLoading}
      markdown={data ? `\`\`\`\n${data}\n\`\`\`` : ""}
      actions={
        <ActionPanel>
          {data && <Action.CopyToClipboard title="Copy Content" content={data} />}
        </ActionPanel>
      }
    />
  );
}

export default function TabsCommand({ sessionId: initialSessionId }: { sessionId?: string }) {
  const [selectedSession, setSelectedSession] = useState<string>(initialSessionId || "");

  const { data: sessions } = useCachedPromise(
    async () => {
      const resp = await execCommandJson<OkResponse<Session[]>>("list-sessions");
      return resp.data;
    },
    [],
    { execute: !initialSessionId },
  );

  const { data: tabs, isLoading, revalidate, mutate } = useCachedPromise(
    async (sid: string) => {
      const resp = await execCommandJson<OkResponse<Tab[]>>("list-tabs", [sid]);
      return resp.data;
    },
    [selectedSession],
    { execute: !!selectedSession, failureToastOptions: { title: "Failed to list tabs" } },
  );

  return (
    <List
      isLoading={isLoading}
      searchBarPlaceholder="Search tabs..."
      searchBarAccessory={
        !initialSessionId && sessions ? (
          <List.Dropdown tooltip="Select Session" onChange={setSelectedSession}>
            <List.Dropdown.Item title="Select a session..." value="" />
            {sessions.map((s) => (
              <List.Dropdown.Item key={s.id} title={s.display_name || s.profile} value={s.id} />
            ))}
          </List.Dropdown>
        ) : undefined
      }
    >
      {!selectedSession ? (
        <List.EmptyView title="Select a Session" description="Choose a session from the dropdown above" icon={Icon.Globe} />
      ) : (
        <>
          <List.EmptyView title="No Tabs" icon={Icon.Window} />
          {tabs?.map((tab) => (
            <List.Item
              key={tab.target_id}
              title={tab.title || "Untitled"}
              subtitle={tab.url}
              icon={Icon.Window}
              actions={
                <ActionPanel>
                  <Action.Push
                    title="Screenshot"
                    icon={Icon.Camera}
                    target={<ScreenshotView sessionId={selectedSession} targetId={tab.target_id} />}
                  />
                  <Action.Push
                    title="View Content"
                    icon={Icon.Document}
                    target={<ContentView sessionId={selectedSession} targetId={tab.target_id} />}
                  />
                  <Action.Push
                    title="Navigate to URL"
                    icon={Icon.Link}
                    shortcut={{ modifiers: ["cmd"], key: "g" }}
                    target={<NavigateForm sessionId={selectedSession} targetId={tab.target_id} onDone={revalidate} />}
                  />
                  <Action.OpenInBrowser title="Open URL" url={tab.url} />
                  <Action.CopyToClipboard title="Copy URL" content={tab.url} shortcut={{ modifiers: ["cmd", "shift"], key: "c" }} />
                  <Action
                    title="Close Tab"
                    icon={Icon.Trash}
                    style={Action.Style.Destructive}
                    shortcut={{ modifiers: ["ctrl"], key: "x" }}
                    onAction={async () => {
                      await mutate(execCommand("close-tab", [selectedSession, tab.target_id]), {
                        optimisticUpdate: (current) => current?.filter((t) => t.target_id !== tab.target_id),
                      });
                      await showToast({ style: Toast.Style.Success, title: "Tab closed" });
                    }}
                  />
                </ActionPanel>
              }
            />
          ))}
        </>
      )}
    </List>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add extensions/raycast/src/tabs.tsx
git commit -m "feat(raycast): implement tabs command with screenshot and content views"
```

---

### Task 6: KV Browser Command

**Files:**
- Modify: `extensions/raycast/src/kv-browser.tsx`

- [ ] **Step 1: Implement kv-browser.tsx**

Replace `extensions/raycast/src/kv-browser.tsx` with:

```typescript
import { List, ActionPanel, Action, Icon, Detail, Form, showToast, Toast, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand } from "./pagerunner";
import type { KvEntry, OkResponse, KvGetResponse } from "./types";
import { useState } from "react";

function KvValueView({ namespace, kvKey }: { namespace: string; kvKey: string }) {
  const { data, isLoading } = useCachedPromise(
    async (ns: string, k: string) => {
      const resp = await execCommandJson<KvGetResponse>("kv-get", [ns, k]);
      return resp.value;
    },
    [namespace, kvKey],
  );

  const markdown = data !== null && data !== undefined
    ? `## ${kvKey}\n\n\`\`\`\n${data}\n\`\`\``
    : `## ${kvKey}\n\n*Key not found*`;

  return (
    <Detail
      isLoading={isLoading}
      markdown={markdown}
      actions={
        <ActionPanel>
          {data && <Action.CopyToClipboard title="Copy Value" content={data} />}
        </ActionPanel>
      }
    />
  );
}

function KvSetForm({ namespace, kvKey, currentValue, onSaved }: { namespace: string; kvKey?: string; currentValue?: string; onSaved: () => void }) {
  const { pop } = useNavigation();

  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Save"
            onSubmit={async (values: { key: string; value: string }) => {
              await execCommand("kv-set", [namespace, values.key, values.value]);
              await showToast({ style: Toast.Style.Success, title: "Key saved" });
              onSaved();
              pop();
            }}
          />
        </ActionPanel>
      }
    >
      <Form.TextField id="key" title="Key" defaultValue={kvKey || ""} />
      <Form.TextArea id="value" title="Value" defaultValue={currentValue || ""} />
    </Form>
  );
}

export default function KvBrowserCommand() {
  const [namespace, setNamespace] = useState<string>("");
  const [prefix, setPrefix] = useState<string>("");

  const { data, isLoading, revalidate, mutate } = useCachedPromise(
    async (ns: string, pfx: string) => {
      const args = [ns];
      if (pfx) args.push("--prefix", pfx);
      const resp = await execCommandJson<OkResponse<KvEntry[]>>("kv-list", args);
      return resp.data;
    },
    [namespace, prefix],
    { execute: !!namespace, failureToastOptions: { title: "Failed to list keys" } },
  );

  return (
    <List
      isLoading={isLoading}
      searchBarPlaceholder="Filter keys..."
      onSearchTextChange={setPrefix}
      throttle
    >
      {!namespace ? (
        <>
          <List.EmptyView
            title="Enter a Namespace"
            description="Type a namespace name and press Enter"
            icon={Icon.HardDrive}
          />
          <List.Item
            title="Enter namespace..."
            icon={Icon.TextInput}
            actions={
              <ActionPanel>
                <Action
                  title="Set Namespace"
                  onAction={() => {
                    // The search text becomes the namespace
                  }}
                />
              </ActionPanel>
            }
          />
        </>
      ) : (
        <>
          <List.EmptyView title="No Keys" description={`No keys in namespace "${namespace}"`} icon={Icon.HardDrive} />
          {data?.map((entry) => (
            <List.Item
              key={entry.key}
              title={entry.key}
              subtitle={entry.value ? (entry.value.length > 80 ? entry.value.slice(0, 80) + "..." : entry.value) : ""}
              icon={Icon.Key}
              actions={
                <ActionPanel>
                  <Action.Push
                    title="View Value"
                    icon={Icon.Eye}
                    target={<KvValueView namespace={namespace} kvKey={entry.key} />}
                  />
                  {entry.value && <Action.CopyToClipboard title="Copy Value" content={entry.value} shortcut={{ modifiers: ["cmd", "shift"], key: "c" }} />}
                  <Action.Push
                    title="Edit"
                    icon={Icon.Pencil}
                    shortcut={{ modifiers: ["cmd"], key: "e" }}
                    target={<KvSetForm namespace={namespace} kvKey={entry.key} currentValue={entry.value} onSaved={revalidate} />}
                  />
                  <Action.Push
                    title="New Key"
                    icon={Icon.Plus}
                    shortcut={{ modifiers: ["cmd"], key: "n" }}
                    target={<KvSetForm namespace={namespace} onSaved={revalidate} />}
                  />
                  <Action
                    title="Delete Key"
                    icon={Icon.Trash}
                    style={Action.Style.Destructive}
                    shortcut={{ modifiers: ["ctrl"], key: "x" }}
                    onAction={async () => {
                      await mutate(execCommand("kv-delete", [namespace, entry.key]), {
                        optimisticUpdate: (current) => current?.filter((e) => e.key !== entry.key),
                      });
                      await showToast({ style: Toast.Style.Success, title: "Key deleted" });
                    }}
                  />
                </ActionPanel>
              }
            />
          ))}
        </>
      )}
    </List>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add extensions/raycast/src/kv-browser.tsx
git commit -m "feat(raycast): implement KV browser command"
```

---

### Task 7: Secrets Command

**Files:**
- Modify: `extensions/raycast/src/secrets.tsx`

- [ ] **Step 1: Implement secrets.tsx**

Replace `extensions/raycast/src/secrets.tsx` with:

```typescript
import { List, ActionPanel, Action, Icon, Alert, confirmAlert, showToast, Toast } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand } from "./pagerunner";
import type { SecretsResponse } from "./types";

export default function SecretsCommand() {
  const { data, isLoading, mutate } = useCachedPromise(
    async () => {
      const resp = await execCommandJson<SecretsResponse>("list-secrets");
      return resp.secrets;
    },
    [],
    { failureToastOptions: { title: "Failed to list secrets" } },
  );

  return (
    <List isLoading={isLoading} searchBarPlaceholder="Search secrets...">
      <List.EmptyView title="No Secrets" description="No secrets stored" icon={Icon.Lock} />
      {data?.map((name) => (
        <List.Item
          key={name}
          title={name}
          icon={Icon.Lock}
          actions={
            <ActionPanel>
              <Action.CopyToClipboard title="Copy Secret Name" content={name} />
              <Action
                title="Delete Secret"
                icon={Icon.Trash}
                style={Action.Style.Destructive}
                shortcut={{ modifiers: ["ctrl"], key: "x" }}
                onAction={async () => {
                  const confirmed = await confirmAlert({
                    title: "Delete Secret",
                    message: `Are you sure you want to delete "${name}"?`,
                    primaryAction: { title: "Delete", style: Alert.ActionStyle.Destructive },
                  });
                  if (!confirmed) return;
                  await mutate(execCommand("delete-secret", [name]), {
                    optimisticUpdate: (current) => current?.filter((s) => s !== name),
                  });
                  await showToast({ style: Toast.Style.Success, title: "Secret deleted" });
                }}
              />
            </ActionPanel>
          }
        />
      ))}
    </List>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add extensions/raycast/src/secrets.tsx
git commit -m "feat(raycast): implement secrets command"
```

---

### Task 8: Site Knowledge Command

**Files:**
- Modify: `extensions/raycast/src/site-knowledge.tsx`

- [ ] **Step 1: Implement site-knowledge.tsx**

Replace `extensions/raycast/src/site-knowledge.tsx` with:

```typescript
import { List, ActionPanel, Action, Icon, Detail } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson } from "./pagerunner";
import type { SiteKnowledge } from "./types";
import { useState } from "react";

function SiteKnowledgeDetail({ origin }: { origin: string }) {
  const { data, isLoading } = useCachedPromise(
    async (o: string) => {
      return await execCommandJson<SiteKnowledge>("get-site-knowledge", [o]);
    },
    [origin],
  );

  if (isLoading || !data) {
    return <Detail isLoading={isLoading} markdown="" />;
  }

  const adapterCount = Object.keys(data.adapters).length;
  const selectorCount = data.selectors.length;
  const endpointCount = data.endpoints.length;

  let md = `# ${data.origin}\n\n`;

  if (adapterCount > 0) {
    md += `## Adapters (${adapterCount})\n\n`;
    for (const [name, adapter] of Object.entries(data.adapters)) {
      md += `### ${name}\n`;
      md += `${adapter.description}\n`;
      md += `- Trusted: ${adapter.trusted ? "Yes" : "No"}\n\n`;
    }
  }

  if (selectorCount > 0) {
    md += `## Selectors (${selectorCount})\n\n`;
    md += `| Selector | Reliability | Successes | Failures |\n`;
    md += `|----------|------------|-----------|----------|\n`;
    for (const s of data.selectors) {
      md += `| \`${s.selector}\` | ${(s.reliability * 100).toFixed(0)}% | ${s.successes} | ${s.failures} |\n`;
    }
    md += `\n`;
  }

  if (endpointCount > 0) {
    md += `## Endpoints (${endpointCount})\n\n`;
    md += `| Method | Path | Kind | Observations |\n`;
    md += `|--------|------|------|-------------|\n`;
    for (const e of data.endpoints) {
      md += `| ${e.method} | \`${e.path_pattern}\` | ${e.api_kind} | ${e.observations} |\n`;
    }
    md += `\n`;
  }

  return (
    <Detail
      markdown={md}
      actions={
        <ActionPanel>
          <Action.CopyToClipboard title="Copy as Markdown" content={md} />
        </ActionPanel>
      }
    />
  );
}

export default function SiteKnowledgeCommand() {
  const [origin, setOrigin] = useState<string>("");

  if (origin) {
    return <SiteKnowledgeDetail origin={origin} />;
  }

  return (
    <List
      searchBarPlaceholder="Enter an origin (e.g. https://example.com)..."
      onSearchTextChange={() => {}}
    >
      <List.EmptyView
        title="Enter an Origin"
        description="Type a URL origin and press Enter to view site knowledge"
        icon={Icon.Globe}
        actions={
          <ActionPanel>
            <Action title="Search" onAction={() => {}} />
          </ActionPanel>
        }
      />
    </List>
  );
}
```

Note: The origin input UX is intentionally simple — type origin in the search bar, pick from results. Raycast doesn't support a pure "text input then fetch" pattern cleanly, so we use a `List` with `searchBarPlaceholder` as a text prompt. The actual submission happens via an Action.

**Update:** Let's improve this with a Form-based approach for better UX:

Replace the above with:

```typescript
import { ActionPanel, Action, Icon, Detail, Form, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson } from "./pagerunner";
import type { SiteKnowledge } from "./types";

function SiteKnowledgeDetail({ origin }: { origin: string }) {
  const { data, isLoading } = useCachedPromise(
    async (o: string) => {
      return await execCommandJson<SiteKnowledge>("get-site-knowledge", [o]);
    },
    [origin],
  );

  if (isLoading || !data) {
    return <Detail isLoading={isLoading} markdown="" />;
  }

  const adapterCount = Object.keys(data.adapters).length;
  const selectorCount = data.selectors.length;
  const endpointCount = data.endpoints.length;

  let md = `# ${data.origin}\n\n`;

  if (adapterCount > 0) {
    md += `## Adapters (${adapterCount})\n\n`;
    for (const [name, adapter] of Object.entries(data.adapters)) {
      md += `### ${name}\n`;
      md += `${adapter.description}\n`;
      md += `- Trusted: ${adapter.trusted ? "Yes" : "No"}\n\n`;
    }
  }

  if (selectorCount > 0) {
    md += `## Selectors (${selectorCount})\n\n`;
    md += `| Selector | Reliability | Successes | Failures |\n`;
    md += `|----------|------------|-----------|----------|\n`;
    for (const s of data.selectors) {
      md += `| \`${s.selector}\` | ${(s.reliability * 100).toFixed(0)}% | ${s.successes} | ${s.failures} |\n`;
    }
    md += `\n`;
  }

  if (endpointCount > 0) {
    md += `## Endpoints (${endpointCount})\n\n`;
    md += `| Method | Path | Kind | Observations |\n`;
    md += `|--------|------|------|-------------|\n`;
    for (const e of data.endpoints) {
      md += `| ${e.method} | \`${e.path_pattern}\` | ${e.api_kind} | ${e.observations} |\n`;
    }
    md += `\n`;
  }

  return (
    <Detail
      markdown={md}
      actions={
        <ActionPanel>
          <Action.CopyToClipboard title="Copy as Markdown" content={md} />
        </ActionPanel>
      }
    />
  );
}

export default function SiteKnowledgeCommand() {
  const { push } = useNavigation();

  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Look Up"
            icon={Icon.MagnifyingGlass}
            onSubmit={(values: { origin: string }) => {
              push(<SiteKnowledgeDetail origin={values.origin} />);
            }}
          />
        </ActionPanel>
      }
    >
      <Form.TextField id="origin" title="Origin" placeholder="https://example.com" />
    </Form>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add extensions/raycast/src/site-knowledge.tsx
git commit -m "feat(raycast): implement site knowledge command"
```

---

### Task 9: Checkpoints Command

**Files:**
- Modify: `extensions/raycast/src/checkpoints.tsx`

- [ ] **Step 1: Implement checkpoints.tsx**

Replace `extensions/raycast/src/checkpoints.tsx` with:

```typescript
import { List, ActionPanel, Action, Icon, showToast, Toast } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand } from "./pagerunner";
import type { Checkpoint, Session, Profile, OkResponse } from "./types";
import { useState } from "react";

export default function CheckpointsCommand() {
  const [selectedProfile, setSelectedProfile] = useState<string>("");

  const { data: profiles } = useCachedPromise(
    async () => {
      const resp = await execCommandJson<OkResponse<Profile[]>>("list-profiles");
      return resp.data;
    },
    [],
  );

  const { data: sessions } = useCachedPromise(
    async () => {
      const resp = await execCommandJson<OkResponse<Session[]>>("list-sessions");
      return resp.data;
    },
    [],
  );

  const { data: checkpoints, isLoading } = useCachedPromise(
    async (profile: string) => {
      const resp = await execCommandJson<OkResponse<Checkpoint[]>>("list-session-checkpoints", ["--profile", profile]);
      return resp.data;
    },
    [selectedProfile],
    { execute: !!selectedProfile, failureToastOptions: { title: "Failed to list checkpoints" } },
  );

  function formatTimestamp(microseconds: number): string {
    return new Date(microseconds / 1000).toLocaleString();
  }

  return (
    <List
      isLoading={isLoading}
      searchBarPlaceholder="Search checkpoints..."
      searchBarAccessory={
        profiles ? (
          <List.Dropdown tooltip="Select Profile" onChange={setSelectedProfile}>
            <List.Dropdown.Item title="Select a profile..." value="" />
            {profiles.map((p) => (
              <List.Dropdown.Item key={p.name} title={p.display_name || p.name} value={p.name} />
            ))}
          </List.Dropdown>
        ) : undefined
      }
    >
      {!selectedProfile ? (
        <List.EmptyView title="Select a Profile" description="Choose a profile from the dropdown" icon={Icon.Clock} />
      ) : (
        <>
          <List.EmptyView title="No Checkpoints" description={`No checkpoints for profile "${selectedProfile}"`} icon={Icon.Clock} />
          {checkpoints?.map((cp) => (
            <List.Item
              key={cp.checkpoint_id}
              title={cp.name}
              subtitle={formatTimestamp(cp.saved_at)}
              icon={Icon.Clock}
              accessories={[
                { text: `${cp.tab_count} tabs` },
                { text: cp.origins.length > 0 ? cp.origins[0] : "" },
              ]}
              actions={
                <ActionPanel>
                  <Action
                    title="Restore to Session..."
                    icon={Icon.RotateAntiClockwise}
                    onAction={async () => {
                      if (!sessions || sessions.length === 0) {
                        await showToast({ style: Toast.Style.Failure, title: "No active sessions", message: "Open a session first" });
                        return;
                      }
                      // Restore to the first matching session for this profile
                      const target = sessions.find((s) => s.profile === selectedProfile) || sessions[0];
                      const toast = await showToast({ style: Toast.Style.Animated, title: "Restoring checkpoint..." });
                      await execCommand("restore-session-checkpoint", [target.id, cp.checkpoint_id]);
                      toast.style = Toast.Style.Success;
                      toast.title = "Checkpoint restored";
                      toast.message = `Restored to session ${target.id}`;
                    }}
                  />
                  <Action.CopyToClipboard title="Copy Checkpoint ID" content={cp.checkpoint_id} />
                </ActionPanel>
              }
            />
          ))}
        </>
      )}
    </List>
  );
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add extensions/raycast/src/checkpoints.tsx
git commit -m "feat(raycast): implement checkpoints command"
```

---

### Task 10: Final Build Verification and Cleanup

**Files:**
- All files in `extensions/raycast/`

- [ ] **Step 1: Full type check**

```bash
cd extensions/raycast && npx tsc --noEmit
```

Expected: Pass with no errors.

- [ ] **Step 2: Lint**

```bash
cd extensions/raycast && npx ray lint
```

Fix any lint issues found.

- [ ] **Step 3: Build**

```bash
cd extensions/raycast && npx ray build
```

Expected: Build succeeds.

- [ ] **Step 4: Verify all command files match manifest**

Check that each command `name` in `package.json` has a matching `src/<name>.tsx` file:
- sessions → src/sessions.tsx
- new-session → src/new-session.tsx
- tabs → src/tabs.tsx
- kv-browser → src/kv-browser.tsx
- secrets → src/secrets.tsx
- site-knowledge → src/site-knowledge.tsx
- checkpoints → src/checkpoints.tsx

- [ ] **Step 5: Final commit**

```bash
git add -A extensions/raycast/
git commit -m "feat(raycast): complete extension — sessions, tabs, KV, secrets, site knowledge, checkpoints"
```
