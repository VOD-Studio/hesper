"""Verify Alpine system disk exports across independent release CLI processes."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from alpine_acceptance import checked
from alpine_system_acceptance import CONFIG, POWER_OFF, ROOT_MOUNT, cold_states, install_states
from alpine_system_disk import SEED, SIZE, check_filesystem
from build_guest import ALPINE_ROOT_IMAGE_DIR, ALPINE_ROOT_LOGS, ENV, verified_manifest
from native import (ALPINE_BOOT_SECONDS, ALPINE_MAX_FUEL, ALPINE_STEP_SECONDS, EXIT_SECONDS,
                    capture_serial, linux_command, require_exit)
from native_disk import OCCUPANT, expect_entries, expect_stderr, require_boot_fuel
from serial_acceptance import session
from sources import require_tools, sha256

WRITE = checked("discarded-write", b"echo discarded > " + CONFIG)


def boot(binary, logs, transport, source, target, *, states=(ROOT_MOUNT,), final=POWER_OFF,
         images=ALPINE_ROOT_IMAGE_DIR, termination=None, max_fuel=ALPINE_MAX_FUEL, before_exit=None):
    argv = linux_command(binary, images=images, disk=(source, target), max_fuel=max_fuel)
    commands = tuple(command for _, command, _ in states) + ((final,) if final else ())

    def exchange(receive, send):
        timings = session(receive, send, states, ALPINE_BOOT_SECONDS, step_seconds=ALPINE_STEP_SECONDS,
                          allow_cursor_query=True)
        if before_exit:
            before_exit()
        if final:
            send(final)
        return timings

    return capture_serial(argv, transport, logs=logs, commands=commands, exchange=exchange if states else None,
                          termination=termination, exit_seconds=ALPINE_STEP_SECONDS if states else ALPINE_BOOT_SECONDS)


def check_export(run, path, log):
    require_exit(run, "poweroff")
    require_boot_fuel(run)
    if path.stat().st_size != SIZE:
        raise ValueError(f"{path}: expected a {SIZE}-byte raw system disk")
    check_filesystem(path, log, ENV)
    return sha256(path)


def persistence(binary, logs, seed, manifest):
    logs.mkdir()
    first, second = logs / "first.raw", logs / "second.raw"
    write = boot(binary, logs / "install", "pty", seed, first, states=install_states())
    first_hash = check_export(write, first, logs / "first-e2fsck.log")
    if first_hash == manifest["seed"]["sha256"]:
        raise ValueError("install boot produced an unchanged disk")

    # The cold process has the four locked boot images and previously exported disk.
    cold_images = logs / "cold-images"
    cold_images.mkdir()
    for name in ("manifest.json", *(artifact["path"] for artifact in manifest["images"])):
        (cold_images / name).symlink_to(ALPINE_ROOT_IMAGE_DIR / name)
    read = boot(binary, logs / "cold", "pty", first, second, states=cold_states(), images=cold_images)
    second_hash = check_export(read, second, logs / "second-e2fsck.log")
    if sha256(first) != first_hash:
        raise ValueError("cold boot modified its input disk")
    return dict(install=write, cold=read, first_sha256=first_hash, second_sha256=second_hash)


def preflight(binary, logs, seed):
    logs.mkdir()
    output = logs / "output.raw"
    output.write_bytes(OCCUPANT)
    base = linux_command(binary, images=ALPINE_ROOT_IMAGE_DIR)
    images = logs / "corrupt-seed-images"
    images.mkdir()
    (images / "manifest.json").symlink_to(ALPINE_ROOT_IMAGE_DIR / "manifest.json")
    corrupt = images / SEED
    corrupt.write_bytes(bytes(SIZE))
    other = logs / "other.raw"
    cases = (
        ("missing-pair", base, "images-alpine-lts-root requires --disk-in and --disk-out"),
        ("existing-output", base + ["--disk-in", str(seed), "--disk-out", str(output)], "output already exists"),
        ("corrupt-locked-seed", linux_command(binary, images=images, disk=(corrupt, other)), "SHA-256 mismatch"),
    )
    result = {}
    for name, argv, message in cases:
        run = subprocess.run(argv, env=ENV, stdin=subprocess.DEVNULL, capture_output=True, timeout=EXIT_SECONDS)
        stderr = run.stderr.decode(errors="replace")
        if run.returncode <= 0 or run.stdout or message not in stderr:
            raise ValueError(f"{name}: expected preflight rejection containing {message!r}, got {stderr!r}")
        result[name] = dict(exit_status=run.returncode, stderr=stderr)
    expect_entries(logs, output.name, images.name)
    if output.read_bytes() != OCCUPANT or corrupt.read_bytes() != bytes(SIZE):
        raise ValueError("preflight modified a host file")
    return result


def control(binary, logs, seed, name):
    publish = logs / "publish"
    publish.mkdir(parents=True)
    target = publish / "output.raw"
    if name == "pipe":
        run = boot(binary, logs, "pipe", seed, target, termination="eof")
        run["sha256"] = check_export(run, target, logs / "e2fsck.log")
        expect_entries(publish, target.name)
    elif name == "quit":
        run = boot(binary, logs, "pty", seed, target, states=(ROOT_MOUNT, WRITE), final=None, termination="quit")
        require_exit(run, "quit")
        expect_entries(publish)
    elif name == "fuel":
        run = boot(binary, logs, "pipe", seed, target, states=(), final=None, max_fuel=1)
        require_exit(run, "fuel", success=False)
        expect_entries(publish)
    elif name == "invalid-root":
        invalid = logs / "invalid.raw"
        with invalid.open("xb") as stream:
            stream.truncate(SIZE)
        original = sha256(invalid)
        run = boot(binary, logs, "pipe", invalid, target, states=(), final=None)
        require_exit(run, "guest-reboot", success=False)
        if b"Hesper Alpine root: cannot mount /dev/vda as ext4" not in Path(run["serial_log"]).read_bytes():
            raise ValueError(f"missing mount failure; see {run['serial_log']}")
        if sha256(invalid) != original:
            raise ValueError(f"{invalid}: input changed after mount failure")
        expect_entries(publish)
    elif name == "panic":
        run = boot(binary, logs, "pipe", seed, target, states=(ROOT_MOUNT, WRITE),
                   final=b"echo c > /proc/sysrq-trigger\n")
        require_exit(run, "guest-reboot", success=False)
        if b"Kernel panic" not in Path(run["serial_log"]).read_bytes():
            raise ValueError(f"missing panic; see {run['serial_log']}")
        expect_entries(publish)
    elif name == "forced-poweroff":
        run = boot(binary, logs, "pipe", seed, target, states=(ROOT_MOUNT, WRITE), final=b"poweroff -f\n")
        require_exit(run, "export-error", success=False)
        expect_stderr(run["diagnostics_log"], "unclean ext4 system disk")
        expect_entries(publish)
    elif name == "publish-race":
        def occupy():
            with target.open("xb") as stream:
                stream.write(OCCUPANT)

        run = boot(binary, logs, "pty", seed, target, states=(ROOT_MOUNT, WRITE), before_exit=occupy)
        require_exit(run, "export-error", success=False)
        expect_stderr(run["diagnostics_log"], "saving system disk", "File exists")
        expect_entries(publish, target.name)
        if target.read_bytes() != OCCUPANT:
            raise ValueError(f"{target}: late occupant was overwritten")
    else:
        raise ValueError(f"unknown system disk control: {name}")
    return run


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built release Hesper CLI")
    binary = parser.parse_args().binary.resolve()
    require_tools("e2fsck")
    manifest = verified_manifest(blk=True, alpine=True, root_disk=True)
    seed = ALPINE_ROOT_IMAGE_DIR / SEED
    ALPINE_ROOT_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="native-", dir=ALPINE_ROOT_LOGS))
    print(f"Alpine system disk logs: {logs}", flush=True)
    preflight_results = preflight(binary, logs / "preflight", seed)
    names = ("pipe", "quit", "fuel", "invalid-root", "panic", "forced-poweroff", "publish-race")
    with ThreadPoolExecutor(max_workers=4) as workers:
        trip = workers.submit(persistence, binary, logs / "persistence", seed, manifest)
        futures = {name: workers.submit(control, binary, logs / name, seed, name) for name in names}
        round_trip = trip.result()
        controls = {name: result.result() for name, result in futures.items()}
    after = sha256(seed)
    if after != manifest["seed"]["sha256"]:
        raise ValueError(f"{seed}: original seed changed")
    report = dict(binary=str(binary), binary_sha256=sha256(binary), manifest=manifest, seed_after_sha256=after,
                  round_trip=round_trip, preflight=preflight_results, controls=controls)
    path = logs / "report.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Hesper verified Alpine system disk exports and cold persistence: {path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
