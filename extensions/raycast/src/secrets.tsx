import { List, ActionPanel, Action, Icon, Alert, confirmAlert, showToast, Toast } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
import type { SecretsResponse } from "./types";

export default function SecretsCommand() {
  const { data, isLoading, mutate } = useCachedPromise(
    async () => {
      await ensureDaemon();
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
