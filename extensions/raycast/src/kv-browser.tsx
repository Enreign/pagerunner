import { List, ActionPanel, Action, Icon, Detail, Form, showToast, Toast, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, execCommand, ensureDaemon } from "./pagerunner";
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

function KvKeyList({ namespace }: { namespace: string }) {
  const [prefix, setPrefix] = useState<string>("");

  const { data, isLoading, revalidate, mutate } = useCachedPromise(
    async (ns: string, pfx: string) => {
      await ensureDaemon();
      const args = [ns];
      if (pfx) args.push("--prefix", pfx);
      const resp = await execCommandJson<OkResponse<KvEntry[]>>("kv-list", args);
      return resp.data;
    },
    [namespace, prefix],
    { failureToastOptions: { title: "Failed to list keys" } },
  );

  return (
    <List
      isLoading={isLoading}
      searchBarPlaceholder={`Filter keys in "${namespace}"...`}
      onSearchTextChange={setPrefix}
      throttle
    >
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
    </List>
  );
}

export default function KvBrowserCommand() {
  const { push } = useNavigation();

  return (
    <Form
      actions={
        <ActionPanel>
          <Action.SubmitForm
            title="Browse Namespace"
            icon={Icon.MagnifyingGlass}
            onSubmit={(values: { namespace: string }) => {
              push(<KvKeyList namespace={values.namespace} />);
            }}
          />
        </ActionPanel>
      }
    >
      <Form.TextField id="namespace" title="Namespace" placeholder="my-namespace" />
    </Form>
  );
}
