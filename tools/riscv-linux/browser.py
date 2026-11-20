"""Boot real pinned Linux in the production workbench using a temporary Chromium."""

import base64
from contextlib import ExitStack, contextmanager
import json
import os
from pathlib import Path
import platform
import re
import select
import signal
import subprocess
import sys
import tempfile
import time

from build_guest import ENV, IMAGE_DIR, LOGS, verified_manifest
from serial_acceptance import STEP_BYTES, result_line, session, shell_states
from sources import PROFILE, require_tools

BOOT_SECONDS = 180
COMMAND_SECONDS = 60
SETUP_SECONDS = 25
EXIT_SECONDS = 20
MAX_INTERVAL_MS = 350


@contextmanager
def browser_session(argv, logs, command_count):
    with ExitStack() as stack:
        stderr_path = logs / "browser-driver.stderr.log"
        errors = stack.enter_context(stderr_path.open("wb"))
        io_path = logs / "browser-serial.raw"
        output = stack.enter_context(io_path.open("wb"))
        inputs = stack.enter_context((logs / "browser-input.raw").open("wb"))
        guest = stack.enter_context(subprocess.Popen(argv, env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                                    stderr=errors, start_new_session=True))
        wire = bytearray()
        serial = bytearray()

        def event(deadline):
            while b"\n" not in wire:
                seconds = deadline - time.monotonic()
                if seconds <= 0 or not select.select((guest.stdout,), (), (), seconds)[0]:
                    raise TimeoutError("browser transport deadline")
                data = os.read(guest.stdout.fileno(), 8192)
                if not data:
                    raise EOFError("browser transport closed")
                wire.extend(data)
                if len(wire) > STEP_BYTES:
                    raise ValueError("browser message exceeded its byte budget")
            line, _, suffix = wire.partition(b"\n")
            wire[:] = suffix
            reply = json.loads(line)
            if reply["type"] == "error":
                raise ValueError(reply["message"])
            if reply["type"] == "serial":
                data = base64.b64decode(reply["bytes"], validate=True)
                serial.extend(data)
                output.write(data)
                if len(serial) > (command_count + 2) * STEP_BYTES:
                    raise ValueError("browser serial transcript exceeded its byte budget")
                reply["bytes"] = data
            return reply

        def receive(seconds):
            reply = event(time.monotonic() + seconds)
            if reply["type"] != "serial":
                raise ValueError(f"expected serial bytes, got {reply['type']}")
            return reply["bytes"]

        def send(data):
            guest.stdin.write(json.dumps(data).encode("ascii") + b"\n")
            guest.stdin.flush()

        def keys(data, *, kind="keys"):
            send(dict(type=kind, text=data.decode("ascii")))
            inputs.write(data)

        def finish():
            reply = event(time.monotonic() + EXIT_SECONDS)
            guest.stdin.close()
            if reply["type"] != "done" or guest.wait(timeout=EXIT_SECONDS) != 0:
                raise ValueError("browser did not complete the acceptance session")
            return reply

        try:
            yield event, receive, send, keys, finish, serial
        except (OSError, EOFError, ValueError, subprocess.TimeoutExpired) as error:
            raise ValueError(f"{error}; see {io_path} and {stderr_path}") from error
        finally:
            if guest.poll() is None:
                if not guest.stdin.closed:
                    guest.stdin.close()
                try:
                    guest.wait(timeout=EXIT_SECONDS)
                except subprocess.TimeoutExpired:
                    os.killpg(guest.pid, signal.SIGKILL)
                    guest.wait()


def run(dist, manifest):
    with tempfile.TemporaryDirectory(prefix="browser-image-", dir=LOGS) as directory:
        corrupt = Path(directory)
        for item in manifest["images"]:
            name = item["path"]
            if name == "board.dtb":
                board = bytearray((IMAGE_DIR / name).read_bytes())
                board[-1] ^= 1
                (corrupt / name).write_bytes(board)
            else:
                os.link(IMAGE_DIR / name, corrupt / name)
        argv = ["bun", PROFILE / "browser_session.mjs", dist, IMAGE_DIR, LOGS, "serial", corrupt]
        states = (*shell_states(manifest["kernelrelease"], manifest["isa"]),
                  ("past-former-limit", b"sleep 4000 && printf 'HESPER_LONG:%s\\n' \"$((6 * 7))\"\n",
                   lambda output: result_line(output, b"HESPER_LONG:42")))
        with browser_session(argv, LOGS, len(states)) as (event, receive, send, keys, complete, serial):
            start = event(time.monotonic() + SETUP_SECONDS)
            if start["type"] != "start" or start["imageBytes"] != sum(entry["bytes"] for entry in manifest["images"]):
                raise ValueError("browser did not select the four pinned local resources")
            times = session(receive, keys, states, BOOT_SECONDS, step_seconds=COMMAND_SECONDS)
            if len(re.findall(rb"(?:^|\n)Linux \S+ " + re.escape(manifest["kernelrelease"].encode("ascii")), serial)) != 1:
                raise ValueError("paused uname command must be delivered exactly once")
            send(dict(type="finish"))
            finish = complete()
            if finish["maxIntervalMs"] > MAX_INTERVAL_MS:
                raise ValueError(f"main-thread response gap exceeded {MAX_INTERVAL_MS} ms: {finish['maxIntervalMs']}")

    return dict(manifest=manifest, browser=start["browser"], url=start["url"], host=platform.platform(),
                guest_http_download_bytes=finish["guestDownloadedBytes"], local_image_bytes=start["imageBytes"],
                serial_timing_seconds=times, boot_deadline_seconds=BOOT_SECONDS,
                deadline_per_command_seconds=COMMAND_SECONDS, serial_bytes=len(serial),
                paused_tick=finish["pausedTick"], guest_exit_tick=finish["tick"], guest_exit_retired=finish["retired"],
                guest_exit_fuel=finish["fuel"],
                max_main_interval_ms=finish["maxIntervalMs"], main_interval_limit_ms=MAX_INTERVAL_MS,
                serial_log=str(LOGS / "browser-serial.raw"), screenshot=str(LOGS / "browser-terminal.png"))


def production_dist():
    require_tools("bun", os.environ.get("CHROME_BIN") or "google-chrome")
    dist = PROFILE.parent.parent / "examples/web/dist"
    if not (dist / "index.html").is_file():
        raise ValueError("missing production site; run bun run build in examples/web")
    return dist


def main():
    dist = production_dist()
    LOGS.mkdir(parents=True, exist_ok=True)
    report = LOGS / "hesper-browser.json"
    report.unlink(missing_ok=True)
    result = run(dist, verified_manifest())
    report.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Production Chrome Linux shell, pause/continue, cancel and cold restart passed: {report}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")
