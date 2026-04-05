import { List, ActionPanel, Action, Icon, showToast, Toast } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
import type { Checkpoint, Session, Profile, OkResponse } from "./types";
import { useState } from "react";

export default function CheckpointsCommand() {
  const [selectedProfile, setSelectedProfile] = useState<string>("");

  const { data: profiles } = useCachedPromise(
    async () => {
      await ensureDaemon();
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
