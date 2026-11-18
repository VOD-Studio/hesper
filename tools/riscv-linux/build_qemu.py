"""Build a reference QEMU/TCG binary from the verified release tarball."""

import os
import subprocess
import sys

from build_guest import CACHE, LOGS, capture, logged, unpack
from sources import archives, releases, require_tools

QEMU_OUT = CACHE / "build/qemu-tcg"
QEMU = QEMU_OUT / "qemu-system-riscv64"


def version():
    pinned = next(entry["version"] for entry in releases() if entry["name"] == "qemu")
    version_line = capture(QEMU, "--version").splitlines()[0]
    if version_line != f"QEMU emulator version {pinned}":
        raise ValueError(f"{QEMU}: expected QEMU {pinned}, got {version_line}")
    return version_line


def main():
    require_tools("make", "tar", "gcc", "pkg-config", "ninja")
    source = unpack(archives()["qemu"])
    jobs = int(os.environ.get("RISCV_LINUX_JOBS", len(os.sched_getaffinity(0))))
    if jobs < 1:
        raise ValueError("RISCV_LINUX_JOBS must be positive")
    LOGS.mkdir(parents=True, exist_ok=True)
    QEMU_OUT.mkdir(parents=True, exist_ok=True)
    logged("qemu-configure", source / "configure", "--target-list=riscv64-softmmu", "--without-default-features",
           "--disable-download", "--disable-docs", "--enable-tcg", "--enable-fdt=system", cwd=QEMU_OUT)
    logged("qemu-build", "ninja", f"-j{jobs}", QEMU.name, cwd=QEMU_OUT)
    logged("qemu-cpu-help", QEMU, "-cpu", "help")
    print(version())


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")
