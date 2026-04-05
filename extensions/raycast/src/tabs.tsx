import { List, ActionPanel, Action, Icon, Detail, Form, showToast, Toast, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
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
      await ensureDaemon();
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
