# MCP Publication & Testing Plan

**Goal**: Publish Pagerunner MCP on official registries and test with Dispatch/Cowork
**Status**: Ready for Phase 1 (Publication Checklist)
**Target**: v0.1.1+

---

## Phase 1: Publication Requirements Checklist

### A. Metadata & Manifest

**Current Status**: ✅ Mostly Complete

- [x] README.md with setup instructions
- [x] GitHub repository (public)
- [x] MIT License
- [x] v0.1.1 release with binaries
- [ ] **TODO**: Create `mcp.json` registry entry (formal MCP metadata)
- [ ] **TODO**: Create `mcp-manifest.json` for official registry
- [ ] **TODO**: Add OpenAPI/JSON Schema for all tools

### B. Documentation

**Current Status**: ✅ Excellent

- [x] Setup instructions (quick start)
- [x] CLI subcommands reference
- [x] Feature documentation (anonymization, stealth, snapshots)
- [x] Testing guidelines
- [x] Known issues
- [x] Hallucination prevention guide (NEW in v0.1.1)
- [x] Monitoring guide (NEW in v0.1.1)
- [ ] **TODO**: MCP-specific documentation for registry
- [ ] **TODO**: Example use cases for different LLM platforms

### C. Security & Compliance

**Current Status**: ✅ Good

- [x] Security policy (allowed domains, navigation budget)
- [x] PII anonymization options
- [x] Audit logging
- [x] Prompt injection blocking
- [x] Content sanitization
- [ ] **TODO**: Security disclosure policy
- [ ] **TODO**: Signed releases (GPG keys)
- [ ] **TODO**: Supply chain security checklist

### D. Distribution

**Current Status**: ✅ Complete for v0.1.1

- [x] Pre-built binaries (macOS arm64, x86_64, Linux x86_64)
- [x] Source distribution (GitHub releases)
- [x] Cargo package support
- [ ] **TODO**: Homebrew tap (macOS)
- [ ] **TODO**: APT/DNF packages (Linux)
- [ ] **TODO**: Windows binary (if applicable)

### E. Testing & Validation

**Current Status**: ⚠️ Partial

- [x] 232 unit tests passing
- [x] Real-world validation (agent-browser, Playwright comparison)
- [x] CLI integration tests
- [ ] **TODO**: MCP protocol compliance testing
- [ ] **TODO**: Multi-client testing (Claude Desktop, other MCP clients)
- [ ] **TODO**: Dispatch/Cowork integration testing

---

## Phase 2: Official Registry Options

### Option A: Anthropic Official Registry (Recommended)

**Status**: Not yet available publicly
**Expected**: Q2 2026 (estimated)

**What's needed**:
- [ ] `mcp-manifest.json` with:
  - MCP spec version
  - Tool definitions (JSON Schema)
  - Security metadata
  - Version compatibility
  - Author/maintainer info

**Process**:
1. Prepare manifest
2. Submit to Anthropic (PR or submission form)
3. Review and approval
4. Listed in Claude.com MCP directory

### Option B: GitHub Registry (Community Approach)

**Status**: Can implement now
**Process**:
1. Create `mcp-registry.json` in repo root
2. Submit PR to community registry
3. Discover via GitHub topics (`mcp`, `pagerunner`)
4. Searchable on community aggregators

**Example Structure**:
```json
{
  "name": "pagerunner",
  "version": "0.1.1",
  "description": "Chrome browser automation MCP server",
  "author": "Enreign",
  "repository": "https://github.com/Enreign/pagerunner",
  "license": "MIT",
  "tools": 27,
  "categories": ["browser-automation", "web-scraping"],
  "mcpVersion": "1.0",
  "installation": {
    "linux": "curl -L https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-linux-x86_64 -o pagerunner && chmod +x pagerunner",
    "macos": "curl -L https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-macos-arm64 -o pagerunner && chmod +x pagerunner"
  }
}
```

### Option C: Direct Distribution (Now Available)

**Status**: ✅ Already Working

- Users can install via `cargo install --git https://github.com/Enreign/pagerunner`
- Users can download binaries from GitHub releases
- Users can follow README setup instructions
- Discoverable on GitHub

---

## Phase 3: Testing with Dispatch & Cowork

### What is Dispatch?

