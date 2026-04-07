#!/usr/bin/env bash
# Pagerunner Chrome extension — native messaging host.
#
# Chrome native messaging protocol:
#   Input  (from Chrome): 4-byte LE uint32 length prefix + JSON payload
#   Output (to Chrome):   4-byte LE uint32 length prefix + JSON payload
#
# This host bridges Chrome to the Pagerunner daemon Unix socket
# (~/.pagerunner/daemon.sock).  Each message from Chrome is forwarded
# to the daemon as a newline-terminated JSON line; the daemon's newline-
# terminated JSON response is wrapped and sent back.
#
# The request format expected by the daemon:
#   { "id": "...", "tool": "...", "args": { ... } }
#
# The response format from the daemon:
#   { "id": "...", "result": "<json-string>", "error": "..." }

set -euo pipefail

SOCKET="${HOME}/.pagerunner/daemon.sock"

# ── Helpers ───────────────────────────────────────────────────────────────────

# Read exactly N bytes from stdin into a variable (as a hex string).
read_bytes() {
  local n="$1"
  dd bs=1 count="$n" 2>/dev/null | xxd -p | tr -d '\n'
}

# Decode a 4-byte little-endian hex string to a decimal integer.
le32_to_int() {
  local hex="$1"
  # Bytes are b0 b1 b2 b3 (LE). Reverse to big-endian for printf.
  local b0="${hex:0:2}" b1="${hex:2:2}" b2="${hex:4:2}" b3="${hex:6:2}"
  printf '%d' "0x${b3}${b2}${b1}${b0}"
}

# Encode an integer as a 4-byte little-endian binary string and write to stdout.
int_to_le32() {
  local n="$1"
  printf "\\x$(printf '%02x' $((n & 0xff)))"
  printf "\\x$(printf '%02x' $(((n >> 8) & 0xff)))"
  printf "\\x$(printf '%02x' $(((n >> 16) & 0xff)))"
  printf "\\x$(printf '%02x' $(((n >> 24) & 0xff)))"
}

# Send a JSON string back to Chrome (length-prefixed).
send_response() {
  local json="$1"
  local len="${#json}"
  int_to_le32 "$len"
  printf '%s' "$json"
}

# Send an error response JSON object back to Chrome.
send_error() {
  local id="${1:-null}"
  local msg="$2"
  # Escape double-quotes in msg.
  msg="${msg//\"/\\\"}"
  send_response "{\"id\":\"${id}\",\"result\":null,\"error\":\"${msg}\"}"
}

# Forward one request line to the daemon socket and return the response line.
daemon_call() {
  local request_line="$1"
  # Use nc (netcat) to talk to the Unix socket.
  # -U = Unix socket, -q 1 = quit 1s after EOF on stdin.
  if command -v nc >/dev/null 2>&1; then
    printf '%s\n' "$request_line" | nc -U "$SOCKET" -q 1 2>/dev/null
  else
    # Fallback: socat.
    printf '%s\n' "$request_line" | socat - "UNIX-CONNECT:${SOCKET}" 2>/dev/null
  fi
}

# ── Main loop ─────────────────────────────────────────────────────────────────

while true; do
  # Read 4-byte length prefix.
  hex_len="$(read_bytes 4)"
  if [[ -z "$hex_len" || "${#hex_len}" -lt 8 ]]; then
    # stdin closed — Chrome disconnected.
    break
  fi

  msg_len="$(le32_to_int "$hex_len")"

  # Read the JSON payload.
  json_payload="$(dd bs=1 count="$msg_len" 2>/dev/null)"
  if [[ -z "$json_payload" ]]; then
    break
  fi

  # Extract the request ID (simple grep — avoids requiring jq).
  req_id="$(printf '%s' "$json_payload" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)"
  req_id="${req_id:-null}"

  # Forward to daemon.
  if [[ ! -S "$SOCKET" ]]; then
    send_error "$req_id" "Pagerunner daemon is not running (socket not found)"
    continue
  fi

  response="$(daemon_call "$json_payload" 2>&1)" || true

  if [[ -z "$response" ]]; then
    send_error "$req_id" "No response from Pagerunner daemon"
    continue
  fi

  # The response from the daemon is valid JSON — send it back directly.
  send_response "$response"
done
