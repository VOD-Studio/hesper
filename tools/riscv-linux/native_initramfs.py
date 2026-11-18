"""Prove a QEMU-accepted initramfs cold boots and composes with the native data disk."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

from build_guest import BLK_IMAGE_DIR, ENV, IMAGE_DIR, IMAGE_NAMES, INITRD, verified_manifest
from disk_acceptance import check_proof, disk_states, make_seed
from initramfs_acceptance import custom_states, default_states, fixture_info, initrd_end_state
from native import BOOT_SECONDS, EXIT_SECONDS, capture_serial, linux_command, require_exit
from native_disk import DISK_BYTES, expect_entries, expect_stderr, require_boot_fuel
from serial_acceptance import PROMPT, STEP_BYTES, STEP_SECONDS, session
from sources import CACHE, require_tools, sha256

LOGS = CACHE / "logs-initramfs"
BASE_FILES = ("manifest.json", *(name for name in IMAGE_NAMES if name != "initramfs.cpio"))
MAX_INITRAMFS_BYTES = 16 * 1024 * 1024
MAX_FUEL = 450_000_000
MARKER = "INITRAMFS_DISK_OK"
KERNEL_PANIC = re.compile(rb"Kernel panic - not syncing: (?:VFS: Unable to mount root fs|No working init found)[^\n]*")


def cpio_image(manifest):
    return next(entry for entry in manifest["images"] if entry["path"] == "initramfs.cpio")


def stage_base(destination, source, *, overrides=None):
    destination.mkdir()
    overrides = {} if overrides is None else overrides
    for name in BASE_FILES:
        shutil.copyfile(overrides.get(name, source / name), destination / name)
    expect_entries(destination, *BASE_FILES)
    return destination


def shell_boot(argv, transport, logs, states, expected_end):
    observed_end = []
    states += (initrd_end_state(expected_end, observed_end.append),)

    def exchange(receive, send):
        return session(receive, send, states, BOOT_SECONDS)

    run = capture_serial(argv, transport, logs=logs, commands=tuple(command for _, command, _ in states),
                         exchange=exchange)
    require_exit(run, "quit")
    require_boot_fuel(run)
    run.update(expected_initrd_end=expected_end, initrd_end=observed_end[0])
    return run


def disk_round_trip(binary, images, manifest, archive, archive_end, seed, seed_hash, logs):
    logs.mkdir()
    first, second = logs / "first.raw", logs / "second.raw"
    release = manifest["kernelrelease"]
    write = shell_boot(linux_command(binary, images=images, disk=(seed, first), initramfs=archive),
                       "pty", logs / "write", custom_states(release) + disk_states(True, MARKER, busybox=True),
                       archive_end)
    if sha256(seed) != seed_hash or first.stat().st_size != DISK_BYTES:
        raise ValueError(f"{logs}: custom root changed its input or exported the wrong size")
    after_write = check_proof(first, MARKER)
    if after_write == seed_hash:
        raise ValueError(f"{first}: write left the disk unchanged")

    read = shell_boot(linux_command(binary, images=images, disk=(first, second)),
                      "pipe", logs / "read", default_states(release) + disk_states(False, MARKER),
                      cpio_image(manifest)["end"])
    first_after_read = sha256(first)
    if first_after_read != after_write or second.stat().st_size != DISK_BYTES:
        raise ValueError(f"{logs}: default root changed its input or exported the wrong size")
    after_read = check_proof(second, MARKER)
    if after_read != after_write:
        raise ValueError(f"{second}: read-only export changed raw disk bytes")
    return dict(first_sha256=after_write, first_after_read_sha256=first_after_read,
                second_sha256=after_read, write=write, read=read)


def preflight_controls(binary, serial_base, blk_images, archive, info, seed, logs):
    logs.mkdir()
    publish = logs / "publish"
    publish.mkdir()
    disk = (seed, publish / "absent.raw")
    result = {}

    def reject(name, argv, *parts):
        stderr_log = logs / f"{name}-stderr.log"
        serial_log = logs / f"{name}-serial.raw"
        started = time.monotonic()
        try:
            check = subprocess.run(argv, env=ENV, stdin=subprocess.DEVNULL, capture_output=True,
                                   timeout=EXIT_SECONDS)
        except subprocess.TimeoutExpired as error:
            stderr_log.write_bytes(error.stderr or b"")
            serial_log.write_bytes(error.stdout or b"")
            raise ValueError(f"{name}: preflight exceeded its deadline; see {stderr_log} and {serial_log}") from error
        stderr_log.write_bytes(check.stderr)
        serial_log.write_bytes(check.stdout)
        if check.returncode <= 0 or check.stdout or b"linux: exit=" in check.stderr:
            raise ValueError(f"{name}: expected rejection before guest execution; status={check.returncode}; "
                             f"see {stderr_log} and {serial_log}")
        stderr = expect_stderr(stderr_log, *parts).strip()
        expect_entries(publish)
        result[name] = dict(command=argv, exit_status=check.returncode, stderr=stderr,
                            runtime_seconds=time.monotonic() - started,
                            diagnostics_log=str(stderr_log), serial_log=str(serial_log))

    base = linux_command(binary, images=serial_base)
    custom = linux_command(binary, images=serial_base, initramfs=archive)
    for name, options in (("missing-value", []), ("option-as-value", ["--help"]), ("empty-value", [""])):
        reject(name, base + ["--initramfs", *options], "--initramfs requires a file")
    reject("duplicate-argument", custom + ["--initramfs", str(archive)], "--initramfs specified more than once")
    reject("default-cpio-absent", base, str(serial_base / "initramfs.cpio"), "No such file or directory")

    bad_dtb = stage_base(logs / "corrupt-dtb", serial_base)
    path = bad_dtb / "board.dtb"
    contents = bytearray(path.read_bytes())
    contents[-1] ^= 1
    path.write_bytes(contents)
    reject("corrupt-dtb", linux_command(binary, images=bad_dtb, initramfs=archive), str(path), "SHA-256 mismatch")

    mixed = stage_base(logs / "mixed-profile", blk_images, overrides={"Image": serial_base / "Image"})
    mismatch = "manifest differs from the embedded image lock"
    reject("serial-kernel-with-blk-lock", linux_command(binary, images=mixed, disk=disk, initramfs=archive),
           str(mixed / "Image"), "size mismatch")
    reject("serial-profile-with-disk", linux_command(binary, images=serial_base, disk=disk, initramfs=archive),
           str(serial_base / "manifest.json"), mismatch)
    reject("blk-profile-without-disk", linux_command(binary, images=blk_images, initramfs=archive),
           str(blk_images / "manifest.json"), mismatch)

    tampered = stage_base(logs / "tampered-manifest", serial_base)
    path = tampered / "manifest.json"
    manifest = json.loads(path.read_bytes())
    cpio_image(manifest).update(bytes=info["bytes"], sha256=info["sha256"], end=info["initrd_end"])
    path.write_text(json.dumps(manifest, indent=2) + "\n")
    reject("tampered-cpio-lock", linux_command(binary, images=tampered, initramfs=archive), str(path), mismatch)

    malformed = logs / "truncated.cpio"
    malformed.write_bytes(b"070701")
    reject("bad-archive", linux_command(binary, images=serial_base, initramfs=malformed),
           str(malformed), "custom initramfs", "malformed")
    for name, size in (("empty-archive", 0), ("oversized-archive", MAX_INITRAMFS_BYTES + 1)):
        path = logs / name
        with path.open("xb") as stream:
            stream.truncate(size)
        reject(name, linux_command(binary, images=serial_base, initramfs=path),
               str(path), f"custom initramfs length must be 1..={MAX_INITRAMFS_BYTES} bytes (found {size})")
    fifo = logs / "archive.fifo"
    os.mkfifo(fifo)
    for name, path in (("fifo-archive", fifo), ("directory-archive", serial_base)):
        reject(name, linux_command(binary, images=serial_base, initramfs=path), str(path), "not a regular file")
    return result


def panic_control(binary, images, manifest, seed, logs):
    publish = logs / "publish"
    publish.mkdir(parents=True)
    archive = logs / "missing-init.cpio"
    # A valid newc archive with only the trailer: no /init, fallback init, or shell.
    fields = (0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 11, 0)
    archive_data = b"070701" + b"".join(f"{field:08x}".encode("ascii") for field in fields) + b"TRAILER!!!\0\0\0\0"
    archive.write_bytes(archive_data)
    argv = linux_command(binary, images=images, disk=(seed, publish / "absent.raw"), initramfs=archive)
    run = capture_serial(argv + ["--max-fuel", str(MAX_FUEL)], "pipe", logs=logs, termination=None,
                         exit_seconds=BOOT_SECONDS)
    require_exit(run, "fuel", success=False)
    expect_stderr(run["diagnostics_log"], f"Linux guest fuel exhausted (limit {MAX_FUEL} transitions)")
    expect_entries(publish)
    serial = Path(run["serial_log"]).read_bytes().replace(b"\r\n", b"\n")
    panic = KERNEL_PANIC.search(serial)
    linux = b"Linux version " + manifest["kernelrelease"].encode("ascii") + b" "
    if (run["input_bytes"] != 0 or run["fuel_spent"] != MAX_FUEL or run["max_fuel"] != MAX_FUEL
            or not 0 <= serial.find(b"OpenSBI ") < serial.find(linux) or not panic
            or panic.start() < serial.find(linux) or PROMPT in serial or b"Initramfs unpacking failed" in serial):
        raise ValueError(f"missing-init: expected firmware then Linux panic and no shell; see {run['serial_log']}")
    run.update(kernel_panic=panic[0].decode("ascii"), archive_bytes=len(archive_data), archive_sha256=sha256(archive),
               expected_initrd_end=hex(INITRD + len(archive_data)), no_export=True)
    return run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built release Hesper CLI")
    parser.add_argument("--fixture", type=Path, default=CACHE / "initramfs")
    parser.add_argument("--blk-images", type=Path, default=BLK_IMAGE_DIR)
    args = parser.parse_args()
    binary, fixture, blk_images = args.binary.resolve(), args.fixture.resolve(), args.blk_images.resolve()
    if sys.platform != "linux":
        raise ValueError("the native initramfs gate requires Linux PTYs and wait4 RSS units")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"not an executable Hesper binary: {binary}")
    require_tools("mke2fs", "debugfs")
    serial_manifest = verified_manifest()
    blk_manifest = verified_manifest(blk=True, directory=blk_images)
    info = fixture_info(fixture)
    if (int(info["initrd_end"], 16) != INITRD + info["bytes"]
            or any(info["bytes"] == cpio_image(manifest)["bytes"] for manifest in (serial_manifest, blk_manifest))):
        raise ValueError(f"{fixture}: custom fixture end must match its size and differ from the locked end")

    LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="native-", dir=LOGS))
    archive = logs / "guest payload.bin"
    shutil.copyfile(fixture / "initramfs.cpio", archive)
    if archive.stat().st_size != info["bytes"] or sha256(archive) != info["sha256"]:
        raise ValueError(f"{archive}: staged archive differs from the QEMU-accepted fixture")
    manifest_hashes = dict(serial=sha256(IMAGE_DIR / "manifest.json"),
                           blk=sha256(blk_images / "manifest.json"))
    serial_base = stage_base(logs / "serial-base", IMAGE_DIR)
    seed = logs / "seed.raw"
    seed_info = make_seed(seed)
    controls = preflight_controls(binary, serial_base, blk_images, archive, info, seed, logs / "preflight")

    release = serial_manifest["kernelrelease"]
    argv = linux_command(binary, images=serial_base, initramfs=archive)
    custom = {mode: shell_boot(argv, mode, logs / "custom", custom_states(release), info["initrd_end"])
              for mode in ("pipe", "pty")}
    expect_entries(serial_base, *BASE_FILES)
    base_hashes = {name: sha256(serial_base / name) for name in BASE_FILES}
    expected_hashes = {image["path"]: image["sha256"] for image in serial_manifest["images"]
                       if image["path"] in BASE_FILES}
    expected_hashes["manifest.json"] = manifest_hashes["serial"]
    if base_hashes != expected_hashes:
        raise ValueError(f"{serial_base}: custom boot changed the locked base files")
    default = shell_boot(linux_command(binary), "pipe", logs / "default", default_states(release),
                         cpio_image(serial_manifest)["end"])
    disk = disk_round_trip(binary, blk_images, blk_manifest, archive, info["initrd_end"], seed,
                           seed_info["sha256"], logs / "disk")
    panic = panic_control(binary, blk_images, blk_manifest, seed, logs / "missing-init")
    seed_info["after_sha256"] = sha256(seed)
    if seed_info["after_sha256"] != seed_info["sha256"]:
        raise ValueError(f"{seed}: original seed changed")
    if sha256(archive) != info["sha256"]:
        raise ValueError(f"{archive}: replacement archive changed during acceptance")

    report = dict(binary=str(binary), binary_sha256=sha256(binary), python=sys.version,
                  host_kernel=os.uname().release, host_arch=os.uname().machine,
                  manifests=dict(serial=serial_manifest, blk=blk_manifest),
                  manifest_sha256=manifest_hashes, serial_base_sha256=base_hashes,
                  fixture=info, fixture_directory=str(fixture), staged_archive=str(archive), seed=seed_info,
                  boot_deadline_seconds=BOOT_SECONDS, deadline_per_state_seconds=STEP_SECONDS,
                  byte_budget_per_state=STEP_BYTES, exit_deadline_seconds=EXIT_SECONDS,
                  preflight_controls=controls, custom=custom, default=default, disk=disk, missing_init=panic)
    path = logs / "report.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Native custom/default cold boots, disk round trip, and missing-init panic passed: {path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
