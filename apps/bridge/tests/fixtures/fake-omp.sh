#!/bin/sh
# TEST FIXTURE ONLY. This is not OMP and must never be presented as the real engine.
MODE="__MODE__"
if [ "$1" = "--version" ]; then
  printf '%s\n' 'omp/18.13.3'
  exit 0
fi
printf '%s\n' "$$" > "$0.pid"

if [ "$MODE" = "no-model" ]; then
  printf '%s\n' "No default model selected. Use /login, set an API key environment variable, or select a local model with /model or --model." >&2
  exit 1
fi
if [ "$MODE" = "timeout" ]; then
  sleep 30
  exit 0
fi
if [ "$MODE" = "unsupported" ]; then
  printf '%s\n' '{"type":"ready","protocolVersion":1,"supportedProtocolVersions":[3],"maxFrameBytes":1048576,"maxReassembledFrameBytes":67108864}'
  while IFS= read -r line; do :; done
  exit 0
fi
if [ "$MODE" = "malformed" ]; then
  printf '%s\n' 'this is not json'
fi

SUPPORTED="[1,2]"
if [ "$MODE" = "v1" ]; then
  SUPPORTED="[1]"
fi
printf '{"type":"ready","protocolVersion":1,"supportedProtocolVersions":%s,"maxFrameBytes":1048576,"maxReassembledFrameBytes":67108864}\n' "$SUPPORTED"

start_events() {
  (
    while :; do
      sleep 0.2
      printf '%s\n' '{"type":"fixture_event","note":"test fixture event"}'
    done
  ) &
}

while IFS= read -r line; do
  [ -z "$line" ] && continue
  id=$(printf '%s\n' "$line" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
  case "$line" in
    *negotiate_protocol*)
      if [ "$MODE" = "negotiate-fail" ]; then
        printf '{"type":"response","id":"%s","command":"negotiate_protocol","success":false,"error":"fixture rejection"}\n' "$id"
      else
        printf '{"type":"response","id":"%s","command":"negotiate_protocol","success":true}\n' "$id"
      fi
      ;;
    *get_state*)
      if [ "$MODE" = "state-fail" ]; then
        printf '{"type":"response","id":"%s","command":"get_state","success":false,"error":"fixture state failure"}\n' "$id"
      else
        printf '{"type":"response","id":"%s","command":"get_state","success":true,"data":{"model":{"provider":"fixture","id":"test-model"},"thinkingLevel":"off","isStreaming":false,"isSettled":true,"messageCount":0,"sessionId":"fixture-session","systemPrompt":["must not leak"],"sessionFile":"/tmp/secret-session.jsonl"}}\n' "$id"
        if [ "$MODE" = "events" ]; then
          start_events
        fi
      fi
      ;;
  esac
done
