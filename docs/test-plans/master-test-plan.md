# pagerunner Master Test Plan

Living document. Update this plan when features are added or behaviour changes.
Run a test execution against this plan after every medium or large change — record results in `docs/test-runs/`.

## Column legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Covered — automated test exists and passes |
| 🖐 | Manual run required — not yet automated |
| — | Not applicable for this surface |

**Column definitions:**
- **Unit** — unit test or non-Chrome CLI integration test (runs with `cargo test`)
- **Live MCP** — confirmed in a live MCP session (see `docs/test-runs/2026-03-21-run-6.md`)
- **Live CLI** — confirmed via `#[ignore]` Chrome CLI test in `tests/cli_tools_integration.rs`

---

## 1. Session Management

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| S1 | `list-profiles` returns JSON array with `name` and `display_name` | ✅ | ✅ | ✅ |
| S2 | `open-session <profile>` returns `{"session_id": ..., "stealth": false}` | ✅ | ✅ | ✅ |
| S3 | `open-session --stealth` returns `{"stealth": true}` | — | ✅ | 🖐 |
| S4 | `list-sessions` returns running sessions | ✅ | ✅ | ✅ |
| S5 | `close-session <id>` closes Chrome, returns success | — | ✅ | ✅ |
| S6 | Unknown profile → exit 1, error on stderr | ✅ | — | — |
| S7 | Duplicate `open-session` on locked profile → clear error | — | 🖐 | 🖐 |

---

## 2. Tab Management

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| T1 | `new-tab <sid>` returns `{"target_id", "title", "url"}` | — | ✅ | ✅ |
| T2 | `list-tabs <sid>` returns array containing new tab | — | ✅ | ✅ |
| T3 | `new-tab --url <url>` opens tab at given URL | — | ✅ | 🖐 |
| T4 | Invalid session-id → exit 1 | ✅ | — | — |

---

## 3. Navigation

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| N1 | `navigate <sid> <tid> https://example.com` succeeds | — | ✅ | ✅ |
| N2 | `navigate about:blank` → blocked (`about:` scheme rejected) | ✅ | — | — |
| N3 | `navigate file:///etc/passwd` → blocked (`file:` scheme rejected) | ✅ | — | — |
| N4 | `navigate javascript:alert(1)` → blocked | ✅ | — | — |
| N5 | `navigate http://localhost/` → blocked (private IP) | ✅ | — | — |
| N6 | `navigate http://192.168.1.1/` → blocked (private IP) | ✅ | — | — |
| N7 | `wait-for --ms 100` time-based wait succeeds | ✅ | ✅ | ✅ |
| N8 | `wait-for --selector <sel>` waits for element | — | ✅ | ✅ |
| N9 | `wait-for --url <pattern>` waits for URL substring | — | ✅ | ✅ |
| N10 | Navigation with `allowed-domains` restricts to listed domains | ✅ | ✅ | ✅ |
| N11 | Navigation beyond `max-navigations` budget → error | ✅ | ✅ | 🖐 |

---

## 4. Content

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| C1 | `get-content` returns sanitized page text | — | ✅ | ✅ |
| C2 | `screenshot` saves PNG to temp file | — | ✅ | ✅ |
| C3 | `screenshot --base64` returns inline base64 JSON | — | ✅ | ✅ |
| C4 | `evaluate "1+1"` returns `2` | — | ✅ | ✅ |
| C5 | `evaluate` returning object → JSON string | — | ✅ | 🖐 |

---

## 5. Interactions

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| I1 | `click <selector>` clicks element | — | ✅ | ✅ |
| I2 | `fill <selector> <value>` fills input | — | ✅ | ✅ |
| I2b | `fill <selector> <value>` fills textarea | — | ✅ | ✅ |
| I3 | `type-text <text>` types at focused element | — | ✅ | ✅ |
| I4 | `select <selector> <value>` selects dropdown option | — | ✅ | ✅ |
| I5 | `scroll --y 500` scrolls page | — | ✅ | ✅ |
| I6 | `scroll --selector <sel>` scrolls element into view | — | ✅ | 🖐 |
| I7 | Invalid selector → error returned, not panic | — | ✅ | ✅ |

---

