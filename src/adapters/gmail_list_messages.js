// params: { query?: string, max_results?: number }
const q = encodeURIComponent(params.query || "");
const max = params.max_results || 20;
const res = await fetch(
  `https://gmail.googleapis.com/gmail/v1/users/me/messages?q=${q}&maxResults=${max}`,
  { headers: { "Accept": "application/json" } }
);
return res.json();
