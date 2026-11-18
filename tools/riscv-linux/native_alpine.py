"""Prove the locked Alpine guest over native pipes and a real Linux PTY."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from alpine_acceptance import cold_states, kernel_states, states
from build_guest import (ALPINE_IMAGE_DIR, ALPINE_LEGACY_DIR, ALPINE_LOGS, BLK_IMAGE_DIR, ENV, IMAGE_DIR,
                         IMAGE_NAMES, verified_manifest)
from disk_acceptance import check_proof, disk_states, make_seed
from native import ACCEPTANCE_MAX_FUEL, BOOT_SECONDS, EXIT_SECONDS, capture_serial, linux_command, require_exit, run_serial
from native_disk import expect_entries, require_boot_fuel
from serial_acceptance import STEP_BYTES, STEP_SECONDS, session
from sources import require_tools, sha256

ALPINE_MAX_FUEL = 2_000_000_000
ALPINE_BOOT_SECONDS = 180
ALPINE_STEP_SECONDS = 60
MANIFEST_MISMATCH = "manifest differs from the embedded image lock"
DISK_MARKER = "ALPINE_BLK_CONTROL_OK"


def boot_session(argv, transport, logs, protocol, *, boot_seconds, step_seconds, alpine=False):
    def exchange(receive, send):
        return session(receive, send, protocol, boot_seconds, step_seconds=step_seconds, allow_cursor_query=alpine)

    run = capture_serial(argv, transport, logs=logs, commands=tuple(command for _, command, _ in protocol),
                         exchange=exchange)
    require_exit(run, "quit")
    require_boot_fuel(run)
    return run


def reject(name, argv, logs, *expected):
    serial_log = logs / f"{name}-serial.raw"
    diagnostics_log = logs / f"{name}-stderr.log"
    started = time.monotonic()
    try:
        process = subprocess.run(argv, env=ENV, stdin=subprocess.DEVNULL, capture_output=True, timeout=EXIT_SECONDS)
    except subprocess.TimeoutExpired as error:
        serial_log.write_bytes(error.stdout or b"")
        diagnostics_log.write_bytes(error.stderr or b"")
        raise ValueError(f"{name}: rejection exceeded deadline; see {serial_log} and {diagnostics_log}") from error
    elapsed = time.monotonic() - started
    serial_log.write_bytes(process.stdout)
    diagnostics_log.write_bytes(process.stderr)
    stderr = process.stderr.decode(errors="replace").strip()
    if (process.returncode <= 0 or process.stdout or "linux: exit=" in stderr
            or not all(part in stderr for part in expected)):
        raise ValueError(f"{name}: expected {expected!r} before guest execution; status={process.returncode}, "
                         f"serial bytes={len(process.stdout)}; see {serial_log} and {diagnostics_log}")
    return dict(command=argv, exit_status=process.returncode, exit_reason="preflight-rejection",
                output_bytes=len(process.stdout), runtime_seconds=elapsed, stderr=stderr,
                serial_log=str(serial_log), diagnostics_log=str(diagnostics_log))


def negative_controls(binary, directories, manifests, seed, logs):
    alpine_images = directories["alpine"]
    logs.mkdir()
    corrupt = logs / "corrupt-dtb"
    mixed = logs / "mixed-manifest"
    corrupt.mkdir()
    mixed.mkdir()
    for name in ("manifest.json", *IMAGE_NAMES):
        if name != "board.dtb":
            (corrupt / name).symlink_to(alpine_images / name)
        if name != "manifest.json":
            source = IMAGE_DIR if name == "Image" else alpine_images
            (mixed / name).symlink_to(source / name)

    dtb = bytearray((alpine_images / "board.dtb").read_bytes())
    dtb[-1] ^= 1
    (corrupt / "board.dtb").write_bytes(dtb)

    # Every staged image matches the description; only comparison against the
    # embedded lock can distinguish this counterfeit manifest from a real one.
    forged = dict(manifests["alpine"], images=list(manifests["alpine"]["images"]))
    kernel = IMAGE_NAMES.index("Image")
    forged["images"][kernel] = manifests["serial"]["images"][kernel]
    (mixed / "manifest.json").write_text(json.dumps(forged, indent=2) + "\n")

    old_root = logs / "old-root"
    corrupt_kernel = logs / "corrupt-kernel"
    old_root.mkdir()
    corrupt_kernel.mkdir()
    for name in ("manifest.json", *IMAGE_NAMES):
        source = directories["legacy"] if name == "initramfs.cpio" else alpine_images
        (old_root / name).symlink_to(source / name)
        if name != "Image":
            (corrupt_kernel / name).symlink_to(alpine_images / name)
    kernel_bytes = bytearray((alpine_images / "Image").read_bytes())
    kernel_bytes[-1] ^= 1
    (corrupt_kernel / "Image").write_bytes(kernel_bytes)

    publish = logs / "publish"
    publish.mkdir()
    archive = IMAGE_DIR / "initramfs.cpio"
    result = {
        "old_root": reject("old-root", linux_command(binary, images=old_root), logs,
                           str(old_root / "initramfs.cpio"), "size"),
        "corrupt_kernel": reject("corrupt-kernel", linux_command(binary, images=corrupt_kernel), logs,
                                 str(corrupt_kernel / "Image"), "SHA-256 mismatch"),
        "corrupt_dtb": reject("corrupt-dtb", linux_command(binary, images=corrupt), logs,
                              str(corrupt / "board.dtb"), "SHA-256 mismatch"),
        "forged_mixed_manifest": reject("mixed-manifest", linux_command(binary, images=mixed), logs,
                                        str(mixed / "manifest.json"), MANIFEST_MISMATCH),
        "custom_root": reject("custom-root", linux_command(binary, images=alpine_images, initramfs=archive), logs,
                              "Alpine images do not support a custom initramfs"),
        "disk": reject("disk", linux_command(binary, images=alpine_images, disk=(seed, publish / "output.raw")),
                       logs, str(alpine_images / "manifest.json"), MANIFEST_MISMATCH),
    }
    expect_entries(publish)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built release Hesper CLI")
    parser.add_argument("--blk-images", type=Path, default=BLK_IMAGE_DIR, help="directory with original blk images")
    args = parser.parse_args()
    started = time.monotonic()
    binary = args.binary.resolve()
    if sys.platform != "linux":
        raise ValueError("the native Alpine gate requires Linux PTYs and wait4 RSS units")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"not an executable Hesper binary: {binary}")
    require_tools("mke2fs", "debugfs")
    directories = dict(alpine=ALPINE_IMAGE_DIR.resolve(), legacy=ALPINE_LEGACY_DIR.resolve(),
                       serial=IMAGE_DIR.resolve(), blk=args.blk_images.resolve())
    manifests = dict(alpine=verified_manifest(alpine=True, directory=directories["alpine"]),
                     legacy=verified_manifest(alpine=True, legacy=True, directory=directories["legacy"]),
                     serial=verified_manifest(directory=directories["serial"]),
                     blk=verified_manifest(blk=True, directory=directories["blk"]))
    image_hashes = {profile: {image["path"]: image["sha256"] for image in manifest["images"]}
                    for profile, manifest in manifests.items()}
    manifest_hashes = {profile: sha256(directory / "manifest.json") for profile, directory in directories.items()}

    ALPINE_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="native-", dir=ALPINE_LOGS))
    seed = logs / "seed.raw"
    seed_info = make_seed(seed)
    negative = negative_controls(binary, directories, manifests, seed, logs / "preflight")

    alpine = linux_command(binary, images=directories["alpine"], max_fuel=ALPINE_MAX_FUEL)
    protocol = states(manifests["alpine"])
    transports = {mode: boot_session(alpine, mode, logs / "install", protocol,
                                     boot_seconds=ALPINE_BOOT_SECONDS, step_seconds=ALPINE_STEP_SECONDS, alpine=True)
                  for mode in ("pipe", "pty")}
    cold = boot_session(alpine, "pipe", logs / "cold", cold_states(manifests["alpine"]),
                        boot_seconds=ALPINE_BOOT_SECONDS, step_seconds=ALPINE_STEP_SECONDS, alpine=True)

    legacy = boot_session(linux_command(binary, images=directories["legacy"]), "pipe", logs / "legacy",
                          tuple(kernel_states(manifests["legacy"])), boot_seconds=ALPINE_BOOT_SECONDS,
                          step_seconds=ALPINE_STEP_SECONDS, alpine=True)
    serial = run_serial(binary, manifests["serial"], "pipe", logs=logs / "serial")
    require_boot_fuel(serial)
    disk_output = logs / "blk-output.raw"
    blk = boot_session(linux_command(binary, images=directories["blk"], disk=(seed, disk_output)),
                       "pipe", logs / "blk", disk_states(True, DISK_MARKER),
                       boot_seconds=BOOT_SECONDS, step_seconds=STEP_SECONDS)
    blk["output_sha256"] = check_proof(disk_output, DISK_MARKER)
    if disk_output.stat().st_size != seed.stat().st_size or sha256(seed) != seed_info["sha256"]:
        raise ValueError(f"{seed}: blk control modified its input or exported the wrong size")
    for profile, directory in directories.items():
        expected = {**image_hashes[profile], "manifest.json": manifest_hashes[profile]}
        for name, digest in expected.items():
            if sha256(directory / name) != digest:
                raise ValueError(f"{directory / name}: acceptance changed a locked boot input")

    report = dict(binary=str(binary), binary_sha256=sha256(binary), python=sys.version,
                  host_kernel=os.uname().release, host_arch=os.uname().machine,
                  image_directories={profile: str(directory) for profile, directory in directories.items()},
                  image_sha256=image_hashes, manifest_sha256=manifest_hashes, manifests=manifests,
                  boot_deadline_seconds=ALPINE_BOOT_SECONDS, deadline_per_state_seconds=ALPINE_STEP_SECONDS,
                  exit_deadline_seconds=EXIT_SECONDS, byte_budget_per_state=STEP_BYTES, max_fuel=ALPINE_MAX_FUEL,
                  legacy_limits=dict(boot_deadline_seconds=BOOT_SECONDS, deadline_per_state_seconds=STEP_SECONDS,
                                     max_fuel=ACCEPTANCE_MAX_FUEL),
                  transports=transports, cold=cold, legacy_controls=dict(alpine=legacy, serial=serial, blk=blk, seed=seed_info),
                  negative_controls=negative,
                  runtime_seconds=time.monotonic() - started)
    path = logs / "report.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Native Alpine pipe, PTY and cold boots, legacy profiles and image rejection passed: {path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
