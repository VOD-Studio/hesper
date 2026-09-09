#!/usr/bin/env python3
"""Explicitly fetch pinned upstream Klaus Dormann test data and verify integrity.

Uses only Python's standard library. Caches files under .cache/cpu6502/klaus/.
Does not rewrite or modify tracked repository files.
"""

import argparse
import hashlib
import re
import subprocess
import tarfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
CC65 = "555282497c3ecf8b313d87d5973093af19c35bd5"  # Upstream tag V2.19
DECIMAL_HASH = "03798ab778456cc350044fdbe28b4078278648892712b994cdbdda09018674e7"
INTERRUPT_HASH = "ecc829d494fd1f4b4262ac5c58e8cb3925f7aa7570ce815e9a3214b6f66a3c58"
CONFIG = '''MEMORY { ZP: start = $0000, size = $0100, file = ""; RAM: start = $0200, size = $FE00, file = %O; }
SEGMENTS { ZEROPAGE: load = ZP, type = zp; CODE: load = RAM, type = rw; }
'''

REVISION = "7954e2dbb49c469ea286070bf46cdd71aeb29e4b"
REPOSITORY = "https://github.com/Klaus2m5/6502_65C02_functional_tests"
RAW_BASE_URL = (
    f"https://raw.githubusercontent.com/Klaus2m5/6502_65C02_functional_tests/{REVISION}"
)

# Pinned upstream files: (relative_path, expected_sha256, expected_size, description)
PINNED_FILES = [
    (
        "bin_files/6502_functional_test.bin",
        "fa12bfc761e6f9057e4cc01a665a7b800ff01ae91f598af1e39a1201d01953fd",
        65536,
        "Pre-assembled NMOS functional test image",
    ),
    (
        "bin_files/6502_functional_test.lst",
        "a85bf71692a7087182f3a574830e74d420d166032f37d385e34940e7bb8d49e3",
        728468,
        "Functional test listing file",
    ),
    (
        "6502_functional_test.a65",
        "f2665bd02288866c2b210b908e3f387926b4c9f0e0af5ad5513c474361ad1265",
        148497,
        "Functional test source",
    ),
    (
        "6502_decimal_test.a65",
        "dfbe4b907c5821d47d9f7f74eeb51197cf4b9f5260375bc1c6c48c008220e1a0",
        9186,
        "Bruce Clark decimal test source",
    ),
    (
        "6502_interrupt_test.a65",
        "3d794b23a1740e650483990a12f9aa8563b915b48668ae86f51076bee450ecf6",
        31198,
        "Interrupt test source",
    ),
    (
        "license.txt",
        "8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903",
        35147,
        "Upstream GPL-3.0-or-later license",
    ),
]


def checked_hash(data: bytes, expected: str, label: str) -> None:
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise ValueError(f"{label}: SHA-256 expected {expected}, got {actual}")


def instructions(source: str) -> list[str]:
    opcodes = ROOT / "crates/cpu6502/tests/data/opcodes.txt"
    ops = {line.split()[1].lower() for line in opcodes.read_text().splitlines()
           if line and not line.startswith("#")}
    result = []
    for line in source.splitlines():
        fields = line.split(";")[0].split()
        for index in range(min(2, len(fields))):
            if fields[index].lower() in ops:
                result.append("".join(fields[index:]).lower().replace("\\1", "arg")
                              .replace("ibit", "arg").replace("skip\\?", "@skip"))
                break
    return result


