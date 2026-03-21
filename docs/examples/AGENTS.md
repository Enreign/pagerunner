# Browser Automation (Pagerunner)

Use the `pagerunner` MCP tools to drive a real Chrome browser.

## Session lifecycle

1. `list_profiles` — see available profiles
2. `open_session(profile="personal")` — launches Chrome, returns `session_id`
3. `new_tab(session_id, url="...")` — opens a tab, returns `target_id`
4. Do work: `navigate`, `get_content`, `click`, `type_text`, `fill`, `select`, `scroll`, `screenshot`, `evaluate`
5. `close_session(session_id)` — always close when done

## Tips

- Use `wait_for` after navigation before reading content
- Use `screenshot` when unsure what's on screen
- Pass `stealth: true` to `open_session` on sites that detect automation
- `save_snapshot` / `restore_snapshot` to persist login state across sessions
- Run `pagerunner status` to verify setup; `pagerunner daemon &` for multiple agent windows

## Anonymization (PII protection)

Pass `anonymize: true` to `open_session` to prevent PII from reaching the agent:

```json
{ "profile": "personal", "anonymize": true }
```

- All `get_content` and `evaluate` results have PII stripped before the agent sees them
- Screenshots are blocked in anonymization mode
- **Default entities detected:** EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP
- **With NER build:** also detects PERSON names and ORG names via a local ONNX model

### Modes

- **tokenize** (default): replaces PII with tokens like `[EMAIL:a3f9b2]`
  — pass tokens back to `fill`/`type_text` and Pagerunner de-tokenizes before writing to the DOM
- **redact**: one-way replacement with `[EMAIL]` — no vault, no de-tokenization

### Limit to specific entity types

```json
{ "profile": "personal", "anonymize": true, "anonymization_entities": ["EMAIL", "PHONE"], "anonymization_mode": "redact" }
```

### Custom patterns (via config.toml profile)

```json
{ "profile": "personal", "anonymization_profile": "jira-work" }
```
