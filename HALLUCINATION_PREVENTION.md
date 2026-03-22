# Hallucination Prevention in Pagerunner

**Version**: v0.1.1+ | **Status**: Production Ready

This guide explains how Pagerunner prevents LLM hallucination when extracting data from web pages.

---

## The Problem

When extracting data without labels, LLMs make assumptions about field order:

```javascript
// Returns [25, 2]
document.querySelectorAll('.metric').forEach(el =>
  parseInt(el.textContent)
);
```

### What Claude Thinks
> "This looks like engagement metrics. Standard order: likes, replies, reposts. So [25, 2] = 25 likes, 2 replies."

### What Actually Happened
> "25 views, 2 likes" (different semantic order)"

**Result**: ❌ Hallucination. Claude confidently reports wrong information.

---

## The Solution: Semantic Metadata

Starting with v0.1.1, Pagerunner automatically attaches metadata to every tool response that warns Claude when data is ambiguous.

### Example: Unlabeled Array

When you call:
```bash
pagerunner evaluate $SESSION $TARGET "
  Array.from(document.querySelectorAll('.metric'))
    .map(el => parseInt(el.textContent))
"
```

Claude receives **two content blocks**:

**Block 1** (Raw Result):
```json
[25, 2]
```

**Block 2** (Metadata Warning):
```json
{
  "_tool": "evaluate",
  "_result_type": "array",
  "_warning": "Result is an array — field meanings cannot be inferred. Use: return { field1: val1, field2: val2 }",
  "_hint": "Always return labeled objects { key: value }, not arrays. Arrays cause field-order ambiguity."
}
```

### Claude's Response (Now Safe)
> "I see this is an array `[25, 2]` but the metadata warns me not to guess field order. Let me request the data as labeled objects instead. Can you extract this with field labels?"

**Result**: ✅ Claude asks for clarification instead of guessing.

---

## How to Use It

### Best Practice: Labeled Objects

Return data with field labels so metadata gives no warning:

```javascript
// Good ✅
Array.from(document.querySelectorAll('.metric')).map(el => {
  const text = el.textContent.trim();
  const match = text.match(/(\d+)\s+(\w+)/);
  return match ? {
    value: parseInt(match[1]),
    label: match[2]
  } : null;
})
```

**Result**: Metadata includes helpful hint, Claude understands data immediately.

### OK: Fixed Position with Documentation

If you must return arrays, document the order in code:

```javascript
// OK (if documented)
[likes, replies, reposts].map(sel =>
  parseInt(document.querySelector(sel).textContent)
)
```

Claude sees:
- The array `[25, 2, 3]`
- The warning about ambiguity
- Requests labeled format

Then you can provide it with labels.

### Avoid: Unlabeled Arrays Without Documentation

```javascript
// Bad ❌
Array.from(document.querySelectorAll('.metric'))
  .map(el => parseInt(el.textContent))
```

This returns `[25, 2]` with no context. Even with metadata warning, it's unreliable.

---

## Metadata Fields Explained

### evaluate()
```json
{
  "_tool": "evaluate",
  "_result_type": "array|object|primitive",
  "_warning": "...",  // Only if array detected
  "_hint": "..."
}
```

**What it means**:
- `_result_type: "array"` → Unlabeled array, can't infer field order
- `_warning` → Explicit warning: "don't guess field meanings"
- `_hint` → Best practice: return labeled objects instead

### wait_for()
```json
{
  "_tool": "wait_for",
  "_condition_type": "selector|url|fixed_delay",
  "_condition_met": true|false,
  "_note": "Condition met — proceed" | "Fixed delay completed..."
}
```

**What it means**:
- `_condition_type` → What was being waited for
- `_condition_met` → Whether condition actually triggered
- `_note` → Human explanation of what happened

### navigate(), click(), fill(), etc.
```json
{
  "_tool": "navigate",
  "_requested_url": "...",
  "_note": "Navigation dispatched. Use wait_for(selector|url) to confirm page load..."
}
```

**What it means**:
- Tool name and context
- Any important parameters
- Next recommended action

### list_tabs(), list_sessions()
```json
{
  "_tool": "list_tabs",
  "_total": 3,
  "_schema": {
    "target_id": "CDP identifier — pass to navigate, get_content, evaluate...",
    "url": "Current page URL",
    "title": "Page title (may be sanitized)"
  }
}
```

**What it means**:
- How many items returned
- What each field contains
- How to use the data

---

## Why This Matters

### Before (v0.1.0)

```
Claude: "Extract engagement metrics"
  → evaluate() returns [25, 2]
  → Claude guesses field order
  → "25 likes, 2 replies"
  → WRONG (actually views, likes)
```

### After (v0.1.1+)

