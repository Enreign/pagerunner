// params: { owner: string, repo: string, title: string, body?: string, labels?: string[] }
const res = await fetch(
  `https://api.github.com/repos/${params.owner}/${params.repo}/issues`,
  {
    method: "POST",
    headers: { "Accept": "application/vnd.github+json", "Content-Type": "application/json" },
    body: JSON.stringify({ title: params.title, body: params.body || "", labels: params.labels || [] })
  }
);
return res.json();
