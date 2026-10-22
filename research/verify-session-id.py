#!/usr/bin/env python3
"""Verify the opencode session-id format against the local session store.

Ports the id generator found in the opencode binary (chunk-wy9a8t00.js) and
checks every session id in ~/.local/share/opencode/opencode.db:

  * the id matches ses_[0-9a-f]{12}[0-9A-Za-z]{14}
  * the 12 hex characters are the inverted, big-endian 6-byte encoding of
    Date.now() * 0x1000 + counter
  * the timestamp recovered from the prefix matches the stored creation time

Usage:
    verify-session-id.py [db-path]
"""
import re
import sqlite3
import sys

DB = sys.argv[1] if len(sys.argv) > 1 else "/root/.local/share/opencode/opencode.db"
SHAPE = re.compile(r"^ses_[0-9a-f]{12}[0-9A-Za-z]{14}$")


def prefix_of(now_ms, counter=1, invert=True):
    """Port of QZ() from the opencode binary: the 12 hex characters."""
    r = now_ms * 0x1000 + counter
    a = ~r if invert else r
    return "".join(f"{(a >> (40 - 8 * h)) & 0xff:02x}" for h in range(6))


def decode(session_id):
    """Recover the creation time (ms, modulo 2**36) from a session id."""
    inverted = bytes.fromhex(session_id[4:16])
    raw = bytes((~byte) & 0xFF for byte in inverted)
    return int.from_bytes(raw, "big") >> 12


connection = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
rows = list(connection.execute("select id, time_created from session_v2 order by time_created"))

print(f"{'id':36s} {'shape':6s} {'delta_ms':>9s} {'counter':>8s} {'rebuilt':8s}")
bad = 0
for session_id, created in rows:
    shape_ok = bool(SHAPE.match(session_id))
    decoded = decode(session_id)
    delta = decoded - (created % (1 << 36))
    counter = None
    rebuilt = False
    for candidate in range(created - 2000, created + 2000):
        for value in range(1, 16):
            if prefix_of(candidate, value) == session_id[4:16]:
                counter, rebuilt = value, True
                break
        if rebuilt:
            break
    if not shape_ok or not rebuilt:
        bad += 1
    print(f"{session_id:36s} {'ok' if shape_ok else 'BAD':6s} {delta:>9d} "
          f"{str(counter):>8s} {'yes' if rebuilt else 'NO':8s}")

print(f"\n{len(rows)} sessions, {bad} mismatches")
print("delta_ms is the id creation time minus the stored time_created;")
print("the id is minted a moment before the row is written.")
