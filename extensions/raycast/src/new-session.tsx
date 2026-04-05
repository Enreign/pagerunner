import { List, ActionPanel, Action, Icon, showToast, Toast, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, ensureDaemon } from "./pagerunner";
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
