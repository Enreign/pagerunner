# Monitoring Hallucination Prevention

**Tracking**: Effectiveness of semantic metadata in preventing LLM hallucinations
**Period**: v0.1.1 onwards
**Last Updated**: 2026-03-22

---

## What We're Measuring

### Primary Metric: Array Ambiguity Detections

**Definition**: Number of times evaluate() returned unlabeled arrays and metadata warning was triggered

**How to measure**:
```bash
# Count warnings in audit log
grep -c "_warning.*array" ~/.pagerunner/audit.log

# Show recent warnings with context
grep "_warning.*array" ~/.pagerunner/audit.log | jq '.[] | {time: .timestamp, tool: .tool, result: .result}' | tail -20
```

**Goal**: Identify patterns where array warnings were triggered, suggesting ambiguous data extraction.

### Secondary Metric: Hallucination Incidents

**Definition**: Reported cases where metadata prevented a hallucination, or where ambiguous data led to misinterpretation

**How to measure**:
- Collect user bug reports mentioning "metadata", "array", "ambiguity"
- Review audit logs for patterns
- Track in GitHub issues with label `hallucination-prevention`

---

## Audit Log Analysis

### New Fields in Audit Events (v0.1.1+)

Each tool call now includes potential metadata:

```json
{
  "timestamp": "2026-03-22T14:30:00Z",
  "session_id": "...",
  "tool": "evaluate",
  "result": "[25, 2]",
  "_metadata": {
    "_tool": "evaluate",
    "_result_type": "array",
    "_warning": "Result is an array — field meanings cannot be inferred..."
  }
}
```

### Queries to Run

#### 1. Find all array warnings
```bash
jq 'select(._metadata._warning != null and ._metadata._warning | contains("array"))' ~/.pagerunner/audit.log
```

#### 2. Count warnings by tool
```bash
jq 'select(._metadata._warning != null) | ._tool' ~/.pagerunner/audit.log | sort | uniq -c
```

#### 3. Find patterns (e.g., metrics extraction)
```bash
jq 'select(._metadata._warning != null and .result | contains("metric"))' ~/.pagerunner/audit.log
```

#### 4. Check condition clarity (wait_for)
```bash
jq 'select(.tool == "wait_for") | ._metadata' ~/.pagerunner/audit.log
```

#### 5. Show timeline of metadata usage
```bash
jq '{time: .timestamp, tool: .tool, has_metadata: (._metadata != null)}' ~/.pagerunner/audit.log | tail -100
```

---

## Incident Tracking Template

When metadata prevents or nearly prevents a hallucination, log it:

### GitHub Issue Template
```markdown
## Hallucination Prevention Case Study

**Date**: [YYYY-MM-DD]
**Tool**: evaluate | wait_for | other
**Severity**: Prevented | Near-miss

### The Scenario
- What data was being extracted?
- How was the JavaScript returning it?
- What was Claude's initial interpretation?

### The Metadata Intervention
- What warning was triggered?
- How did Claude respond to the warning?
- Did Claude request clarification or different format?

### Outcome
- ✅ Hallucination prevented
- ⚠️ Near-miss (Claude almost misinterpreted)
- ❌ Metadata couldn't prevent (case for improvement)

### Lessons Learned
- What best practice was violated?
- How to prevent similar issues?
- Should metadata warning be clearer?

### Evidence
- Audit log timestamps
- Screenshot of Claude's response
- Original JavaScript code
```

---

## Monthly Review Checklist

**Last Review**: [To be filled]

### Review Process (Monthly)

1. **Audit Log Summary**
   ```bash
   # Check volume of metadata warnings
   grep -c "_warning" ~/.pagerunner/audit.log
   ```
   - Expected: Growing baseline (more tools using metadata)
   - Flag: Sudden spike (possible pattern)

2. **Incident Analysis**
   - Review GitHub issues with `hallucination-prevention` label
   - Count prevented vs near-miss vs failed cases
   - Identify patterns

3. **Metadata Quality**
   - Are warnings clear and actionable?
   - Is Claude responding as expected?
   - Any usability issues?

4. **Tool Coverage**
   - Are P1 tools generating useful metadata?
   - Any P0 tools missing warnings?

5. **Release Notes**
   - Document patterns observed
   - Recommend best practices
   - Update guidance based on incidents

---

## Success Criteria

### Short-term (v0.1.1 - v0.2.0)
- [ ] Array warnings triggered in real usage
- [ ] Claude responding appropriately to warnings
- [ ] Zero reported hallucinations from ambiguous arrays
- [ ] User feedback is positive

