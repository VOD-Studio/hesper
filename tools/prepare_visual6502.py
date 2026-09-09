#!/usr/bin/env python3
"""Fetch the pinned Visual6502 revD reference model; never imported by the CPU."""
from pathlib import Path
import hashlib
import json
import urllib.request

root = Path(__file__).resolve().parents[1]
manifest = json.loads((root / "crates/cpu6502/tests/data/visual6502/manifest.json").read_text())
revision = manifest["revision"]
if revision != "d8ecc129b34e0eaf320e0400fcf33329475bdb1e" or manifest["model"] != "NMOS 6502 revD":
    raise SystemExit("unexpected Visual6502 model or revision")
cache = root / ".cache/cpu6502/visual6502" / revision
cache.mkdir(parents=True, exist_ok=True)
for name, digest in manifest["files"].items():
    if Path(name).name != name:
        raise SystemExit("invalid model filename")
    target = cache / name
    if target.exists():
        data = target.read_bytes()
    else:
        with urllib.request.urlopen(f"https://raw.githubusercontent.com/trebonian/visual6502/{revision}/{name}", timeout=30) as response:
            data = response.read(2 * 1024 * 1024 + 1)
    if hashlib.sha256(data).hexdigest() != digest:
        raise SystemExit(f"{name}: SHA-256 mismatch")
    if not target.exists():
        temporary = target.with_suffix(".tmp")
        temporary.write_bytes(data)
        temporary.replace(target)
print(f"Verified Visual6502 revD @ {revision}: {len(manifest['files'])} files")
