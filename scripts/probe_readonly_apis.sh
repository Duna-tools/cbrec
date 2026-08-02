#!/usr/bin/env bash

# Probes a fixed allowlist of read-only endpoints without credentials or response bodies.

set -euo pipefail

probe_endpoint() {
  local label="$1"
  local url="$2"
  local metadata
  local http_code
  local content_type
  local size
  local status="FAIL"

  if ! metadata="$(curl --silent --show-error --location \
    --proto '=https' \
    --proto-redir '=https' \
    --max-redirs 3 \
    --connect-timeout 5 \
    --max-time 15 \
    --compressed \
    --user-agent 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36' \
    --header 'Accept: application/json' \
    --output /dev/null \
    --write-out '%{http_code}|%{content_type}|%{size_download}' \
    "$url")"; then
    printf '%-20s %-8s %s\n' "$label" "BLOCKED" "request failed"
    return 1
  fi

  IFS='|' read -r http_code content_type size <<< "$metadata"
  if [[ "$http_code" == 200 && "$content_type" == application/json* && "$size" != 0 ]]; then
    status="PASS"
  fi

  printf '%-20s %-8s HTTP=%s type=%s bytes=%s\n' \
    "$label" "$status" "$http_code" "$content_type" "$size"
  [[ "$status" == "PASS" ]]
}

main() {
  local model_name="${1:-}"
  local base_url="https://chaturbate.com"
  local failures=0

  if [[ ! "$model_name" =~ ^[A-Za-z0-9_]{1,20}$ ]]; then
    printf 'Usage: %s <public-model-name>\n' "${0##*/}" >&2
    return 2
  fi

  probe_endpoint "room-list" \
    "$base_url/api/ts/roomlist/room-list/?limit=1&offset=0" || ((failures += 1))
  probe_endpoint "chat-video-context" \
    "$base_url/api/chatvideocontext/$model_name/" || ((failures += 1))
  probe_endpoint "bio-context" \
    "$base_url/api/biocontext/$model_name/" || ((failures += 1))
  probe_endpoint "panel-context" \
    "$base_url/api/panel_context/$model_name/" || ((failures += 1))
  probe_endpoint "current-game" \
    "$base_url/api/ts/games/current/room/$model_name" || ((failures += 1))
  probe_endpoint "more-like" \
    "$base_url/api/more_like/$model_name/" || ((failures += 1))
  probe_endpoint "all-tags" \
    "$base_url/api/ts/roomlist/all-tags/?limit=1&offset=0" || ((failures += 1))
  probe_endpoint "top-tags" \
    "$base_url/api/ts/hashtags/top_tags/?count=1" || ((failures += 1))

  return "$failures"
}

main "$@"
