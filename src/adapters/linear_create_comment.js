// params: { issue_id: string, body: string }
const res = await fetch("https://api.linear.app/graphql", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    query: `mutation($issueId: String!, $body: String!) {
      commentCreate(input: { issueId: $issueId, body: $body }) {
        success comment { id createdAt }
      }
    }`,
    variables: { issueId: params.issue_id, body: params.body }
  })
});
return res.json();
