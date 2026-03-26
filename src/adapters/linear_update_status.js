// params: { issue_id: string, state_id: string }
const res = await fetch("https://api.linear.app/graphql", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    query: `mutation($id: String!, $stateId: String!) {
      issueUpdate(id: $id, input: { stateId: $stateId }) {
        success issue { id state { name } }
      }
    }`,
    variables: { id: params.issue_id, stateId: params.state_id }
  })
});
return res.json();
