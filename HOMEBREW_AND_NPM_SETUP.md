# Homebrew & NPM Distribution Setup

Guide for creating and maintaining Pagerunner's Homebrew tap and future NPM packages.

## Part 1: Homebrew Tap Setup (TODAY)

### Step 1: Create GitHub Repository

```bash
# Create new public repository (via GitHub web or gh CLI)
gh repo create homebrew-pagerunner \
  --public \
  --description "Homebrew tap for Pagerunner — Chrome browser automation MCP server" \
  --homepage "https://github.com/Enreign/pagerunner"
```

Or manually at: https://github.com/new with name `homebrew-pagerunner`

### Step 2: Clone and Set Up Locally

```bash
cd ~/Code
git clone https://github.com/enreign/homebrew-pagerunner
cd homebrew-pagerunner

# Create directory structure
mkdir -p Formula

# Copy the formula
cat > Formula/pagerunner.rb << 'EOF'
class Pagerunner < Formula
  desc "Chrome browser automation MCP server for AI agents"
  homepage "https://github.com/Enreign/pagerunner"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-macos-arm64"
      sha256 "9c79e5b9bf121a504a15daf0a280c7762da03d533dd326182ee3d10669c766f9"
    end

    on_intel do
      url "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-macos-x86_64"
      sha256 "c05ab8ba56495a83976d930901653e7bb98f18c2e0057503804efd849cdf3297"
    end
  end

  on_linux do
    url "https://github.com/Enreign/pagerunner/releases/download/v0.1.1/pagerunner-linux-x86_64"
    sha256 "dba4c03ec524208aa78f67c3c43e1b381554b71803b8fa0b54c5e55ba81ce3c7"
  end

  version "0.1.1"

  def install
    if OS.mac?
      if Hardware::CPU.arm?
        bin.install "pagerunner-macos-arm64" => "pagerunner"
      else
        bin.install "pagerunner-macos-x86_64" => "pagerunner"
      end
    elsif OS.linux?
      bin.install "pagerunner-linux-x86_64" => "pagerunner"
    end
  end

  test do
    system "#{bin}/pagerunner", "--version"
  end
end
EOF
```

### Step 3: Copy Supporting Files

Create `README.md`:
```markdown
# Homebrew Pagerunner Tap

Custom Homebrew tap for [Pagerunner](https://github.com/Enreign/pagerunner) — Chrome browser automation MCP server.

## Installation

```bash
brew tap enreign/pagerunner
brew install pagerunner
```

## Usage

```bash
pagerunner --version
pagerunner init
pagerunner mcp
```

## Documentation

- [Pagerunner README](https://github.com/Enreign/pagerunner#readme)
- [Quick Start](https://github.com/Enreign/pagerunner#quick-start)
- [CLI Commands](https://github.com/Enreign/pagerunner#cli-subcommands)

## Requirements

- macOS 10.13+ or Linux
- Chrome or Chromium browser

## Support

- **Issues**: https://github.com/Enreign/pagerunner/issues
- **Security**: https://github.com/Enreign/pagerunner/blob/main/SECURITY.md
```

Create `.gitignore`:
```
.DS_Store
*.swp
*.swo
.env
.env.local
```