### Medium-term (v0.2.0 - v0.3.0)
- [ ] 50% of array queries return labeled objects (vs unlabeled)
- [ ] Metadata requests for clarification become normal pattern
- [ ] Incident rate drops 90% from baseline
- [ ] Tooling/docs help prevent issues proactively

### Long-term (v0.3.0+)
- [ ] Hallucination from ambiguous data is rare anomaly
- [ ] Best practices well-documented and followed
- [ ] Metadata hints improve extraction quality overall
- [ ] Pattern understood across multiple LLM integrations

---

## Red Flags

Watch for these patterns:

| Pattern | Meaning | Action |
|---------|---------|--------|
| No `_warning` fields in 24h | Metadata not being triggered | Check if users are returning labeled data (good) or skipping validation (bad) |
| Spike in `_warning` fields | Possible pattern of ambiguous extractions | Review code patterns, identify common mistakes |
| "array ambiguity" ignored by Claude | Claude not heeding warning | Check Claude version, escalate to test prompt engineering |
| Users disabling metadata | Users see it as noise | Refine messages, ensure warnings are essential only |
| Repeated same array pattern | Systemic issue in one tool | Fix underlying tool, improve hint message |

---

## Example Analysis

### Session from 2026-03-22 (Validation Test)

**Query**:
```bash
grep "2026-03-22" ~/.pagerunner/audit.log | grep evaluate | jq .
```

**Findings**:
- 3 evaluate() calls
- 2 returned labeled objects (no warning)
- 1 returned unlabeled array (warning triggered)
- Warning message was clear and actionable

**Outcome**: ✅ Metadata working as designed

---

## Historical Baseline

| Period | Ambiguous Arrays | Metadata Warnings | Reported Incidents |
|--------|------------------|-------------------|-------------------|
| v0.1.0 | Unknown (no tracking) | N/A | 1 (2026-03-21 X metrics) |
| v0.1.1+ | [To be filled] | [To be filled] | [To be filled] |

---

## Tools for Analysis

### 1. Real-time Monitoring
```bash
# Watch metadata warnings in real-time
tail -f ~/.pagerunner/audit.log | jq 'select(._metadata._warning != null)'
```

### 2. Weekly Summary
```bash
#!/bin/bash
# Generate weekly report
echo "=== Pagerunner Metadata Usage (Week of $(date +%Y-W%V)) ==="
echo "Total tool calls: $(jq -s length ~/.pagerunner/audit.log)"
echo "Calls with metadata: $(grep -c "_metadata" ~/.pagerunner/audit.log)"
echo "Array warnings: $(grep -c "_warning.*array" ~/.pagerunner/audit.log)"
echo "Condition warnings: $(grep -c "_condition" ~/.pagerunner/audit.log)"
```

### 3. Incident Dashboard
```bash
# Report incidents per month
jq '.timestamp | split("T")[0] | split("-")[0:2] | join("-")' ~/.pagerunner/audit.log | sort | uniq -c | tail -12
```

---

## Questions to Answer Monthly

1. **Are users returning labeled or unlabeled data?**
   - Trend toward labeled? Good—metadata is working
   - Spike in unlabeled? Warning users to change pattern

2. **Is Claude behaving as expected?**
   - Requesting clarification on arrays? Yes—working
   - Ignoring warnings? Escalate for investigation

3. **Are there systematic patterns?**
   - One tool always returns problematic format?
   - One use case particularly prone to hallucination?
   - Root cause for improvement?

4. **Is the messaging working?**
   - Are warnings clear?
   - Is `_hint` helpful?
   - Do users understand what to do?

5. **What's the incident trend?**
   - Going down? Success—metadata preventing issues
   - Stable? Good baseline established
   - Going up? New pattern or increased usage?

---

## Escalation Procedure

If you discover a pattern suggesting metadata isn't preventing hallucinations:

1. **Document the case** — GitHub issue with `hallucination-prevention` label
2. **Include audit logs** — Specific timestamps and tool calls
3. **Classify severity**:
   - 🔴 Critical: Hallucination still occurred despite metadata
   - 🟡 Warning: Claude nearly misinterpreted despite metadata
   - 🟢 Informational: Edge case found, no actual issue
4. **Propose improvement** — New warning message, additional context, etc.
5. **Test fix** — Verify improvement with real audit data

---

## Related Documentation

- [HALLUCINATION_PREVENTION.md](../HALLUCINATION_PREVENTION.md) — User guide
- [master-test-plan.md](test-plans/master-test-plan.md) — Test coverage
- GitHub Issues: `hallucination-prevention` label

---

**Next update due**: 2026-04-22
