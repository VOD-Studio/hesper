"""Prove the QEMU-tested replacement root in the production Chromium workbench."""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from browser import BOOT_SECONDS, SETUP_SECONDS, browser_session, production_dist
from build_guest import BLK_IMAGE_DIR, BLK_IMAGE_LOCK, ENV, IMAGE_DIR, IMAGE_LOCK, PROFILE, verified_manifest
from disk_acceptance import check_proof, disk_states, make_seed, proof
from initramfs_acceptance import custom_states, default_states, fixture_info, initrd_end_state
from serial_acceptance import STEP_BYTES, STEP_SECONDS, session
from sources import CACHE, require_tools, sha256

DEFAULT_ARCHIVE = "initramfs.cpio"
DISK_MARKER = "BROWSER_DATA_OK"
OVERSIZED_BYTES = 16 * 1024 * 1024 + 1


def run(dist, fixture, serial_manifest, blk_manifest, blk_images, logs):
    archive = fixture / DEFAULT_ARCHIVE
    info = fixture_info(fixture)
    disk = logs / "initramfs-data.raw"
    seed = make_seed(disk)
    proof_path = logs / "proof.sh"
    proof_path.write_bytes(proof(DISK_MARKER))
    subprocess.run(["debugfs", "-w", "-R", f"write {proof_path.name} /proof.sh", disk],
                   cwd=logs, env=ENV, check=True, timeout=10)
    disk_hash = check_proof(disk, DISK_MARKER)

    oversized = logs / "oversized.cpio"
    with oversized.open("xb") as stream:
        stream.truncate(OVERSIZED_BYTES)
    ignored = logs / DEFAULT_ARCHIVE
    ignored.write_bytes(b"not a newc archive\n")
    damaged_board = logs / "board.dtb"
    board = bytearray((IMAGE_DIR / damaged_board.name).read_bytes())
    board[-1] ^= 1
    damaged_board.write_bytes(board)

    pinned_archive = next(pin for pin in serial_manifest["images"] if pin["path"] == DEFAULT_ARCHIVE)
    default_end = pinned_archive["end"]

    if int(default_end, 16) == int(info["initrd_end"], 16):
        raise ValueError("reference fixture must exercise a different initrd end from the default")

    def local_bytes(manifest, archive_bytes):
        return sum(pin["bytes"] for pin in manifest["images"] if pin["path"] != DEFAULT_ARCHIVE) + archive_bytes

    read_disk = disk_states(False, DISK_MARKER, busybox=True)
    phases = (
        ("default-after-cancel", serial_manifest, local_bytes(serial_manifest, pinned_archive["bytes"]),
         default_end, default_states(serial_manifest["kernelrelease"])),
        ("custom-serial", serial_manifest, local_bytes(serial_manifest, 0),
         info["initrd_end"], custom_states(serial_manifest["kernelrelease"])),
        ("custom-blk", blk_manifest, local_bytes(blk_manifest, ignored.stat().st_size),
         info["initrd_end"], custom_states(blk_manifest["kernelrelease"]) + read_disk),
    )
    argv = ["bun", PROFILE / "browser_session.mjs", dist, blk_images, logs, "initramfs",
            IMAGE_DIR, archive, disk, oversized, damaged_board, ignored]
    facts = {}
    count = sum(len(states) + 2 for _, _, _, _, states in phases)
    with browser_session(argv, logs, count) as (event, receive, send, keys, complete, serial):
        for index, (phase, manifest, expected_bytes, expected_end, states) in enumerate(phases):
            if index:
                send(dict(type=phase))
            started = event(time.monotonic() + SETUP_SECONDS)
            if (started["type"], started["phase"], started["imageBytes"]) != ("start", phase, expected_bytes):
                raise ValueError(f"unexpected browser image selection for {phase}: {started}")
            offsets = [len(serial)]
            observed_end = []

            end = initrd_end_state(expected_end, observed_end.append)
            timings = session(receive, keys, (*states, end), BOOT_SECONDS)
            offsets.append(len(serial))
            if index == 0 and b"ROOT_OK:" in serial[offsets[0]:offsets[1]]:
                raise ValueError("custom proof leaked into the default restart")
            facts[phase] = dict(profile="blk" if phase == "custom-blk" else "serial",
                                kernelrelease=manifest["kernelrelease"], start=started, timings=timings,
                                expected_initrd_end=expected_end, observed_initrd_end=observed_end[0],
                                serial_offsets=offsets)
        send(dict(type="finish"))
        result = complete()

    if sha256(archive) != info["sha256"] or sha256(disk) != disk_hash:
        raise ValueError("browser acceptance modified the replacement archive or input disk")
    return dict(initramfs=info, fixture=str(archive), browser=started["browser"],
                base_lock_sha256=dict(serial=sha256(IMAGE_LOCK), blk=sha256(BLK_IMAGE_LOCK)),
                manifests=dict(serial=serial_manifest, blk=blk_manifest),
                data_disk=dict(seed=seed, input_sha256=disk_hash, proof=f"{DISK_MARKER}:42"),
                phases=facts, observations=result, boot_deadline_seconds=BOOT_SECONDS,
                command_deadline_seconds=STEP_SECONDS, command_byte_limit=STEP_BYTES, logs=str(logs))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, default=CACHE / "initramfs")
    parser.add_argument("--blk-images", type=Path, default=BLK_IMAGE_DIR)
    args = parser.parse_args()
    require_tools("mke2fs", "debugfs")
    dist = production_dist()
    fixture = args.fixture.resolve(strict=True)
    blk_images = args.blk_images.resolve(strict=True)
    base = CACHE / "initramfs"
    base.mkdir(parents=True, exist_ok=True)
    report = base / "hesper-initramfs-browser.json"
    report.unlink(missing_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="browser-", dir=base))
    result = run(dist, fixture, verified_manifest(), verified_manifest(blk=True, directory=blk_images), blk_images, logs)
    report.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Production Chromium booted the reference initramfs with and without a data disk: {report}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