Create `LICENSE` (link to Pagerunner's MIT):
```
MIT License — See https://github.com/Enreign/pagerunner/blob/main/LICENSE
```

### Step 4: Set Up GitHub Actions (Optional)

Create `.github/workflows/tests.yml`:
```yaml
name: Homebrew Formula Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      - name: Audit formula
        run: brew audit --strict Formula/pagerunner.rb
      - name: Install formula
        run: brew install ./Formula/pagerunner.rb
      - name: Test binary
        run: pagerunner --version
```

### Step 5: Push to GitHub

```bash
git add -A
git commit -m "Initial commit: Add pagerunner formula"
git push origin main
```

### Step 6: Test the Tap

```bash
# From a different directory
brew tap enreign/pagerunner
brew install pagerunner
pagerunner --version

# Cleanup
brew uninstall pagerunner
```

### Step 7: Update Formula for New Releases

When Pagerunner releases a new version:

```bash
cd ~/Code/homebrew-pagerunner

# Get SHA256 from GitHub release
curl -sL https://github.com/Enreign/pagerunner/releases/download/vX.X.X/pagerunner-macos-arm64 | shasum -a 256

# Update Formula/pagerunner.rb with:
# - version "X.X.X"
# - New SHA256 values for all three binaries

git add Formula/pagerunner.rb
git commit -m "chore: Update pagerunner formula to vX.X.X"
git push origin main
```

---

## Part 2: NPM Strategy (For Future TypeScript Packages)

### When to Create NPM Packages

Create separate npm packages for:
- **Node.js client** wrapper around Pagerunner (e.g., `@enreign/pagerunner-client`)
- **TypeScript types** (e.g., `@enreign/pagerunner-types`)
- **Web dashboard** (e.g., `@enreign/pagerunner-web`)
- **Shared utilities** (e.g., `@enreign/pagerunner-utils`)

### Setup Guide (When Ready)

#### Step 1: Create NPM Organization

```bash
# At https://npmjs.com:
# 1. Sign up or login
# 2. Create organization: @enreign
# 3. Add team members if needed
```

#### Step 2: Initialize Package Repository

```bash
mkdir pagerunner-ts-client
cd pagerunner-ts-client

npm init --scope=@enreign --yes

# Update package.json with:
{
  "name": "@enreign/pagerunner-client",
  "version": "0.1.0",
  "description": "TypeScript client for Pagerunner MCP server",
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "files": ["dist"],
  "scripts": {
    "build": "tsc",
    "test": "jest",
    "prepublishOnly": "npm run build && npm run test"
  },
  "license": "MIT",
  "publishConfig": {
    "access": "public"
  }
}
```

#### Step 3: TypeScript Configuration

Create `tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "declaration": true,
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  },
  "include": ["src"],
  "exclude": ["node_modules", "dist"]
}
```

#### Step 4: GitHub Actions for NPM Publish

Create `.github/workflows/npm-publish.yml`:
```yaml
name: Publish to NPM

on:
  release:
    types: [created]

jobs:
  publish:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      id-token: write
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-node@v3
        with:
          node-version: '18'
          registry-url: 'https://registry.npmjs.org'

      - name: Install dependencies
        run: npm ci

      - name: Build
        run: npm run build

      - name: Publish to NPM
        run: npm publish
        env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
```

#### Step 5: Publishing

**First time:**
```bash
npm login
npm publish
```

**Subsequent releases:**
- Tag release in GitHub
- GitHub Actions publishes automatically
- Verify on npmjs.com

### Example NPM Packages

**@enreign/pagerunner-client** (Node.js wrapper)
```typescript
import { PagerunnerClient } from '@enreign/pagerunner-client';

const client = new PagerunnerClient();
const session = await client.openSession('default');
await client.navigate(session, targetId, 'https://example.com');
const content = await client.getContent(session, targetId);
```

**@enreign/pagerunner-types** (Shared TypeScript types)
```typescript
export interface PagerunnerSession {
  session_id: string;
  profile: string;
  stealth: boolean;
}

export interface PagerunnerTab {
  target_id: string;
  url: string;
  title: string;
}
```

---

## Maintenance

### Homebrew Updates

- Update formula with every Pagerunner release
- Run `brew audit --strict` before committing
- Test with `brew install ./Formula/pagerunner.rb`

### NPM Packages

- Publish when TypeScript code changes
- Use semantic versioning
- Document breaking changes
- GitHub Actions automates publishing on release

---

## Future Automation

Consider these GitHub Actions workflows:

1. **Auto-update Homebrew formula on release**
   - Fetch new SHA256 checksums from GitHub release
   - Create PR to homebrew-pagerunner repo
   - Merge automatically if tests pass

2. **Sync versions across all packages**
   - When Pagerunner version is bumped, auto-bump npm packages
   - Keep Cargo.toml, package.json, and formula in sync

3. **Cross-package testing**
   - Test Rust code with TS client
   - Ensure type safety between interfaces

---

## Status

- ✅ Homebrew tap: Ready to create today (30 minutes)
- 🟡 NPM packages: Plan documented, implement when TypeScript code is ready
- 📋 Automation: Can add GitHub Actions workflows in parallel
