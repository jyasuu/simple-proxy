#!/bin/sh
# Start the OpenCode Zen proxy in the foreground.
#
#   ./run-zen.sh              # proxy on 127.0.0.1:8181 -> https://opencode.ai
#   ./run-zen.sh other.toml   # use a different config
#
# Request/response bodies are appended to the file named by `dump.file` in the
# config (default: /tmp/opencode/zen-dump.jsonl).
set -e
cd "$(dirname "$0")"
exec ./target/release/proxy "${1:-conf-zen.toml}"
