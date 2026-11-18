"""Build a reproducible replacement root and cold boot it under pinned QEMU."""

import json
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import time

from build_guest import (IMAGE_DIR, IMAGE_LOCK, INITRD, board_dtb, capture, check_elf, command,
                         guest_build_dir, guest_compiler, logged, verified_manifest)
from build_qemu import QEMU, version
from initramfs_acceptance import custom_states, default_states
from newc import write_newc
from reference import run
from sources import CACHE, PROFILE, require_tools, sha256

OVERLAY = PROFILE / "custom-root"


def build_root(directory, gcc, locked_cpio):
    directory.mkdir()
    readelf = str(gcc).removesuffix("gcc") + "readelf"
    proof = directory / "root-proof"
    logged("compile-root-proof", gcc, "-march=rv64imac", "-mabi=lp64", "-Os", "-static", "-s",
           "-Wall", "-Wextra", "-Werror", "-o", proof, OVERLAY / "root-proof.c", logs=directory)
    check_elf(proof, readelf, "root-proof", directory)
    # Use the exact BusyBox bytes carried by the authenticated stock archive.
    busybox = directory / "busybox"
    with locked_cpio.open("rb") as source, busybox.open("wb") as output:
        command("cpio", "-i", "--quiet", "--to-stdout", "bin/busybox", stdin=source, stdout=output)
    check_elf(busybox, readelf, "busybox", directory)

    entries = [(name, stat.S_IFDIR | 0o755, b"", 0, 0) for name in ("bin", "dev", "proc", "sys")]
    entries += [
        ("dev/console", stat.S_IFCHR | 0o600, b"", 5, 1),
        ("bin/busybox", stat.S_IFREG | 0o755, busybox.read_bytes(), 0, 0),
        ("bin/root-proof", stat.S_IFREG | 0o755, proof.read_bytes(), 0, 0),
    ]
    entries += [(f"bin/{name}", stat.S_IFLNK | 0o777, b"busybox", 0, 0)
                for name in ("mount", "sh", "sync", "umount", "uname")]
    entries += [("init", stat.S_IFREG | 0o755, (OVERLAY / "init").read_bytes(), 0, 0),
                ("TRAILER!!!", 0, b"", 0, 0)]
    archive = directory / "initramfs.cpio"
    write_newc(archive, entries)
    with archive.open("rb") as stream:
        logged("initramfs-list", "cpio", "-itv", stdin=stream, logs=directory)
    return archive


def main():
    require_tools("cpio", "dtc", "fdtget")
    if not QEMU.is_file():
        raise FileNotFoundError(f"{QEMU}: run make riscv-linux-qemu first")
    manifest = verified_manifest()
    gcc = guest_compiler(guest_build_dir() / "buildroot")
    compiler = capture(gcc, "--version").splitlines()[0]
    if compiler != manifest["compiler"]:
        raise ValueError(f"{gcc}: compiler differs from the image lock: {compiler}")
    base = CACHE / "initramfs"
    base.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix="reference-", dir=base))
    started = time.monotonic()
    stock = IMAGE_DIR / "initramfs.cpio"
    first = build_root(evidence / "a", gcc, stock)
    second = build_root(evidence / "b", gcc, stock)
    digest = sha256(first)
    if digest != sha256(second):
        raise ValueError(f"independent archive builds differ; see {evidence}")
    size = first.stat().st_size
    if size == stock.stat().st_size:
        raise ValueError(f"{first}: length must differ from the stock archive")
    end = INITRD + size
    dtb, _ = board_dtb(first.parent, end, first.parent, False)
    release = manifest["kernelrelease"]
    qemu = version()
    custom = run(manifest, qemu, logs=evidence / "custom", replacement=(first, dtb),
                 states=custom_states(release))
    control = run(manifest, qemu, logs=evidence / "default", states=default_states(release))
    report = dict(profile="serial", base_lock_sha256=sha256(IMAGE_LOCK), manifest=manifest,
                  compiler=compiler, dtc=capture("dtc", "--version"), cpio=capture("cpio", "--version").splitlines()[0],
                  initramfs=dict(bytes=size, sha256=digest, initrd_end=f"{end:#x}"),
                  independent_sha256=digest, reference_dtb_sha256=sha256(dtb), custom_boot=custom,
                  default_boot=control, runtime_seconds=time.monotonic() - started, evidence=str(evidence))
    (evidence / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    # Publish a stable handoff only after both reference sessions have passed.
    with tempfile.TemporaryDirectory(dir=base) as temporary:
        staging = Path(temporary)
        for source in (first, evidence / "report.json"):
            shutil.copyfile(source, staging / source.name)
        for name in ("initramfs.cpio", "report.json"):
            (staging / name).replace(base / name)
    print(f"QEMU booted the alternate root and the locked control: {base / 'report.json'}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
