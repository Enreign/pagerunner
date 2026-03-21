#!/usr/bin/env bash
set -euo pipefail

BINARY="$(cd "$(dirname "$0")/.." && pwd)/target/release/pagerunner"
PLIST="$HOME/Library/LaunchAgents/com.pagerunner.daemon.plist"

if [[ ! -f "$BINARY" ]]; then
    echo "Error: binary not found at $BINARY — run 'cargo build --release' first"
    exit 1
fi

mkdir -p "$HOME/Library/LaunchAgents"

cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.pagerunner.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>${BINARY}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>${HOME}/.pagerunner/daemon.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
EOF

launchctl unload "$PLIST" 2>/dev/null || true
launchctl load "$PLIST"

echo "Daemon installed and started. Logs: ~/.pagerunner/daemon.log"
echo "To uninstall: launchctl unload $PLIST && rm $PLIST"
