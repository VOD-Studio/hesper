#!/usr/bin/env python3
"""Verify a user-supplied Woz Monitor ROM image against the recorded
fingerprint.

This project never downloads, embeds, or redistributes the Apple I Woz
Monitor ROM (see AGENTS.md and docs/roadmap.md M3.1): it is Apple's own
firmware, not project code. Acquire a copy you have the rights to use,
then point this script at the file to confirm it is the expected 256-byte
image before pointing `HESPER_APPLE1_ROM` or `hesper apple1 --rom` at it.
"""

import argparse
import hashlib
import sys
from pathlib import Path

EXPECTED_SHA256 = "e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25"
EXPECTED_SIZE = 256


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rom", help="path to a 256-byte Woz Monitor ROM image")
    args = parser.parse_args()

    try:
        data = Path(args.rom).read_bytes()
    except OSError as error:
        parser.exit(1, f"error: cannot read {args.rom}: {error}\n")

    if len(data) != EXPECTED_SIZE:
        parser.exit(
            1,
            f"error: {args.rom} is {len(data)} bytes, expected {EXPECTED_SIZE}\n",
        )

    digest = hashlib.sha256(data).hexdigest()
    if digest != EXPECTED_SHA256:
        parser.exit(
            1,
            "error: SHA-256 mismatch\n"
            f"  got:      {digest}\n"
            f"  expected: {EXPECTED_SHA256}\n",
        )

    print(f"OK: {args.rom} matches the recorded Woz Monitor fingerprint ({digest})")


if __name__ == "__main__":
    main()