## 6. KV Store

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| K1 | `kv-set <ns> <key> <value>` stores value | ✅ | ✅ | ✅ |
| K2 | `kv-get <ns> <key>` retrieves stored value | ✅ | ✅ | ✅ |
| K3 | `kv-list <ns>` lists all keys | ✅ | ✅ | ✅ |
| K4 | `kv-list --prefix <pfx>` filters by prefix | ✅ | ✅ | ✅ |
| K5 | `kv-list --keys-only` omits values | ✅ | ✅ | ✅ |
| K6 | `kv-delete <ns> <key>` removes key | ✅ | ✅ | ✅ |
| K7 | `kv-clear <ns>` removes all keys in namespace | ✅ | ✅ | ✅ |
| K8 | Cross-namespace isolation — key in ns-A not visible in ns-B | ✅ | — | — |

---

## 7. Snapshots

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| P1 | `save-snapshot` saves current page state | — | ✅ | ✅ |
| P2 | `list-snapshots` returns saved snapshots | ✅ | ✅ | ✅ |
| P3 | `list-snapshots --all` includes other profiles | ✅ | ✅ | ✅ |
| P4 | `restore-snapshot <origin>` restores page state | — | ✅ | 🖐 |
| P5 | `delete-snapshot <profile> <origin>` removes snapshot | — | ✅ | 🖐 |
| P6 | `restore-snapshot --from-profile <name>` cross-profile restore | — | 🖐 | 🖐 |

---

## 8. Tab State

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| TS1 | `save-tab-state` saves open tabs to DB | — | ✅ | ✅ |
| TS2 | `restore-tab-state` reopens saved tabs | — | ✅ | ✅ |
| TS3 | `about:blank` tabs skipped during restore (not navigated) | ✅ | — | — |

---

## 9. Security Policy

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| SEC1 | Private IPs blocked regardless of policy | ✅ | — | — |
| SEC2 | Non-HTTP/S schemes blocked | ✅ | — | — |
| SEC3 | `allowed-domains` restricts navigation | ✅ | ✅ | ✅ |
| SEC4 | Navigation budget enforcement | ✅ | ✅ | 🖐 |
| SEC5 | `blocked-tools` prevents listed tool calls | ✅ | ✅ | — |
| SEC6 | `allowed-tools` intersection with server allowlist | — | ✅ | — |
| SEC7 | URLs with embedded credentials blocked | ✅ | — | — |

---

## 10. Prompt Injection

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| PI1 | Injection pattern in tab title → `[injection detected]` | ✅ | ✅ | — |
| PI2 | Injection pattern in `get-content` body → `[REDACTED]` | ✅ | ✅ | — |
| PI3 | Injection pattern in `evaluate` result → `[REDACTED]` | ✅ | ✅ | — |
| PI4 | Non-injection content preserved around redacted phrase | — | ✅ | 🖐 |

---

## 11. Anonymization — Phase 1 (Regex-based PII)

All Phase 1 tests apply with `anonymize: true` (or `--anonymize` CLI flag).
Default entities: EMAIL, PHONE, CREDIT_CARD, IBAN, SSN, IP.

### 11a. Tokenize mode

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| A1 | EMAIL detected and replaced with `[EMAIL:xxxxxx]` | ✅ | — | — |
| A2 | Multiple emails → separate tokens, same email → same token | ✅ | — | — |
| A3 | PHONE (US/intl formats) detected and tokenized | ✅ | — | — |
| A4 | CREDIT_CARD (Luhn-valid) detected; invalid Luhn ignored | ✅ | — | — |
| A5 | IBAN (mod-97 valid) detected; invalid ignored | ✅ | — | — |
| A6 | SSN (valid area code) detected; 000/666/900+ rejected | ✅ | — | — |
| A7 | IPv4 (valid octets) detected; 999.x.x.x rejected | ✅ | — | — |
| A8 | Token format matches `[ENTITY_TYPE:xxxxxx]` pattern | ✅ | — | — |
| A9 | `is_token()` returns true for all emitted tokens | ✅ | — | — |
| A10 | Same value in same session always gets same token | ✅ | — | — |
| A11 | Different sessions get independent vault scoping | ✅ | — | — |
| A12 | Mixed entity types in one text each tokenized | ✅ | — | — |
| A13 | Entity counts accurate across multiple types | ✅ | — | — |
| A14 | Residual scan fails-closed if PII survives substitution | ✅ | — | — |
| A15 | `get-content` with `anonymize: true` — PII replaced in output | ✅ | ✅ | ✅ |
| A16 | `evaluate` with `anonymize: true` — PII replaced in result | ✅ | ✅ | — |
| A17 | `screenshot` blocked when `anonymize: true` | ✅ | ✅ | ✅ |
| A18 | Token passed to `fill` is de-tokenized before DOM write | ✅ | ✅ | — |
| A19 | Token passed to `type-text` is de-tokenized | ✅ | — | — |
| A20 | `ContentAnonymized` audit event records type counts only | ✅ | — | — |
| A21 | `open-session --anonymize` flag accepted | ✅ | — | ✅ |
| A22 | Live page: email in page body → replaced in `get-content` | — | ✅ | ✅ |
| A23 | Live page: fill with token → correct value written to DOM | — | ✅ | 🖐 |