def prepare_interrupt(cache: Path, assembler: Path) -> None:
    original = (cache / "6502_interrupt_test.a65").read_text()
    source = re.sub(r"(?m)^(\w+)[ \t]+equ[ \t]+", r"\1 = ", original)
    source = re.sub(r"(?m)^[ \t]*noopt[^\n]*$", "", source)
    # Horizontal whitespace only: crossing a newline here would erase trap bodies.
    source = re.sub(r"(?m)^(\w+)[ \t]+macro\b[^\n]*",
                    lambda m: ".macro " + m[1] + (" arg" if m[1] in
                        ["I_set", "I_clr", "push_stat", "set_stat"] else ""), source)
    source = re.sub(r"\bibit\b", "arg", source.replace("\\1", "arg"))
    source = source.replace("skip\\?", "@skip")
    for old, new in [("if", ".if"), ("else", ".else"), ("endif", ".endif"),
                     ("endm", ".endmacro"), ("db", ".byte"), ("dw", ".word"),
                     ("include", ".include")]:
        source = re.sub(r"(?m)^([ \t]*)" + old + r"\b", r"\1" + new, source)
    source = re.sub(r"(?m)^(\w+[ \t]+)ds[ \t]+", r"\1.res ", source).replace("!=", "<>")
    source = re.sub(r"(?m)^[ \t]*(?:data|bss|code)[ \t]*$", "", source)
    for old, new in [("zero_page", "ZEROPAGE"), ("data_segment", "DATA"),
                     ("code_segment", "CODE"), (r"\$fffa", "VECTORS")]:
        source = re.sub(r"(?m)^[ \t]*org " + old + r"[ \t]*$", f'.segment "{new}"', source)
    source = re.sub(r"(?m)^[ \t]*end start[ \t]*$", "", source)
    source = source.replace("        success         ;if you get here everything went well",
                            "PASSED:\n        success         ;if you get here everything went well")
    source = ".feature labels_without_colons\n.export start, PASSED, I_port\n" + source
    if len(instructions(original)) != 505 or instructions(source) != instructions(original):
        raise ValueError("interrupt adapter must preserve all 505 original instruction statements")
    checked_hash(source.encode(), "f2ce31cba447eef9ad0a292a5d616b85e4b1ba1b947abaf168d3c00fcad0b1d9", "interrupt adapter")
    (cache / "interrupt-ca65.s").write_text(source)
    (cache / "interrupt.cfg").write_text(
        'MEMORY { ZP: start=$000A,size=$00F6,file=""; RAM: start=$0000,size=$10000,file=%O,fill=yes; }\n'
        'SEGMENTS { ZEROPAGE:load=ZP,type=zp; DATA:load=RAM,type=bss,start=$0200; CODE:load=RAM,type=rw,start=$0400; VECTORS:load=RAM,type=ro,start=$FFFA; }\n')
    subprocess.run([str(assembler / "ca65"), "-o", str(cache / "interrupt.o"),
                    "-l", str(cache / "interrupt.lst"), str(cache / "interrupt-ca65.s")], check=True)
    subprocess.run([str(assembler / "ld65"), "-C", str(cache / "interrupt.cfg"),
                    "-o", str(cache / "interrupt.bin"), "-Ln", str(cache / "interrupt.lbl"),
                    str(cache / "interrupt.o")], check=True)
    checked_hash((cache / "interrupt.bin").read_bytes(), INTERRUPT_HASH, "interrupt image")
    if (cache / "interrupt.lbl").read_text().splitlines() != [
        "al 00BFFC .I_port", "al 0006F5 .PASSED", "al 000400 .start"
    ]:
        raise ValueError("interrupt entry, success or feedback address changed")


