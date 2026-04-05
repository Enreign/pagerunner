import { ActionPanel, Action, Icon, Detail, Form, useNavigation } from "@raycast/api";
import { useCachedPromise } from "@raycast/utils";
import { execCommandJson, ensureDaemon } from "./pagerunner";
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
