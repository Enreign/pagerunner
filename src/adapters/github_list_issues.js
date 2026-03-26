// params: { owner: string, repo: string, state?: "open"|"closed"|"all", limit?: number }
const state = params.state || "open";
const limit = params.limit || 30;
const res = await fetch(
  `https://api.github.com/repos/${params.owner}/${params.repo}/issues?state=${state}&per_page=${limit}`,
  { headers: { "Accept": "application/vnd.github+json" } }
);
return res.json();
