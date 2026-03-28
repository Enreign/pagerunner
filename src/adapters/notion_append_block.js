// params: { page_id: string, content: string }
const res = await fetch(`https://api.notion.com/v1/blocks/${params.page_id}/children`, {
  method: "PATCH",
  headers: { "Content-Type": "application/json", "Notion-Version": "2022-06-28" },
  body: JSON.stringify({
    children: [{
      object: "block",
      type: "paragraph",
      paragraph: {
        rich_text: [{ type: "text", text: { content: params.content } }]
      }
    }]
  })
});
return res.json();
