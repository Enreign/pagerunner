// params: { parent_id: string, title: string, content?: string }
const res = await fetch("https://api.notion.com/v1/pages", {
  method: "POST",
  headers: { "Content-Type": "application/json", "Notion-Version": "2022-06-28" },
  body: JSON.stringify({
    parent: { page_id: params.parent_id },
    properties: { title: { title: [{ text: { content: params.title } }] } },
    children: params.content ? [{
      object: "block", type: "paragraph",
      paragraph: { rich_text: [{ type: "text", text: { content: params.content } }] }
    }] : []
  })
});
return res.json();
