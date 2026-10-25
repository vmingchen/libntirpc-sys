#!/usr/bin/env python3
"""Generate deterministic starting corpora; discoveries stay in ignored dirs."""
from pathlib import Path
import struct

root = Path(__file__).resolve().parent / "corpus"


def op(code, payload=b""):
    return bytes([code]) + struct.pack("<H", len(payload)) + payload


def wire(text):
    return struct.pack(">I", len(text)) + text + b"\0" * (-len(text) % 4)


seeds = {
    "roundtrip": op(3, b"hello") + op(5) + op(1) + op(2) + op(1) + op(7),
    "failure-recovery": op(3, b"x" * 128) + op(4) + op(3, b"ok") + op(5) + op(1) + op(6),
    "grow-shrink-retain": op(0, wire(b"z" * 9000)) + op(1) + op(0) + op(1) + op(7),
    "truncated": b"".join(op(0, wire(b"abc")[:n]) + op(1) + op(2) + op(1) for n in range(8)),
    "non-utf8-and-nul": op(0, wire(b"\xff\0a")) + op(1) + op(6) + op(7),
}
for name, data in seeds.items():
    path = root / "xdr_state" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)

for name, data in {
    "empty": b"\0" * 4,
    "string": wire(b"abc"),
    "oversized": b"\xff" * 4,
    "call": struct.pack(">10I", 1, 0, 2, 7, 1, 1, 0, 0, 0, 0),
    "reply": struct.pack(">6I", 1, 1, 0, 0, 0, 0),
}.items():
    path = root / "rpc_xdr" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
