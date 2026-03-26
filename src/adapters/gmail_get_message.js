// params: { message_id: string, format?: "full"|"metadata"|"raw" }
const format = params.format || "full";
const res = await fetch(
  `https://gmail.googleapis.com/gmail/v1/users/me/messages/${params.message_id}?format=${format}`,
  { headers: { "Accept": "application/json" } }
);
return res.json();
