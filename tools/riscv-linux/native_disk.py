"""Prove native Linux data-disk round trips over both pipes and a real PTY."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from build_guest import BLK_IMAGE_DIR, BLK_LOGS, ENV, IMAGE_LOCK, verified_manifest
from disk_acceptance import SECTORS, check_proof, commands, drive_disk, make_seed, proof
from native import BOOT_SECONDS, EXIT_SECONDS, capture_serial, linux_command, require_exit, run_serial
from serial_acceptance import STEP_BYTES, STEP_SECONDS
from sources import require_tools, sha256

MARKER = "DISK_OK"
DISK_BYTES = SECTORS * 512
INVALID_SIZE = "positive multiple of 512"
OCCUPANT = b"existing host file, not a disk export\n"


def expect_entries(directory, *names):
    actual = sorted(entry.name for entry in directory.iterdir())
    if actual != sorted(names):
        raise ValueError(f"{directory}: expected entries {sorted(names)}, found {actual}")


def expect_stderr(log, *parts):
    stderr = Path(log).read_text(errors="replace")
    if not all(part in stderr for part in parts):
        raise ValueError(f"expected diagnostics containing {parts!r}; see {log}")
    return stderr


def require_boot_fuel(run):
    if not 0 < run["exit_retired"] <= run["fuel_spent"] < run["max_fuel"]:
        raise ValueError(f"boot did not finish inside its fuel budget; see {run['diagnostics_log']}")


def round_trip(binary, manifest, transport, directory, seed, seed_hash):
    directory.mkdir()
    source = directory / "input.raw"
    first, second = directory / "first.raw", directory / "second.raw"
    shutil.copyfile(seed, source)
    if sha256(source) != seed_hash:
        raise ValueError(f"{source}: seed copy mismatch")

    write = run_serial(binary, manifest, transport, disk=(source, first), write=True,
                       marker=MARKER, logs=directory / "write")
    require_boot_fuel(write)
    source_after = sha256(source)
    if source_after != seed_hash or first.stat().st_size != DISK_BYTES:
        raise ValueError(f"{directory}: write changed input or exported the wrong disk size")
    after_write = check_proof(first, MARKER)
    if after_write == seed_hash:
        raise ValueError(f"{first}: write left the disk unchanged")

    read = run_serial(binary, manifest, transport, disk=(first, second), marker=MARKER,
                      logs=directory / "read")
    require_boot_fuel(read)
    first_after_read = sha256(first)
    if first_after_read != after_write or second.stat().st_size != DISK_BYTES:
        raise ValueError(f"{directory}: read changed input or exported the wrong disk size")
    after_read = check_proof(second, MARKER)
    if after_read != after_write:
        raise ValueError(f"{second}: read-only round trip changed raw disk bytes")
    return dict(input_sha256=seed_hash, input_after_write_sha256=source_after,
                first_sha256=after_write, first_after_read_sha256=first_after_read,
                second_sha256=after_read, write=write, read=read)


def preflight_controls(binary, manifest, seed, directory):
    directory.mkdir()
    publish = directory / "publish"
    publish.mkdir()
    target = publish / "absent.raw"
    result = {}

    def reject(name, argv, *parts):
        error_log = directory / f"{name}-stderr.log"
        serial_log = directory / f"{name}-serial.raw"
        started = time.monotonic()
        try:
            process = subprocess.run(argv, env=ENV, stdin=subprocess.DEVNULL, capture_output=True,
                                     timeout=EXIT_SECONDS)
        except subprocess.TimeoutExpired as error:
            error_log.write_bytes(error.stderr or b"")
            serial_log.write_bytes(error.stdout or b"")
            raise ValueError(f"{name}: preflight exceeded its deadline; see {error_log} and {serial_log}") from error
        runtime = time.monotonic() - started
        error_log.write_bytes(process.stderr)
        serial_log.write_bytes(process.stdout)
        if process.returncode <= 0 or process.stdout or b"linux: exit=" in process.stderr:
            raise ValueError(f"{name}: expected nonzero preflight rejection before guest execution; "
                             f"status={process.returncode}; see {error_log} and {serial_log}")
        stderr = expect_stderr(error_log, *parts)
        result[name] = dict(command=argv, exit_status=process.returncode, stderr=stderr.strip(),
                            runtime_seconds=runtime, diagnostics_log=str(error_log), serial_log=str(serial_log))

    def args(source=seed, output=target, *, images=BLK_IMAGE_DIR):
        return linux_command(binary, images=images, disk=(source, output))

    base = linux_command(binary, images=BLK_IMAGE_DIR)
    required_pair = "--disk-in and --disk-out must be used together"
    reject("missing-disk-out", base + ["--disk-in", str(seed)], required_pair)
    reject("missing-disk-in", base + ["--disk-out", str(target)], required_pair)

    # The serial lock is checked in; no built serial images are needed to prove that
    # a disk session rejects its manifest before looking for any firmware bytes.
    serial = directory / "serial-profile"
    serial.mkdir()
    shutil.copyfile(IMAGE_LOCK, serial / "manifest.json")
    mismatch = "manifest differs from the embedded image lock"
    reject("serial-profile-with-disk", args(images=serial), str(serial / "manifest.json"), mismatch)
    reject("blk-profile-without-disk", base, str(BLK_IMAGE_DIR / "manifest.json"), mismatch)

    corrupt = directory / "corrupt-firmware"
    corrupt.mkdir()
    firmware = corrupt / "fw_jump.bin"
    shutil.copyfile(BLK_IMAGE_DIR / firmware.name, firmware)
    with firmware.open("r+b") as stream:
        stream.seek(-1, os.SEEK_END)
        last = stream.read(1)[0]
        stream.seek(-1, os.SEEK_END)
        stream.write(bytes([last ^ 1]))
    for filename in ("manifest.json", *(image["path"] for image in manifest["images"])):
        if filename != firmware.name:
            os.link(BLK_IMAGE_DIR / filename, corrupt / filename)
    reject("one-byte-bad-firmware", args(images=corrupt), str(firmware), "SHA-256 mismatch")

    for name, size in (("zero-size", 0), ("unaligned-size", 511), ("oversized", 16 * 1024 * 1024 + 512)):
        invalid = directory / f"{name}.raw"
        with invalid.open("xb") as stream:
            stream.truncate(size)
        reject(name, args(invalid), str(invalid), INVALID_SIZE)
    fifo = directory / "fifo.raw"
    os.mkfifo(fifo)
    reject("fifo-input", args(fifo), str(fifo), "not a regular file")
    reject("directory-input", args(corrupt), str(corrupt), "not a regular file")
    expect_entries(publish)

    existing = publish / "existing.raw"
    existing.write_bytes(OCCUPANT)
    alias = publish / "symlink.raw"
    alias.symlink_to(seed)
    dangling = publish / "dangling.raw"
    dangling.symlink_to(target)
    hardlink = publish / "hardlink.raw"
    os.link(seed, hardlink)
    for name, output in (("existing-output", existing), ("symlink-output", alias),
                         ("dangling-output", dangling), ("hardlink-output", hardlink), ("same-path", seed)):
        reject(name, args(output=output), str(output), "already exists")
    if (existing.read_bytes() != OCCUPANT or not alias.is_symlink() or alias.readlink() != seed
            or not dangling.is_symlink() or dangling.readlink() != target or hardlink.is_symlink()
            or not hardlink.samefile(seed)):
        raise ValueError(f"{publish}: preflight rejection changed an existing output")
    expect_entries(publish, existing.name, alias.name, dangling.name, hardlink.name)
    return result


def session_controls(binary, seed, directory):
    directory.mkdir()
    result = {}
    write_commands = tuple(command for command, _ in commands(True, MARKER))

    for name in ("fuel", "eof", "publish-race", "missing-directory-at-export"):
        logs = directory / name
        publish = logs / "publish"
        publish.mkdir(parents=True)
        target = publish / "output.raw"
        if name == "fuel":
            argv = linux_command(binary, images=BLK_IMAGE_DIR, disk=(seed, target), max_fuel=1)
            run = capture_serial(argv, "pipe", logs=logs, termination=None)
            require_exit(run, "fuel", success=False)
            if run["fuel_spent"] != 1 or run["max_fuel"] != 1:
                raise ValueError(f"fuel: expected exactly one transition; see {run['diagnostics_log']}")
            expect_stderr(run["diagnostics_log"], "Linux guest fuel exhausted (limit 1 transitions)")
            expect_entries(publish)
        else:
            argv = linux_command(binary, images=BLK_IMAGE_DIR, disk=(seed, target))
            moved = logs / "moved"

            def exchange(receive, send):
                timings = drive_disk(receive, send, True, MARKER, boot_seconds=BOOT_SECONDS)
                # Only change host state after the accepted write and full shell prompt.
                expect_entries(publish)
                if name == "publish-race":
                    with target.open("xb") as stream:
                        stream.write(OCCUPANT)
                elif name == "missing-directory-at-export":
                    publish.rename(moved)
                return timings

            run = capture_serial(argv, "pipe", logs=logs, commands=write_commands, exchange=exchange,
                                 termination="eof" if name == "eof" else "quit")
            require_boot_fuel(run)
            if name == "eof":
                require_exit(run, "eof")
                expect_entries(publish)
            else:
                require_exit(run, "export-error", success=False)
                expect_stderr(run["diagnostics_log"], "saving data disk", str(target),
                              "File exists" if name == "publish-race" else "No such file or directory")
                if name == "publish-race":
                    expect_entries(publish, target.name)
                    if target.is_symlink() or target.read_bytes() != OCCUPANT:
                        raise ValueError(f"{target}: late output occupant was overwritten")
                    run["occupant_sha256"] = sha256(target)
                else:
                    if os.path.lexists(publish):
                        raise ValueError(f"{publish}: export recreated the missing destination directory")
                    expect_entries(moved)
        result[name] = run
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built release Hesper CLI")
    binary = parser.parse_args().binary.resolve()
    if sys.platform != "linux":
        raise ValueError("the native disk gate requires Linux PTYs and wait4 RSS units")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"not an executable Hesper binary: {binary}")
    require_tools("mke2fs", "debugfs")
    manifest = verified_manifest(blk=True)
    BLK_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="native-", dir=BLK_LOGS))
    seed = logs / "seed.raw"
    info = make_seed(seed)

    preflight = preflight_controls(binary, manifest, seed, logs / "preflight")
    transports = {mode: round_trip(binary, manifest, mode, logs / mode, seed, info["sha256"])
                  for mode in ("pipe", "pty")}
    controls = session_controls(binary, seed, logs / "session-controls")
    info["after_sha256"] = sha256(seed)
    if info["after_sha256"] != info["sha256"]:
        raise ValueError(f"{seed}: original seed changed")

    protocol = {name: [{"command": command.decode("ascii"),
                        "expected_result_lines": [line.decode("ascii") for line in lines]}
                       for command, lines in commands(write, MARKER)]
                for name, write in (("write", True), ("read", False))}
    report = dict(binary=str(binary), binary_sha256=sha256(binary), host_kernel=os.uname().release,
                  host_arch=os.uname().machine, manifest=manifest, seed=info, protocol=protocol,
                  expected_proof=proof(MARKER).decode("ascii"), boot_deadline_seconds=BOOT_SECONDS,
                  deadline_per_state_seconds=STEP_SECONDS, byte_budget_per_state=STEP_BYTES,
                  exit_deadline_seconds=EXIT_SECONDS, transports=transports,
                  preflight_controls=preflight, session_controls=controls)
    path = logs / "report.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Hesper disk writes, cold readbacks, and no-save controls passed: {path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