Dispatch is likely:
- MCP orchestration/testing framework
- Allows testing MCP compatibility
- Validates tool definitions
- Tests error handling

### What is Cowork?

Cowork appears to be:
- Collaborative MCP development platform
- Multi-client testing environment
- Shared session management
- Real-time tool validation

### Testing Strategy

#### 3A. MCP Protocol Compliance

**Test Points**:
- [ ] Verify stdio transport works
- [ ] Test all 27 tools with Dispatch
- [ ] Validate tool argument schemas
- [ ] Test error responses
- [ ] Verify timeout handling
- [ ] Test concurrent sessions

**Dispatch Test Command** (hypothetical):
```bash
dispatch validate /path/to/pagerunner mcp
dispatch test /path/to/pagerunner mcp --tools all
dispatch benchmark /path/to/pagerunner mcp
```

#### 3B. Cowork Integration Testing

**Test Points**:
- [ ] Multiple clients connecting simultaneously
- [ ] Session isolation
- [ ] Shared KV store
- [ ] Snapshot persistence
- [ ] Concurrent browser operations

**Setup in Cowork**:
```json
{
  "mcp_server": "pagerunner",
  "command": "/path/to/pagerunner mcp",
  "clients": ["claude", "claude-2", "claude-3"],
  "tests": [
    "concurrent_navigation",
    "shared_session",
    "kv_store_isolation",
    "snapshot_persistence"
  ]
}
```

#### 3C. Real-World Scenario Testing

**Scenarios to Test**:
1. **Single-user workflow**: Claude Desktop + Pagerunner
2. **Multi-client workflow**: Multiple Claude Code windows
3. **Integration**: With other MCP tools
4. **Error recovery**: Network interruption, Chrome crash
5. **Security**: Blocked domains, injection attempts

---

## Phase 4: Publication Workflow

### Step 1: Prepare Registry Metadata (This Week)

```bash
# Create registry entry
cat > mcp-registry.json << 'EOF'
{
  "name": "pagerunner",
  "version": "0.1.1",
  "description": "Chrome browser automation MCP server for AI agents",
  "author": {"name": "Enreign", "email": "contact@enreign.io"},
  "repository": "https://github.com/Enreign/pagerunner",
  "license": "MIT",
  "homepage": "https://github.com/Enreign/pagerunner",
  "bugs": "https://github.com/Enreign/pagerunner/issues",
  "keywords": ["mcp", "browser-automation", "chrome", "cdp", "web-scraping", "ai-agents"],
  "mcp": {
    "version": "1.0",
    "transport": "stdio",
    "tools": 27,
    "features": ["anonymization", "snapshots", "stealth-mode", "audit-logging"]
  },
  "installation": {
    "methods": [
      {
        "method": "binary",
        "description": "Download pre-built binary"
      },
      {
        "method": "cargo",
        "command": "cargo install --git https://github.com/Enreign/pagerunner"
      },
      {
        "method": "source",
        "command": "git clone ... && cargo build --release"
      }
    ]
  },
  "setup": "pagerunner init && claude mcp add pagerunner \"$(which pagerunner)\" mcp"
}
EOF
```

### Step 2: Create Tool Definitions (Next Week)

```bash
# Generate JSON Schema for all 27 tools
cat > tools-schema.json << 'EOF'
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "Pagerunner Tools",
  "description": "27 browser automation tools",
  "tools": [
    {
      "name": "navigate",
      "description": "Navigate to URL",
      "inputSchema": {
        "type": "object",
        "properties": {
          "session_id": {"type": "string"},
          "target_id": {"type": "string"},
          "url": {"type": "string"}
        },
        "required": ["session_id", "target_id", "url"]
      }
    },
    // ... 26 more tools
  ]
}
EOF
```

### Step 3: Submit to Registries (Next 2 Weeks)

1. **Community GitHub Registry**
   - Find active MCP registry repo
   - Create PR with mcp-registry.json
   - Include verification of tool count & quality

2. **Anthropic Official Registry** (when available)
   - Register account
   - Submit mcp-manifest.json
   - Undergo review process

3. **Package Managers** (Optional)
   - Homebrew: Create tap formula
   - Linux: APT/DNF packages

### Step 4: Dispatch/Cowork Testing (Parallel)

