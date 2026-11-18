"""Cold boot the pinned Linux images in the built Hesper CLI over pipes and a PTY."""

import argparse
from contextlib import ExitStack
import errno
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from build_guest import DRAM, DRAM_END, ENV, IMAGE_DIR, LOGS, verified_manifest
from serial_acceptance import COMMANDS, STEP_BYTES, STEP_SECONDS, drive

QUIT = b"\x1d"
INPUT_BUDGET = sum(map(len, COMMANDS)) + len(QUIT)
BOOT_SECONDS = 90
EXIT_SECONDS = 5
SUMMARY = re.compile(r"^linux: exit=(\S+) fuel=(\d+)/(\d+) retired=(\d+) time=(\d+)(?:\s|$)", re.M)


def reap(guest, options):
    # Reap this CLI only, rather than measuring the high-water RSS of all children.
    pid, status, usage = os.wait4(guest.pid, options)
    if pid:
        guest.returncode = os.waitstatus_to_exitcode(status)
        return usage.ru_maxrss  # KiB on Linux
    return None


def run_serial(binary, manifest, transport):
    argv = [str(binary), "linux", "--images", str(IMAGE_DIR), "--diagnostics"]
    prefix = f"hesper-{transport}"
    output_path = LOGS / f"{prefix}-serial.raw"
    input_path = LOGS / f"{prefix}-input.raw"
    error_path = LOGS / f"{prefix}-stderr.log"
    started = time.monotonic()
    input_bytes = output_bytes = 0
    with ExitStack() as stack:
        output = stack.enter_context(output_path.open("wb"))
        inputs = stack.enter_context(input_path.open("wb"))
        errors = stack.enter_context(error_path.open("wb"))
        if transport == "pty":
            master, slave = os.openpty()
            stack.callback(os.close, master)
            # Start with a normal slave: the tested CLI must set its own raw mode.
            with os.fdopen(slave, "r+b", buffering=0) as terminal:
                guest = subprocess.Popen(argv, env=ENV, stdin=terminal, stdout=terminal, stderr=errors)
            rx = tx = master
        else:
            guest = subprocess.Popen(argv, env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors)
            rx, tx = guest.stdout.fileno(), guest.stdin.fileno()
        stack.enter_context(guest)

        def receive(seconds):
            nonlocal output_bytes
            if not select.select([rx], [], [], max(0, seconds))[0]:
                raise TimeoutError("no serial output before deadline")
            try:
                data = os.read(rx, 8192)
            except OSError as error:
                # Linux PTY masters return EIO after the last slave fd closes.
                if transport != "pty" or error.errno != errno.EIO:
                    raise
                return b""
            output_bytes += len(data)
            if output_bytes > (len(COMMANDS) + 2) * STEP_BYTES:
                raise ValueError("serial transcript exceeded its byte budget")
            output.write(data)
            return data

        def send(command):
            nonlocal input_bytes
            if input_bytes + len(command) > INPUT_BUDGET:
                raise ValueError("input transcript exceeded its byte budget")
            pending = memoryview(command)
            deadline = time.monotonic() + EXIT_SECONDS
            while pending:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([], [tx], [], remaining)[1]:
                    raise TimeoutError("serial input was not accepted before deadline")
                try:
                    count = os.write(tx, pending)
                except BlockingIOError:
                    continue
                if not count:
                    raise ValueError("serial input transport closed")
                inputs.write(pending[:count])
                input_bytes += count
                pending = pending[count:]

        try:
            os.set_blocking(tx, False)
            timings = drive(receive, send, manifest["kernelrelease"], manifest["isa"], boot_seconds=BOOT_SECONDS)
            send(QUIT)
            deadline = time.monotonic() + EXIT_SECONDS
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("serial transport stayed open after Ctrl-]")
                if not receive(remaining):
                    break
            while (rss := reap(guest, os.WNOHANG)) is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError("CLI stayed alive after Ctrl-]")
                time.sleep(0.01)
            runtime = time.monotonic() - started
            if guest.returncode != 0:
                raise ValueError(f"Ctrl-] exit status {guest.returncode}, expected 0")
        except (OSError, ValueError) as error:
            raise ValueError(f"{transport}: {error}; see {output_path} and {error_path}") from error
        finally:
            if guest.returncode is None:
                try:
                    os.kill(guest.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                reap(guest, 0)

    diagnostics = SUMMARY.findall(error_path.read_text(errors="replace"))
    if len(diagnostics) != 1 or diagnostics[0][0] != "quit":
        raise ValueError(f"{transport}: missing unambiguous Ctrl-] exit diagnostics; see {error_path}")
    _, fuel, limit, retired, tick = diagnostics[0]
    return dict(command=argv, exit_status=guest.returncode, serial_timing_seconds=timings,
                runtime_seconds=runtime, peak_rss_kib=rss,
                fuel_spent=int(fuel), max_fuel=int(limit), exit_retired=int(retired), exit_tick=int(tick),
                input_bytes=input_bytes, output_bytes=output_bytes, input_log=str(input_path),
                serial_log=str(output_path), diagnostics_log=str(error_path))


def negative_controls(binary, manifest):
    result = {}
    oversized_bytes = DRAM_END - DRAM + 1
    pinned_bytes = next(image["bytes"] for image in manifest["images"] if image["path"] == "Image")
    for name, target, reason in (
        ("missing-image", "Image", "No such file or directory"),
        ("corrupt-dtb", "board.dtb", r"SHA-256 mismatch"),
        ("oversized-image", "Image", f"size mismatch (expected {pinned_bytes} bytes, found {oversized_bytes})"),
    ):
        with tempfile.TemporaryDirectory(prefix="native-control-", dir=IMAGE_DIR.parent) as temporary:
            images = Path(temporary)
            for filename in ("manifest.json", *(image["path"] for image in manifest["images"])):
                if filename != target:
                    os.link(IMAGE_DIR / filename, images / filename)
            path = images / target
            if name == "corrupt-dtb":
                shutil.copyfile(IMAGE_DIR / target, path)
                with path.open("r+b") as stream:
                    stream.seek(-1, os.SEEK_END)
                    final = stream.read(1)[0]
                    stream.seek(-1, os.SEEK_END)
                    stream.write(bytes([final ^ 1]))
            elif name == "oversized-image":
                with path.open("xb") as stream:
                    stream.truncate(oversized_bytes)
            started = time.monotonic()
            check = subprocess.run([binary, "linux", "--images", images], env=ENV, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=EXIT_SECONDS)
            stderr = check.stderr.decode(errors="replace").strip()
            log = LOGS / f"hesper-{name}-stderr.log"
            log.write_bytes(check.stderr)
            if check.returncode <= 0 or check.stdout or target not in stderr or reason not in stderr:
                raise ValueError(f"{name}: expected {target} load error and empty stdout; status={check.returncode}, "
                                 f"stdout tail={check.stdout[-256:]!r}; see {log}")
            result[name] = dict(exit_status=check.returncode, stderr=stderr, stderr_log=str(log),
                                runtime_seconds=time.monotonic() - started)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built release Hesper CLI")
    binary = parser.parse_args().binary.resolve()
    if sys.platform != "linux":
        raise ValueError("the native serial gate requires Linux PTYs and wait4 RSS units")
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"not an executable Hesper binary: {binary}")
    LOGS.mkdir(parents=True, exist_ok=True)
    path = LOGS / "hesper-native.json"
    path.unlink(missing_ok=True)
    manifest = verified_manifest()
    report = dict(manifest=manifest, boot_deadline_seconds=BOOT_SECONDS, deadline_per_state_seconds=STEP_SECONDS,
                  byte_budget_per_state=STEP_BYTES,
                  input_byte_budget=INPUT_BUDGET, exit_deadline_seconds=EXIT_SECONDS,
                  negative_controls=negative_controls(binary, manifest),
                  transports={mode: run_serial(binary, manifest, mode) for mode in ("pipe", "pty")})
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Hesper cold boots, serial I/O, Ctrl-] exits and image rejection passed: {path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
