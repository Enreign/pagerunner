// params: { project_key: string, summary: string, issue_type?: string, description?: string }
const baseUrl = window.location.origin;
const res = await fetch(`${baseUrl}/rest/api/3/issue`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    fields: {
      project: { key: params.project_key },
      summary: params.summary,
      issuetype: { name: params.issue_type || "Task" },
      description: params.description ? {
        type: "doc", version: 1,
        content: [{ type: "paragraph", content: [{ type: "text", text: params.description }] }]
      } : undefined
    }
  })
});
return res.json();
