#!/usr/bin/env python3
"""Probe the Zen free-tier gate with different x-session-id shapes.

Every request uses the real agent body captured by the proxy in this repo,
so the only variable is the session id (or the missing header).

Usage:
    probe-session-id.py [dump-file]

The dump file is produced by the proxy (see conf-zen.toml); the first record
in it is the title-generator request, which the gateway accepts.
"""
import json
import secrets
import string
import subprocess
import sys
import time

DUMP = sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode/zen-dump.jsonl"
URL = "https://opencode.ai/zen/v1/chat/completions"
UA = "opencode/latest/2.1.2/cli"
B62 = string.digits + string.ascii_uppercase + string.ascii_lowercase

records = [json.loads(line) for line in open(DUMP)]
body = json.dumps(json.loads(records[0]["body"]))
real_id = records[0]["headers"]["x-session-id"]


def status(session_id=None, payload=body):
    """Return the HTTP status code of a probe request."""
    cmd = [
        "curl", "-sS", "-m", "90", "--compressed", "-o", "/dev/null",
        "-w", "%{http_code}", URL,
        "-H", "content-type: application/json",
        "-H", f"user-agent: {UA}",
    ]
    if session_id:
        cmd += ["-H", f"x-session-id: {session_id}"]
    cmd += ["-d", payload]
    return subprocess.run(cmd, capture_output=True, text=True).stdout.strip()


def prefix(now_ms, counter=1):
    """The 12 hex characters a real opencode session id starts with."""
    a = ~(now_ms * 0x1000 + counter)
    return "".join(f"{(a >> (40 - 8 * h)) & 0xff:02x}" for h in range(6))


def tail(count=14):
    """The 14 random base62 characters of a session id."""
    return "".join(secrets.choice(B62) for _ in range(count))


now = int(time.time() * 1000)
cases = [
    ("real captured id", real_id),
    ("no x-session-id header", None),
    ("ses_ + 12 hex + 14 base62", "ses_" + prefix(now) + tail()),
    ("ses_ + 12 hex + 14 base62, ts = 1970", "ses_" + "0" * 12 + tail()),
    ("ses_ + 12 hex + 14 base62, ts = +1 day", "ses_" + prefix(now + 86_400_000) + tail()),
    ("ses_ + 12 hex + 13 base62 (len 25)", "ses_" + prefix(now) + tail(13)),
    ("ses_ + 12 hex + 15 base62 (len 27)", "ses_" + prefix(now) + tail(15)),
    ("ses_ + 12 hex + 12 base62 (len 24)", "ses_" + prefix(now) + tail(12)),
    ("ses_ + 12 hex only (len 12)", "ses_" + prefix(now)),
    ("ses_ + 12 hex, 'g' at index 0", "ses_g" + prefix(now)[1:] + tail()),
    ("ses_ + 12 hex, 'z' at index 11", "ses_" + prefix(now)[:11] + "z" + tail()),
    ("ses_ + UPPERCASE hex", "ses_" + prefix(now).upper() + tail()),
    ("ses_ + 26 random base62", "ses_" + tail(26)),
    ("ses_ + 26 base62, '-' in the tail", "ses_" + prefix(now) + tail(13) + "-"),
    ("no ses_ prefix", prefix(now) + tail()),
]

print(f"{'case':40s} {'x-session-id':36s} HTTP")
for label, session_id in cases:
    print(f"{label:40s} {str(session_id)[:34]:36s} {status(session_id)}")

print()
print("body checks (session id is the real captured one):")
print("  trivial body                  ", status(real_id, json.dumps(
    {"model": "big-pickle", "messages": [{"role": "user", "content": "hi"}]})))
print("  real body, stream false       ", status(real_id, json.dumps(
    {**json.loads(records[0]["body"]), "stream": False})))
print("  real body, stream true        ", status(real_id))
