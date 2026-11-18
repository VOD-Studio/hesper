"""Prove the locked Alpine userspace in the production Chromium workbench."""

import json
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time

from alpine_acceptance import PACKAGE, STATUS, cold_states, states
from browser import BOOT_SECONDS as STANDARD_BOOT_SECONDS, SETUP_SECONDS, browser_session, production_dist
from build_guest import ALPINE_IMAGE_DIR, ALPINE_LOGS, IMAGE_DIR, PROFILE, verified_manifest
from serial_acceptance import COMMANDS, STEP_BYTES, STEP_SECONDS, drive, session
from sources import sha256

BOOT_SECONDS = 300
COMMAND_SECONDS = 60
ARCHIVE = "initramfs.cpio"
VERIFY_LINE = (PACKAGE + b": OK").decode("ascii")


def run(dist, manifest, standard, logs):
    started = time.monotonic()
    archive = bytearray((ALPINE_IMAGE_DIR / ARCHIVE).read_bytes())
    archive[-1] ^= 1
    corrupt = logs / ARCHIVE
    corrupt.write_bytes(archive)

    install, cold = states(), cold_states()
    argv = ["bun", PROFILE / "browser_session.mjs", dist, ALPINE_IMAGE_DIR, logs, "alpine", IMAGE_DIR, corrupt]
    count = len(install) + len(cold) + len(COMMANDS) + 3  # three independently bounded boot transcripts
    phases = {}
    input_offset = 0
    with browser_session(argv, logs, count) as (event, receive, send, keys, complete, serial):
        def measure(phase, profile, pins, commands, boot_seconds, setup_seconds=SETUP_SECONDS):
            nonlocal input_offset
            start = event(time.monotonic() + setup_seconds)
            expected_bytes = sum(image["bytes"] for image in pins["images"])
            if (start["type"], start["phase"], start["profile"], start["imageBytes"]) != (
                    "start", phase, profile, expected_bytes):
                raise ValueError(f"unexpected browser image selection for {phase}: {start}")
            offset = len(serial)
            if profile == "alpine":
                timings = session(receive, keys, commands, boot_seconds, step_seconds=COMMAND_SECONDS,
                                  allow_cursor_query=True)
                status = dict(zip((name for name, _, _ in commands),
                                  map(int, STATUS.findall(bytes(serial[offset:]).replace(b"\r\n", b"\n"))), strict=True))
                command_bytes = sum(len(command) for _, command, _ in commands)
            else:
                timings = drive(receive, keys, pins["kernelrelease"], pins["isa"], boot_seconds=boot_seconds)
                status = None
                command_bytes = sum(map(len, COMMANDS))
            end_input = input_offset + command_bytes
            phases[phase] = dict(start=start, timing_seconds=timings, exit_status_by_command=status,
                                 boot_deadline_seconds=boot_seconds,
                                 command_deadline_seconds=COMMAND_SECONDS if profile == "alpine" else STEP_SECONDS,
                                 serial_offsets=[offset, len(serial)], input_offsets=[input_offset, end_input])
            input_offset = end_input

        measure("install", "alpine", manifest, install, BOOT_SECONDS)
        send(dict(type="cold"))
        measure("cold", "alpine", manifest, cold, BOOT_SECONDS)
        send(dict(type="standard", resultLine=VERIFY_LINE))
        # This transition also stops Alpine and runs both rejected boots through the UI.
        measure("standard", "standard", standard, COMMANDS, STANDARD_BOOT_SECONDS, setup_seconds=COMMAND_SECONDS)
        send(dict(type="finish"))
        finished = complete()

    for phase, proof in phases.items():
        details = finished["phases"][phase]
        if details["id"] != proof["start"]["id"]:
            raise ValueError(f"browser ended a different {phase} session")
        proof.update(worker=details)
    wasm = dist / finished["wasmPath"].removeprefix("/hesper/")
    return dict(manifest=manifest, standard_manifest=standard, browser=phases["install"]["start"]["browser"],
                url=phases["install"]["start"]["url"], host=platform.platform(), production_dist=str(dist),
                wasm=dict(path=finished["wasmPath"], sha256=sha256(wasm)),
                phases=phases, rejected=finished["rejected"], command_byte_limit=STEP_BYTES,
                guest_http_requests=finished["guestRequests"], guest_http_download_bytes=finished["guestDownloadedBytes"],
                chromium_guest_requests=finished["guestNetworkRequests"],
                chromium_guest_transfer_bytes=finished["guestNetworkBytes"],
                max_main_interval_ms=finished["maxIntervalMs"], elapsed_seconds=time.monotonic() - started,
                serial_log=str(logs / "browser-serial.raw"), input_log=str(logs / "browser-input.raw"),
                worker_log=str(logs / "browser-worker-events.jsonl"), network_log=str(logs / "browser-network.jsonl"))


def main():
    dist = production_dist()
    manifest = verified_manifest(alpine=True)
    standard = verified_manifest()
    ALPINE_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="browser-", dir=ALPINE_LOGS))
    result = run(dist, manifest, standard, logs)
    report = logs / "report.json"
    report.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Production Chromium installed and ran signed mawk offline (xterm: 7.500), "
          f"then verified a clean Alpine restart, rejected invalid images and booted standard Linux: {report}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")