```
Claude: "Extract engagement metrics"
  → evaluate() returns [25, 2] + metadata warning
  → Claude sees "_warning: array ambiguity"
  → Claude requests: "Can you return this with labels?"
  → You provide: {views: 25, likes: 2}
  → Claude reports: "25 views, 2 likes"
  → CORRECT
```

---

## Real-World Example

### Your JavaScript
```javascript
// Extract social media metrics
Array.from(document.querySelectorAll('.stat')).map(el => ({
  number: parseInt(el.querySelector('.count').textContent),
  label: el.querySelector('.label').textContent
}))
```

### Pagerunner Response
```json
{
  "content": [
    {
      "type": "text",
      "text": "[{\"number\": 1234, \"label\": \"Likes\"}, {\"number\": 567, \"label\": \"Replies\"}]"
    },
    {
      "type": "text",
      "text": "{\"_tool\": \"evaluate\", \"_result_type\": \"array\", \"_hint\": \"Always return labeled objects...\"}"
    }
  ]
}
```

### Claude's Interpretation
> "I see an array of objects, each with a `number` and `label`. The metadata suggests labeled objects are best practice. I have the labels right here, so I can confidently report: 1234 Likes, 567 Replies."

✅ **Correct and confident**

---

## For CLI Users

If you're using Pagerunner CLI directly, metadata appears in JSON output:

```bash
$ pagerunner evaluate $SESSION $TARGET "..."
{
  "result": [25, 2],
  "_metadata": {
    "_tool": "evaluate",
    "_result_type": "array",
    "_warning": "Result is an array — field meanings cannot be inferred..."
  }
}
```

Extract just the result:
```bash
pagerunner evaluate $SESSION $TARGET "..." | jq .result
```

Or capture metadata too:
```bash
pagerunner evaluate $SESSION $TARGET "..." | jq ._metadata
```

---

## Troubleshooting

### I'm getting "_warning: array ambiguity" but I know the field order

**Solution**: Return labeled objects instead:

```javascript
// Before (gets warning)
[value1, value2, value3]

// After (no warning)
{first: value1, second: value2, third: value3}
```

### Claude is asking for clarification when I don't want it to

**Solution**: Return labeled data:

```javascript
// Include labels so metadata doesn't trigger warning
Array.from(...).map(el => ({
  label: el.dataset.name,
  value: parseInt(el.textContent)
}))
```

### I want to return raw numbers but need labels for Claude

**Solution**: Provide both in metadata via code comments:

```javascript
// Return [likes, replies, reposts] in that order
Array.from(selectors).map(sel => parseInt(document.querySelector(sel).textContent))
```

Claude will see the warning but can infer from your code comment. Still not ideal—prefer labeled objects.

---

## Monitoring: Is This Working?

Pagerunner tracks incidents where metadata was used to prevent misinterpretation:

### Look for these patterns in audit logs:

1. **Array with warning** — Metadata triggered
2. **Claude request for clarification** — Claude heeded the warning
3. **Followup with labeled data** — User provided corrected format
4. **Correct final result** — Hallucination prevented ✅

### How to Check

```bash
# View recent audit log
tail -f ~/.pagerunner/audit.log | jq '.[] | select(.event_type == "ToolCall")'

# Count metadata usage
grep "_warning" ~/.pagerunner/audit.log | jq -s length
```

---

## Version History

| Version | Feature |
|---------|---------|
| v0.1.0  | Initial release |
| **v0.1.1** | **Semantic metadata + hallucination prevention** |

---

## FAQ

**Q: Will this slow down my queries?**
A: No. Metadata generation is minimal (~5-10ms per tool call). Most tools don't generate metadata at all.

**Q: Are there any breaking changes?**
A: No. Existing code continues to work. Metadata is added automatically.

**Q: What if I don't want metadata?**
A: Both CLI and MCP include it automatically, but you can ignore it. It's purely advisory.

**Q: Does this apply to anonymized content?**
A: Yes. Metadata is generated regardless of anonymization mode.

**Q: Can I disable metadata?**
A: Not currently. Metadata generation is always on. If you need to disable it, please open an issue.

---

## Related Incidents

- **2026-03-21**: Array `[25, 2]` interpreted as "25 likes, 2 replies" instead of "25 views, 2 likes"
  - **Fix**: Metadata warning prevents similar hallucinations going forward
  - **Prevention**: Always return labeled objects

---

## Next Steps

1. **Write tests** — Verify metadata works for your use case
2. **Monitor results** — Check audit logs for warning patterns
3. **Iterate** — Provide feedback if metadata could be clearer
4. **Best practices** — Document your data extraction patterns with labels

---

**Questions or issues?** Open a GitHub issue with your use case.