1. **Get Dispatch Access**
   - Sign up for Dispatch platform
   - Download Dispatch CLI

2. **Run Validation**
   ```bash
   dispatch validate pagerunner
   dispatch test pagerunner --exhaustive
   dispatch benchmark pagerunner
   ```

3. **Get Cowork Access**
   - Join Cowork platform
   - Create shared workspace
   - Invite collaborators

4. **Run Multi-Client Tests**
   - Deploy Pagerunner to Cowork
   - Connect multiple clients
   - Run concurrent scenarios
   - Validate isolation & persistence

---

## Deliverables Checklist

### Immediate (v0.1.1, Now)
- [x] v0.1.1 released with binaries
- [x] Hallucination prevention feature documented
- [x] README complete
- [x] CLAUDE.md tracked (project instructions)

### Before First Registry Submission (Target: 2026-04-01)
- [ ] Create `mcp-registry.json`
- [ ] Create `tools-schema.json` (all 27 tools)
- [ ] Add security disclosure policy
- [ ] Update README with registry info
- [ ] Create CONTRIBUTING.md for developers

### Before Official Registry (Target: 2026-04-15)
- [ ] Create `mcp-manifest.json` (official format)
- [ ] Security audit (code review)
- [ ] GPG-signed releases
- [ ] Comprehensive tool documentation
- [ ] Example notebooks/use cases

### Before Dispatch Testing (Target: 2026-04-01)
- [ ] Compile protocol compliance checklist
- [ ] Create test fixtures
- [ ] Document expected behaviors
- [ ] Prepare benchmark suite

### Before Cowork Testing (Target: 2026-04-15)
- [ ] Multi-client testing guide
- [ ] Session isolation tests
- [ ] Error recovery procedures
- [ ] Performance benchmarks

---

## Files to Create

### 1. mcp-registry.json
Registry metadata for community discovery

### 2. tools-schema.json
Complete JSON Schema for all 27 tools
- Input/output schemas
- Error conditions
- Security notes

### 3. SECURITY.md
Security policy and responsible disclosure

### 4. mcp-manifest.json (Future)
Official Anthropic MCP format
- When official registry launches

### 5. CONTRIBUTING.md
Developer contribution guidelines
- Code style
- Testing requirements
- PR process

### 6. EXAMPLES.md or examples/
Real-world use cases
- Web scraping
- Form automation
- Security testing
- Research/data collection

---

## Success Metrics

### Registration Success
- [ ] Listed in at least 1 community registry
- [ ] 100+ GitHub stars (ecosystem interest)
- [ ] Searchable on community aggregators

### Testing Success
- [ ] 100% tool compatibility on Dispatch
- [ ] Multi-client scenarios passing on Cowork
- [ ] Zero security issues found
- [ ] <100ms latency on tool calls

### Adoption Metrics
- [ ] 50+ users in first month
- [ ] Positive community feedback
- [ ] Contributions/forks from developers
- [ ] Real-world use cases documented

---

## Timeline

```
Week 1 (Now):       ✅ v0.1.1 released
Week 2-3:           [ ] Create registry metadata
Week 4:             [ ] Submit to community registries
Week 5-6:           [ ] Dispatch testing (in parallel)
Week 7-8:           [ ] Cowork testing (in parallel)
Week 9-10:          [ ] Official registry submission
Week 11-12:         [ ] Monitor adoption & iterate
```

---

## Open Questions

1. **Dispatch Platform**: Where/how to sign up? What are the exact validation rules?
2. **Cowork Platform**: Is this an open platform or invite-only? What's the testing workflow?
3. **Official Registry**: When will Anthropic launch the official MCP registry?
4. **Windows Support**: Is Pagerunner needed on Windows? (CDP availability?)
5. **Package Managers**: Should we create Homebrew tap and Linux packages in parallel?

---

## Next Action Items

1. **Immediate** (Today):
   - Get access to Dispatch platform
   - Get access to Cowork platform
   - Create basic registry metadata

2. **This Week**:
   - Create mcp-registry.json
   - Create tools-schema.json
   - Write SECURITY.md

3. **Next Week**:
   - Run Dispatch validation
   - Start Cowork integration
   - Submit to first community registry

---

**Owner**: @Enreign
**Last Updated**: 2026-03-22
**Next Review**: After Phase 1 completion
