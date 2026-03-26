// params: { issue_key: string, transition_id: string }
const baseUrl = window.location.origin;
const res = await fetch(`${baseUrl}/rest/api/3/issue/${params.issue_key}/transitions`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ transition: { id: params.transition_id } })
});
// Jira returns 204 No Content on success
if (res.status === 204) return { success: true };
return res.json();