### 11b. Redact mode

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| AR1 | EMAIL → `[EMAIL]` (no colon, no nonce) | ✅ | — | — |
| AR2 | Mixed types in redact mode → `[EMAIL]`, `[PHONE]` etc. | ✅ | — | — |
| AR3 | `fill` with token-shaped value in redact session → error | ✅ | — | — |
| AR4 | `anonymization_mode: "redact"` inline param accepted | ✅ | — | — |

### 11c. Inline / named profile params

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| AP1 | `anonymization_entities: ["EMAIL","PHONE"]` limits detection | ✅ | — | — |
| AP2 | `anonymization_profile: "jira-work"` loads domain profile | ✅ | — | — |
| AP3 | Named profile + inline entities → mutual exclusion error | ✅ | — | — |
| AP4 | `anonymize: false` → no anonymization applied | ✅ | — | — |

### 11d. Custom patterns

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| AC1 | Custom regex pattern tokenizes matches | ✅ | — | — |
| AC2 | Multiple custom pattern matches each tokenized | ✅ | — | — |
| AC3 | Custom pattern in redact mode → `[NAME]` placeholder | ✅ | — | — |
| AC4 | `custom_patterns` in `config.toml` parsed correctly | ✅ | — | — |

---

## 12. Anonymization — Phase 2 (NER: PERSON and ORG)

Requires `--features ner` build + `pagerunner download-model`.
NER detects names and organisations using a BERT-based ONNX model.

### 12a. Model lifecycle

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| NM1 | `MODEL_SHA256` constant is 64 lowercase hex chars | ✅ | — | — |
| NM2 | `verify_model_hash` errors on missing file | ✅ | — | — |
| NM3 | `verify_model_hash` errors on wrong hash (`HashMismatch`) | ✅ | — | — |
| NM4 | `pagerunner download-model` downloads ner.onnx + tokenizer.json | — | — | 🖐 |
| NM5 | `NerSession::load` succeeds after download | — | — | 🖐 |

### 12b. NER inference helpers

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| NH1 | `char_to_byte` correct for ASCII text | ✅ | — | — |
| NH2 | `char_to_byte` correct for multi-byte Unicode (e.g. "café") | ✅ | — | — |
| NH3 | `char_to_byte` returns `text.len()` for out-of-range offset | ✅ | — | — |
| NH4 | `char_to_byte` on empty string returns 0 | ✅ | — | — |
| NH5 | `flush` with `None` — no span added | ✅ | — | — |
| NH6 | `flush` with valid `(start, end, type)` — span appended | ✅ | — | — |
| NH7 | `flush` with zero-width span `(n, n, type)` — discarded | ✅ | — | — |

### 12c. AnonEngine with NER disabled (no model required)

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| ND1 | NER disabled: PERSON/ORG not tokenized, EMAIL still tokenized | ✅ | — | — |
| ND2 | NER disabled: redact mode — EMAIL redacted, PERSON/ORG not | ✅ | — | — |
| ND3 | PERSON/ORG in entity list with no NER session → no panic | ✅ | — | — |
| ND4 | `detect_spans(Person, Org)` returns empty (no regex detection) | ✅ | — | — |
| ND5 | `open_session` with PERSON in entities but no NER model → check_ner_model skipped | ✅ | — | — |

