import { List, ActionPanel, Action, Icon, Color, showToast, Toast } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
import type { Session, OkResponse } from "./types";
import TabsCommand from "./tabs";

export default function SessionsCommand() {
  const { data, isLoading, mutate } = useCachedPromise(
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

