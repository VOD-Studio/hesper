"""Cold boot the actual Node.js and Bun Wasm binding, with the native serial assertions."""

from contextlib import ExitStack
import json
import os
import select
import subprocess
import sys
import time

from build_guest import ENV, IMAGE_DIR, LOGS, verified_manifest
from native import EXIT_SECONDS, QUIT, SUMMARY, reap
from serial_acceptance import COMMANDS, STEP_BYTES, STEP_SECONDS, drive
from sources import PROFILE, require_tools

BOOT_SECONDS = 180


def run(runtime, manifest):
    argv = [runtime, str(PROFILE / "wasm_serial.mjs"), str(IMAGE_DIR)]
    prefix = LOGS / f"wasm-{runtime}"
    with ExitStack() as stack:
        serial = stack.enter_context(prefix.with_suffix(".serial.raw").open("wb"))
        inputs = stack.enter_context(prefix.with_suffix(".input.raw").open("wb"))
        stderr_path = prefix.with_suffix(".stderr.log")
        stderr = stack.enter_context(stderr_path.open("wb"))
        guest = stack.enter_context(subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, env=ENV))
        started = time.monotonic()
        output_bytes = 0

        def receive(seconds):
            nonlocal output_bytes
            if not select.select((guest.stdout,), (), (), max(seconds, 0))[0]:
                raise TimeoutError("Wasm serial deadline")
            data = os.read(guest.stdout.fileno(), 8192)
            output_bytes += len(data)
            if output_bytes > (len(COMMANDS) + 2) * STEP_BYTES:
                raise ValueError("serial transcript exceeded its byte budget")
            serial.write(data)
            return data

        def send(data):
            guest.stdin.write(data)
            guest.stdin.flush()
            inputs.write(data)

        try:
            times = drive(receive, send, manifest["kernelrelease"], manifest["isa"], boot_seconds=BOOT_SECONDS)
            send(QUIT)
            deadline = time.monotonic() + EXIT_SECONDS
            while receive(deadline - time.monotonic()):
                if time.monotonic() >= deadline:
                    raise TimeoutError("Wasm output still open after quit")
            while (rss := reap(guest, os.WNOHANG)) is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError("Wasm process still running after quit")
                time.sleep(0.01)
            summary = SUMMARY.findall(stderr_path.read_text(errors="replace"))
            if guest.returncode != 0 or len(summary) != 1 or summary[0][0] != "quit":
                raise ValueError(f"unclean Wasm quit (status {guest.returncode})")
        except (OSError, ValueError) as error:
            raise ValueError(f"{runtime}: {error}; see {prefix}.serial.raw and {stderr_path}") from error
        finally:
            if guest.returncode is None:
                guest.kill()
                reap(guest, 0)

    _, fuel, budget, retired, ticks = summary[0]
    return dict(command=argv, serial_timing_seconds=times, runtime_seconds=time.monotonic() - started,
                peak_rss_kib=rss, fuel_spent=int(fuel), max_fuel=int(budget), exit_retired=int(retired),
                exit_tick=int(ticks), output_bytes=output_bytes, serial_log=str(prefix.with_suffix(".serial.raw")))


def main():
    require_tools("bun", "node")
    LOGS.mkdir(parents=True, exist_ok=True)
    report = LOGS / "hesper-wasm.json"
    report.unlink(missing_ok=True)
    manifest = verified_manifest()
    results = {}
    for runtime in ("bun", "node"):
        results[runtime] = run(runtime, manifest)
        print(f"{runtime} Linux serial passed: prompt after {results[runtime]['serial_timing_seconds']['boot']:.2f}s", flush=True)
    report.write_text(json.dumps(dict(manifest=manifest, boot_deadline_seconds=BOOT_SECONDS,
                                      deadline_per_command_seconds=STEP_SECONDS, runs=results), indent=2) + "\n")
    print(report)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")
