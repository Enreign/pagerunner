# MCP & CLI Registry Submission Plan

## Executive Summary

Pagerunner can be distributed across multiple registries to reach different audiences:

| Registry | Type | Effort | Timeline | Impact | Status |
|----------|------|--------|----------|--------|--------|
| **MCP Registry** | Official MCP | Low | Same day | Direct MCP client discovery | 🟢 Ready |
| **crates.io** | Rust packages | Minimal | Immediate | Reach Rust developers | 🟢 Ready |
| **GitHub Releases** | Binary distro | Low | Today | Foundation for all others | 🟢 Ready |
| **Homebrew tap** | macOS package | Low | 1 day | Native macOS support | 🟢 Ready |
| **Cline Marketplace** | MCP marketplace | Very Low | 1-2 days | Reach Cline IDE users | 🟢 Ready |
| **WinGet** | Windows package | Medium | 3-7 days | Windows 10/11 support | 🟡 Need MSI |
| **Fedora COPR** | Linux (Fedora) | Medium | 1 day | Easy Fedora install | 🟡 Need RPM |
| **Ubuntu PPA** | Linux (Ubuntu) | Medium | 1-2 days | Easy Ubuntu install | 🟡 Need build |
| **Scoop** | Windows CLI | Low | 1-2 days | Dev-friendly Windows | 🟢 Ready |

---

## Phase 1: Priority 1 (DO THIS FIRST - TODAY)

### 1A. MCP Registry (Official)
**Status**: Ready immediately
**Effort**: 15 minutes

```bash
# 1. Install mcp-publisher
brew install mcp-publisher  # or download from GitHub releases

# 2. Create server.json metadata
cat > server.json << 'EOJSON'
{
  "$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
  "name": "io.github.enreign/pagerunner",
  "description": "Chrome browser automation MCP server for AI agents — drives real Chrome with your profiles",
  "repository": {
    "url": "https://github.com/Enreign/pagerunner",
    "source": "github"
  },
  "version": "0.1.1",
  "packages": [
    {
      "registryType": "cargo",
      "identifier": "pagerunner",
      "version": "0.1.1",
      "transport": {
        "type": "stdio"
      }
    }
  ]
}
EOJSON

# 3. Authenticate with GitHub
mcp-publisher login github

# 4. Publish
mcp-publisher publish

# 5. Verify on registry
# https://registry.modelcontextprotocol.io/servers/io.github.enreign/pagerunner
```

