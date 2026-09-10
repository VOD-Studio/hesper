#!/usr/bin/env python3

import argparse
import hashlib
import re
import sys
from pathlib import Path

SOURCE = Path("crates/apple1/tests/wozmon.rs")
DECLARATION = re.compile(r"WOZMON\s*:\s*\[u8;\s*256\]\s*=\s*\[")
BYTE = re.compile(r"0x([0-9A-Fa-f]{2})")


def main() -> None:
    parser = argparse.ArgumentParser(description="Extract the Woz Monitor ROM")
    parser.add_argument("output", nargs="?", help="output file (default: stdout)")
    args = parser.parse_args()

    try:
        source = SOURCE.read_text()
        declaration = DECLARATION.search(source)
        if declaration is None:
            raise ValueError("WOZMON declaration not found")
        body, separator, _ = source[declaration.end() :].partition("]")
        if not separator:
            raise ValueError("WOZMON array has no closing bracket")

        rom = bytes(int(value, 16) for value in BYTE.findall(body))
        if len(rom) != 256:
            raise ValueError(f"expected 256 bytes, extracted {len(rom)}")

        if args.output:
            Path(args.output).write_bytes(rom)
        else:
            sys.stdout.buffer.write(rom)
        print(f"SHA-256: {hashlib.sha256(rom).hexdigest()}", file=sys.stderr)
    except (OSError, ValueError) as error:
        parser.exit(1, f"error: {error}\n")


if __name__ == "__main__":
    main()
