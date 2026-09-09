#!/usr/bin/env python3
"""Explicitly fetch pinned upstream data and verify the checked-in selection.

Uses only Python's standard library. Does not rewrite tracked fixtures or manifests.
"""

import argparse
import hashlib
import json
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/cpu6502/tests/data/singlestep"


def checked_hash(data, expected, label):
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise ValueError(f"{label}: SHA-256 expected {expected}, got {actual}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full", action="store_true", help="prepare all 151 official opcode files")
    args = parser.parse_args()
    manifest = json.loads((FIXTURES / ("full-manifest.json" if args.full else "manifest.json")).read_text())
    revision = manifest["revision"]
    if (
        manifest["repository"] != "https://github.com/SingleStepTests/65x02"
        or manifest["variant"] != "6502/v1"
        or len(revision) != 40
        or any(c not in "0123456789abcdef" for c in revision)
        or not manifest["files"]
    ):
        raise ValueError("expected a pinned, nonempty NMOS 6502/v1 manifest")
    opcodes = [f["opcode"] for f in manifest["files"]]
    if len(set(opcodes)) != len(opcodes) or any(
        len(op) != 2 or any(c not in "0123456789abcdef" for c in op)
        for op in opcodes
    ):
        raise ValueError("manifest has duplicate or invalid opcode filenames")
    if args.full:
        official = {line.split()[0].lower() for line in (FIXTURES.parent / "opcodes.txt").read_text().splitlines()
                    if line and not line.startswith("#")}
        if len(official) != 151 or set(opcodes) != official or any(
            f["source_count"] != 10000 or f["selected_count"] != 10000
            or f["source_sha256"] != f["fixture_sha256"] for f in manifest["files"]
        ):
            raise ValueError("full manifest must preserve all 10000 cases for each official opcode")
    cache = ROOT / ".cache/cpu6502/singlestep" / revision / "6502/v1"
    cache.mkdir(parents=True, exist_ok=True)

    def prepare(entry):
        name = entry["opcode"] + ".json"
        target = cache / name
        if target.exists():
            raw = target.read_bytes()
        else:
            url = f"https://raw.githubusercontent.com/SingleStepTests/65x02/{revision}/6502/v1/{name}"
            with urllib.request.urlopen(url, timeout=30) as response:
                raw = response.read(16 * 1024 * 1024 + 1)
            if len(raw) > 16 * 1024 * 1024:
                raise ValueError(f"{name}: source exceeds 16 MiB limit")
        checked_hash(raw, entry["source_sha256"], name)
        cases = json.loads(raw)
        count = entry["selected_count"]
        if len(cases) != entry["source_count"] or not 0 < count <= len(cases):
            raise ValueError(f"{name}: unexpected source/selection count")
        if not args.full:
            selected = (
                "[\n"
                + ",\n".join(json.dumps(c, separators=(",", ":")) for c in cases[:count])
                + "\n]\n"
            ).encode()
            checked_hash(selected, entry["fixture_sha256"], f"{name} selection")
            checked_hash((FIXTURES / name).read_bytes(), entry["fixture_sha256"], f"{name} fixture")
        if not target.exists():
            temporary = target.with_suffix(".json.tmp")
            temporary.write_bytes(raw)
            temporary.replace(target)
        return count

    with ThreadPoolExecutor(max_workers=4) as pool:
        counts = list(pool.map(prepare, manifest["files"]))
    print(f"Verified source hashes ({'full official corpus' if args.full else 'original-order fixtures'}): {len(counts)} files, {sum(counts)} cases")
    print(f"Source cache: {cache}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"prepare_singlestep: {error}") from error