def prepare_decimal(cache_dir: Path, check_only: bool) -> None:
    if check_only:
        checked_hash((cache_dir / "decimal.bin").read_bytes(), DECIMAL_HASH, "decimal image")
        checked_hash((cache_dir / "interrupt.bin").read_bytes(), INTERRUPT_HASH, "interrupt image")
        return
    tool_cache = ROOT / ".cache/cpu6502/cc65"
    archive = tool_cache / f"{CC65}.tar.gz"
    if archive.exists():
        raw = archive.read_bytes()
    else:
        with urllib.request.urlopen(f"https://codeload.github.com/cc65/cc65/tar.gz/{CC65}", timeout=30) as response:
            raw = response.read(16 * 1024 * 1024 + 1)
    checked_hash(raw, "62c77f00ef4141153a0ddecef06ca086c11c68f14d022beadeaf353d1d833ff1", "cc65 archive")
    if not archive.exists():
        tool_cache.mkdir(parents=True, exist_ok=True)
        archive.write_bytes(raw)
    source_dir = tool_cache / f"cc65-{CC65}"
    if not source_dir.exists():
        with tarfile.open(archive) as source:
            source.extractall(tool_cache, filter="data")
    # One dependency graph prevents duplicate concurrent builds of common objects.
    subprocess.run(["make", "-C", str(source_dir / "src"), "ca65", "ld65", "-j4",
                    "BUILD_ID=Git 55528249"], check=True)
    source = (cache_dir / "6502_decimal_test.a65").read_text()
    for setting in ("cputype", "vld_bcd"):
        if not re.search(rf"^{setting}\s*=\s*0\b", source, re.M):
            raise ValueError(f"expected {setting}=0 in pinned source")
    source, count = re.subn(r"^(chk_[anvzc]\s*=\s*)[01]", r"\g<1>1", source, flags=re.M)
    if count != 5:
        raise ValueError("expected all five A/N/V/Z/C check switches")
    # Translate directives only; all 6502 instructions and oracle code stay intact.
    source = source.replace("end_of_test macro", ".macro end_of_test")
    source = re.sub(r"(?m)^(\s*)db\s+", r"\1.byte ", source)
    source = re.sub(r"(?m)^(\w+\s+)ds\s+", r"\1.res ", source)
    source = re.sub(r'(?m)^\s*bss\s*$', '.segment "ZEROPAGE"', source)
    source = re.sub(r'(?m)^\s*code\s*$', '.segment "CODE"', source)
    source = re.sub(r"(?m)^\s*org\s+[^\n]+$", "; Placement is defined by decimal.cfg.", source)
    for old, new in [("if", ".if"), ("endif", ".endif"), ("endm", ".endmacro")]:
        source = re.sub(r"(?m)^(\s*)" + old + r"\b", r"\1" + new, source)
    source = source.replace("!=", "<>")
    source = re.sub(r"(?m)^\s*end\s+TEST\s*$", "", source)
    source = (
        "; Adapted from the pinned public-domain Bruce Clark test: ca65 directives and all flag checks.\n"
        '.feature labels_without_colons\n.export TEST, DONE\n.exportzp ERROR\n' + source
    )
    if instructions(source) != instructions((cache_dir / "6502_decimal_test.a65").read_text()):
        raise ValueError("decimal adapter changed original instruction statements")
    checked_hash(source.encode(), "586f6f2da4fc8763630f73211356c6de5f8d47cbc01cc38fddcd761a6ed3ec39", "decimal adapter")
    (cache_dir / "decimal-ca65.s").write_text(source)
    (cache_dir / "decimal.cfg").write_text(CONFIG)
    subprocess.run([str(source_dir / "bin/ca65"), "-o", str(cache_dir / "decimal.o"),
                    "-l", str(cache_dir / "decimal.lst"), str(cache_dir / "decimal-ca65.s")], check=True)
    subprocess.run([str(source_dir / "bin/ld65"), "-C", str(cache_dir / "decimal.cfg"),
                    "-o", str(cache_dir / "decimal.bin"), "-Ln", str(cache_dir / "decimal.lbl"),
                    str(cache_dir / "decimal.o")], check=True)
    checked_hash((cache_dir / "decimal.bin").read_bytes(), DECIMAL_HASH, "decimal image")
    if (cache_dir / "decimal.lbl").read_text().splitlines() != [
        "al 00024B .DONE", "al 00000B .ERROR", "al 000200 .TEST"
    ]:
        raise ValueError("decimal entry, DONE or ERROR address changed")
    prepare_interrupt(cache_dir, source_dir / "bin")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary-only",
        action="store_true",
        help="fetch/verify only the pre-assembled functional test binary",
    )
    parser.add_argument(
        "--check-only",
        action="store_true",
        help="verify integrity of cached files without downloading missing ones",
    )
    args = parser.parse_args()

    cache_dir = ROOT / ".cache/cpu6502/klaus" / REVISION
    targets = (
        PINNED_FILES[:1] if args.binary_only else PINNED_FILES
    )

    def prepare_entry(entry: tuple[str, str, int, str]) -> str:
        rel_path, expected_hash, expected_size, desc = entry
        target = cache_dir / rel_path

        if target.exists():
            raw = target.read_bytes()
        elif args.check_only:
            raise ValueError(f"{rel_path}: file missing in cache ({target})")
        else:
            url = f"{RAW_BASE_URL}/{rel_path}"
            target.parent.mkdir(parents=True, exist_ok=True)
            with urllib.request.urlopen(url, timeout=30) as response:
                raw = response.read(expected_size * 2 + 1)

        if len(raw) != expected_size:
            raise ValueError(
                f"{rel_path}: size expected {expected_size}, got {len(raw)}"
            )
        checked_hash(raw, expected_hash, rel_path)

        if not target.exists():
            temporary = target.with_suffix(target.suffix + ".tmp")
            temporary.write_bytes(raw)
            temporary.replace(target)

        return f"{rel_path} ({expected_size} B)"

    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(prepare_entry, targets))

    if not args.binary_only:
        prepare_decimal(cache_dir, args.check_only)

    print(
        f"Verified Klaus Dormann NMOS test data @ {REVISION}\n"
        f"Files ({len(results)}): {', '.join(results)}\n"
        f"Cache directory: {cache_dir}"
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError, tarfile.TarError) as error:
        raise SystemExit(f"prepare_klaus: {error}") from error