### 12d. NER entity detection (require model)

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| NE1 | "Alice Smith is the CEO of Acme Corp" → PERSON: "Alice Smith", ORG: "Acme Corp" | ✅ (`#[ignore]`) | — | — |
| NE2 | "Bob Dylan won a Nobel Prize" → PERSON: "Bob Dylan" | ✅ (`#[ignore]`) | — | — |
| NE3 | "Contact support at Google" → ORG detected | ✅ (`#[ignore]`) | — | — |
| NE4 | "Jane Smith works at Microsoft Corp" → PERSON + ORG | ✅ (`#[ignore]`) | — | — |
| NE5 | "John Paul Smith attended the conference" → multi-token name = single span | ✅ (`#[ignore]`) | — | — |
| NE6 | "Apple and IBM are technology companies" → 2 ORG spans | ✅ (`#[ignore]`) | — | — |
| NE7 | All spans from multi-entity sentence have valid UTF-8 byte boundaries | ✅ (`#[ignore]`) | — | — |
| NE8 | Adjacent PERSON + ORG spans are non-overlapping after deduplication | ✅ (`#[ignore]`) | — | — |

### 12e. Full NER pipeline via AnonEngine (require model)

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| NP1 | `process()` with PERSON entity: name tokenized → `[PERSON:xxxxxx]` | ✅ (`#[ignore]`) | — | — |
| NP2 | `process()` with ORG entity: company tokenized → `[ORG:xxxxxx]` | ✅ (`#[ignore]`) | — | — |
| NP3 | `process()` with EMAIL+PERSON+ORG: all three replaced | ✅ (`#[ignore]`) | — | — |
| NP4 | `process()` tokenize: same person name → same token across calls | ✅ (`#[ignore]`) | — | — |
| NP5 | `process()` redact: name → `[PERSON]`, company → `[ORG]` | ✅ (`#[ignore]`) | — | — |
| NP6 | `entity_counts` includes PERSON and ORG correctly | ✅ (`#[ignore]`) | — | — |
| NP7 | Residual scan excludes PERSON/ORG (no NER re-run on output) | ✅ (`#[ignore]`) | — | — |
| NP8 | Live `get-content` with NER — Alice Smith/Bob Jones/Acme Corp masked | — | — | ✅ (live CLI) |
| NP9 | Live `fill` with `[PERSON:xxxxxx]` token → original name written to DOM | — | — | ✅ (live CLI) |

---

## 13. Error Handling

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| E1 | Missing required args → clap error on stderr, exit 1 | ✅ | — | — |
| E2 | Invalid session for any session-requiring command → exit 1 | ✅ | — | — |
| E3 | Tool call with unknown tool name → error response | ✅ | — | — |
| E4 | Tool call with missing required param → error response | ✅ | — | — |

---

## 14. Help / Flags

| ID | Test | Unit | Live MCP | Live CLI |
|----|------|------|----------|---------|
| H1 | `screenshot --help` includes `--base64` | ✅ | — | — |
| H2 | `open-session --help` includes `--anonymize` and `--stealth` | ✅ | — | — |
| H3 | `wait-for --help` shows `--selector`, `--url`, `--ms` modes | ✅ | — | — |

---

## Coverage Summary

| Surface | Unit | Live MCP | Live CLI |
|---------|------|----------|---------|
| Session management | 3/7 | 6/7 | 5/7 |
| Tab management | 1/4 | 3/4 | 2/4 |
| Navigation | 6/11 | 5/11 | 7/11 |
| Content | 0/5 | 5/5 | 4/5 |
| Interactions | 0/8 | 8/8 | 7/8 |
| KV store | 8/8 | 7/8 | 7/8 |
| Snapshots | 2/6 | 5/6 | 3/6 |
| Tab state | 1/3 | 2/3 | 2/3 |
| Security | 5/7 | 4/7 | 3/7 |
| Prompt injection | 3/4 | 4/4 | 0/4 |
| Anon Phase 1 (unit) | 20/23 | 5/23 | 4/23 |
| Anon Phase 2 (NER) | 22/22 | 0/22 | 2/22 |
| Error handling | 4/4 | 0/4 | 0/4 |
| Help / flags | 3/3 | 0/3 | 0/3 |

**Automated Chrome tests (`#[ignore]`):** 22 in `tests/cli_tools_integration.rs`
- Original 9: session lifecycle, screenshot (file + base64), evaluate, kv roundtrip, list-tabs, wait-for ms, snapshot save/list/delete, tab state save/restore
- New 13: click, fill (input), fill (textarea), type-text, select, scroll-y, invalid-selector error, wait-for selector, wait-for url, anonymize get-content, anonymize screenshot blocked, allowed-domains blocks nav, NER anonymize person masked

Last updated: 2026-03-21 (added 13 new `#[ignore]` Chrome CLI tests; 3-column coverage format; 22 total Chrome tests; 23 non-Chrome pass; 237 unit tests passing)
