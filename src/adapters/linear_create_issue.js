// params: { team_id: string, title: string, description?: string, state_id?: string }
const res = await fetch("https://api.linear.app/graphql", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    query: `mutation($teamId: String!, $title: String!, $description: String, $stateId: String) {
      issueCreate(input: { teamId: $teamId, title: $title, description: $description, stateId: $stateId }) {
        success issue { id identifier title url }
      }
    }`,
    variables: {
      teamId: params.team_id,
      title: params.title,
      description: params.description || null,
      stateId: params.state_id || null
    }
  })
});
return res.json();