**URL**: [registry.modelcontextprotocol.io](https://registry.modelcontextprotocol.io)
**Approval**: Immediate (automated validation)
**Impact**: High — Official MCP registry, used by Claude Desktop and all MCP clients

---

### 1B. crates.io (If not already published)
**Status**: Ready immediately
**Effort**: 5 minutes

```bash
# Check if already published
cargo search pagerunner

# If not published:
cargo publish

# Verify
# https://crates.io/crates/pagerunner
```

**Notes**:
- Cargo.toml already has all required metadata
- Can enable Trusted Publishing (GitHub Actions OIDC) to avoid API tokens
- Enables `cargo install pagerunner` directly

---

### 1C. GitHub Releases (Binary Distribution Foundation)
**Status**: Already done for v0.1.1
**Verify**:

```bash
# Check existing releases
gh release list

# Already has: pagerunner-macos-arm64, pagerunner-macos-x86_64, pagerunner-linux-x86_64
# All with SHA256 checksums
```

**What this enables**:
- Homebrew formula references these
- WinGet manifests point here
- Independent installers download from here
- Backup distribution if package managers fail

---

## Phase 2: Priority 2 (DO THIS WEEK - 3-5 days)

### 2A. Homebrew Tap (Custom)
**Status**: Ready (formulas reference existing releases)
**Effort**: 1 hour

```bash
# 1. Create GitHub repo
gh repo create homebrew-pagerunner --public --readme

# 2. Clone and structure
git clone https://github.com/YOUR-USERNAME/homebrew-pagerunner
cd homebrew-pagerunner
mkdir -p Formula

# 3. Create formula (Formula/pagerunner.rb)
cat > Formula/pagerunner.rb << 'EORB'
class Pagerunner < Formula
  desc "Chrome browser automation MCP server for AI agents"
  homepage "https://github.com/Enreign/pagerunner"
  
  # macOS arm64 (M1/M2)
  on_macos do
    on_arm do
      url "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-macos-arm64"
      sha256 "REPLACE_WITH_ARM64_SHA256"
    end
    
    on_intel do
      url "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-macos-x86_64"
      sha256 "REPLACE_WITH_X86_SHA256"
    end
  end
  
  on_linux do
    url "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-linux-x86_64"
    sha256 "REPLACE_WITH_LINUX_SHA256"
  end
  
  version "0.1.1"
  
  def install
    bin.install "pagerunner-macos-arm64" => "pagerunner" if OS.mac? && Hardware::CPU.arm?
    bin.install "pagerunner-macos-x86_64" => "pagerunner" if OS.mac? && Hardware::CPU.intel?
    bin.install "pagerunner-linux-x86_64" => "pagerunner" if OS.linux?
  end
end
EORB

# 4. Test locally
brew install ./Formula/pagerunner.rb
pagerunner --version

# 5. Push to GitHub
git add .
git commit -m "Initial commit: Add pagerunner formula"
git push origin main

# 6. Users then install with:
# brew tap YOUR-USERNAME/pagerunner
# brew install pagerunner
```

**Get SHA256 checksums**:
```bash
cd /tmp
curl -L -O https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-macos-arm64
shasum -a 256 pagerunner-macos-arm64
```

**Document in project README**:
```markdown
### macOS (Homebrew)
```bash
brew tap enreign/pagerunner
brew install pagerunner
```
```

---

### 2B. Cline MCP Marketplace
**Status**: Ready immediately
**Effort**: 30 minutes

**Steps**:
1. Prepare 400×400 PNG logo (Pagerunner brand colors)
2. Open issue on [github.com/cline/mcp-marketplace](https://github.com/cline/mcp-marketplace) with:

```
Title: Add Pagerunner MCP Server

## Server Information
- **Name**: Pagerunner
- **Repository**: https://github.com/Enreign/pagerunner
- **Description**: Chrome browser automation MCP server for AI agents — drives real Chrome with your profiles

## Installation
Users can install via:
```bash
npm install @pagerunner/mcp
# or
brew install enreign/pagerunner/pagerunner
```

## Why Include?
- 27 MCP tools for browser automation
- Real Chrome instance (not headless browser simulation)
- Security features (SSRF protection, prompt injection mitigation, PII anonymization)
- Session persistence and snapshot capabilities
- Active development and maintenance

## Logo
[Attach PNG 400×400]

## Verification
Repository: https://github.com/Enreign/pagerunner
Install test: `pagerunner --version`
MCP test: `pagerunner mcp`
```

**Approval**: 1-2 days (team response time)
**Impact**: Direct integration into Cline IDE, millions of potential users

---

## Phase 3: Priority 3 (OPTIONAL - DO AFTER PHASE 2)

### 3A. WinGet (Windows Package Manager)
**Status**: Requires MSI installer
**Effort**: 2-3 hours

**Prerequisites**:
- Create `.msi` installer for Windows
- Can use tools like WiX, Inno Setup, or NSIS
- Or use `cargo-wix` for Rust projects

```bash
# Install WiX toolset (Windows)
cargo install cargo-wix

# Build MSI
cargo wix

# This creates pagerunner-0.1.1-x86_64.msi
```

**Then submit**:
```bash
# Install wingetcreate
winget install Microsoft.WinGetCreate

# Create manifest from MSI
wingetcreate new https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-0.1.1-x86_64.msi

# Validate
winget validate <generated-manifest>

# Submit PR to https://github.com/microsoft/winget-pkgs
```

**Approval**: 3-7 days
**Impact**: Native Windows 10/11 `winget install Enreign.Pagerunner`

---

### 3B. Fedora COPR (Linux)
**Status**: Ready (can build from source)
**Effort**: 1-2 hours

```bash
# 1. Create Fedora account at https://copr.fedorainfracloud.org

# 2. Create new project in COPR web UI
# Name: pagerunner
# Build target: Fedora Rawhide, Fedora 41, EPEL 9+

# 3. Add webhook from GitHub
# Triggers rebuild on each release

# 4. Users install with:
# sudo dnf copr enable enreign/pagerunner
# sudo dnf install pagerunner
```

**Approval**: Automatic (no review)
**Impact**: Easy Fedora/RHEL installation

---

### 3C. Ubuntu PPA (Linux)
**Status**: Requires Debian/Ubuntu packaging
**Effort**: 2-3 hours

**Steps**:
1. Create Launchpad account
2. Create PPA at https://launchpad.net/~your-username/+ppas
3. Build `.deb` packages
4. Upload to PPA
5. Document: `sudo add-apt-repository ppa:your-username/pagerunner`

**Approval**: 1-2 days
**Impact**: Easy Ubuntu/Debian installation

---

### 3D. Scoop (Windows - Developer Friendly)
**Status**: Ready (manifest only)
**Effort**: 30 minutes

```bash
# Create manifest (scoop-pagerunner.json)
{
  "version": "0.1.1",
  "description": "Chrome browser automation MCP server for AI agents",
  "homepage": "https://github.com/Enreign/pagerunner",
  "license": "MIT",
  "architecture": {
    "64bit": {
      "url": "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-x86_64-pc-windows-msvc.zip",
      "hash": "SHA256_HASH_HERE"
    }
  },
  "bin": [
    "pagerunner.exe"
  ]
}

# Submit to https://github.com/ScoopInstaller/Main
# Users install with: scoop install pagerunner
```

**Approval**: 1-2 days
**Impact**: Developer-friendly Windows distribution

---

## CLI-Specific Distribution (Beyond MCP)

### Linux System Packages
For users who want `pagerunner` in their system `$PATH` without Homebrew/Cargo:

**Option A: Fedora Package** (via COPR)
- Users: `sudo dnf install pagerunner`
- Setup: COPR + GitHub Actions integration
- Effort: Medium (2-3 hours)

**Option B: Ubuntu PPA**
- Users: `sudo apt install pagerunner`
- Setup: Launchpad + Debian packaging
- Effort: Medium (2-3 hours)

**Option C: Arch User Repository (AUR)**
- Users: `yay -S pagerunner` or `pamac install pagerunner`
- Setup: Minimal (create PKGBUILD)
- Effort: Low (1 hour)
- URL: https://aur.archlinux.org

### macOS System Integration

**Option A: Homebrew tap** (already planned)
- Already covers this with custom tap

**Option B: Homebrew core** (long-term)
- Submit PR to [homebrew/homebrew-core](https://github.com/Homebrew/homebrew-core)
- Requires: Established project, active maintenance
- Timeline: 1-4 weeks
- Benefit: Pre-installed on many dev machines

---

## Summary: Recommended Timeline

```
Week 1 (This Week):
├─ TODAY:
│  ├─ ✅ Publish to MCP Registry (15 min)
│  ├─ ✅ Verify crates.io publication (5 min)
│  └─ ✅ Verify GitHub Releases (already done)
│
├─ Tomorrow:
│  ├─ Create Homebrew tap repo (1 hour)
│  ├─ Test Homebrew installation locally
│  └─ Document in README
│
└─ This Week:
   ├─ Submit to Cline Marketplace (30 min)
   └─ Monitor approval (1-2 days)

Week 2 (Next Week):
├─ Build Windows MSI (2-3 hours)
├─ Submit to WinGet (30 min setup)
├─ Monitor WinGet approval (3-7 days)
│
├─ Create COPR repo (1-2 hours)
├─ Test Fedora installation
│
└─ (Optional) Create Ubuntu PPA (2-3 hours)
```

---

## Verified Status Check

```bash
# After each publication, verify with:

# 1. MCP Registry
curl https://registry.modelcontextprotocol.io/api/servers/io.github.enreign/pagerunner | jq .

# 2. crates.io
cargo search pagerunner

# 3. Homebrew
brew info enreign/pagerunner/pagerunner

# 4. Cline Marketplace
# https://cline.bot/mcp-marketplace (search for pagerunner)

# 5. WinGet (after approval)
winget search Pagerunner

# 6. GitHub releases
gh release list --repo Enreign/pagerunner
```

---

## Files to Create/Update

1. ✅ `server.json` — MCP Registry metadata
2. ✅ `Formula/pagerunner.rb` — Homebrew formula (new repo)
3. 🟡 Windows `.msi` installer (WinGet)
4. 🟡 Fedora `.spec` file (COPR)
5. 🟡 Ubuntu `.deb` build files (PPA)
6. ✅ Update `README.md` with install methods

---

## Documentation Updates Needed

Add to README.md:
```markdown
## Installation

### macOS
```bash
brew tap enreign/pagerunner
brew install pagerunner
```

### Linux
```bash
# Ubuntu
sudo add-apt-repository ppa:enreign/pagerunner
sudo apt install pagerunner

# Fedora
sudo dnf copr enable enreign/pagerunner
sudo dnf install pagerunner

# Arch
yay -S pagerunner
```

### Windows
```bash
# WinGet (Windows 10/11)
winget install Enreign.Pagerunner

# Scoop
scoop bucket add enreign https://github.com/enreign/scoop-bucket
scoop install pagerunner

# Cargo
cargo install pagerunner
```

### MCP Integration
```bash
# Register with Claude Code
/mcp /usr/local/bin/pagerunner mcp
```
```

---

## Success Metrics

- [ ] Listed on 5+ registries
- [ ] Accessible via 3+ package managers
- [ ] 100+ users in first month
- [ ] Positive community feedback
- [ ] Real-world use cases documented

---

**Recommended First Action**: Publish to MCP Registry TODAY (15 minutes), then Homebrew tap tomorrow (1 hour).
