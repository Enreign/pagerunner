// params: { query: string, limit?: number }
// query uses GitHub issue search syntax, e.g. "repo:owner/repo is:open label:bug"
const limit = params.limit || 30;
const q = encodeURIComponent(params.query);
const res = await fetch(
  `https://api.github.com/search/issues?q=${q}&per_page=${limit}`,
  { headers: { "Accept": "application/vnd.github+json" } }
);
return res.json();
