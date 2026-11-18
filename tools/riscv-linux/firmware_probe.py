"""Verify real firmware handoff, SBI TIME and a UART interrupt on Hesper."""

import subprocess
import sys

from build_guest import FW_ISA, IMAGE_DIR, KERNEL, LOGS, command, guest_build_dir, logged, verified_manifest
from sources import PROFILE


def main():
    manifest = verified_manifest()
    assemblers = list((guest_build_dir() / "buildroot/host/bin").glob("riscv64-*-linux-musl-as"))
    if len(assemblers) != 1:
        raise ValueError("expected the prepared cross-assembler; run make riscv-linux-data")
    prefix = str(assemblers[0]).removesuffix("as")
    LOGS.mkdir(parents=True, exist_ok=True)
    obj = LOGS / "opensbi-probe.o"
    elf = LOGS / "opensbi-probe.elf"
    binary = LOGS / "opensbi-probe.bin"
    command(prefix + "as", f"-march={FW_ISA}", "-mabi=lp64", PROFILE / "opensbi-probe.S", "-o", obj)
    command(prefix + "ld", f"-Ttext={KERNEL:#x}", "--no-relax", obj, "-o", elf)
    command(prefix + "objcopy", "-O", "binary", elf, binary)
    logged("l5c-opensbi-probe", "cargo", "run", "--locked", "--release", "-p", "hesper-riscv-linux",
           "--example", "opensbi_probe", "--", IMAGE_DIR, manifest["images"][0]["end"], binary)
    log = LOGS / "l5c-opensbi-probe.log"
    print(log.read_text().splitlines()[-1])
    print(f"OpenSBI handoff, SBI TIME, and serial IRQ passed: {log}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")
