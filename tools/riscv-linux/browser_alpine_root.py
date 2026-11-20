"""Round-trip an Alpine ext4 system disk through release CLI and production Chromium."""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from alpine_acceptance import checked
from alpine_system_acceptance import CONFIG, POWER_OFF, ROOT_MOUNT, cold_states, install_states
from alpine_system_disk import SEED, SIZE, check_filesystem
from browser import BOOT_SECONDS, COMMAND_SECONDS, SETUP_SECONDS, browser_session, production_dist
from build_guest import ALPINE_IMAGE_DIR, ALPINE_ROOT_IMAGE_DIR, ALPINE_ROOT_LOGS, ENV, PROFILE, verified_manifest
from native_alpine_root import WRITE, boot, check_export
from serial_acceptance import session
from sources import require_tools, sha256

DOWNLOAD = "hesper-system.raw"
WEB_CONFIG = b"enabled=1\\nlabel=Hesper Chromium\\n"
UPDATE = checked("update-config-in-browser", b"printf '" + WEB_CONFIG + b"' > " + CONFIG
                 + b" && echo HESPER_WEB_SYSTEM_DISK:42", b"HESPER_WEB_SYSTEM_DISK:42")


def browser_round_trip(dist, manifest, cli_disk, logs):
    (logs / "downloads").mkdir(parents=True)
    corrupt_seed = logs / SEED
    with corrupt_seed.open("xb") as disk:
        disk.truncate(SIZE)
    argv = ["bun", PROFILE / "browser_session.mjs", dist, ALPINE_ROOT_IMAGE_DIR, logs,
            "alpine-root", cli_disk, corrupt_seed, ALPINE_IMAGE_DIR]
    read_and_update = (*cold_states(), UPDATE)
    dirty = (ROOT_MOUNT, WRITE)
    with browser_session(argv, logs, len(read_and_update) + 2 * len(dirty)) as (event, receive, send, keys, finish, _):
        def expect(kind, seconds):
            deadline = time.monotonic() + seconds
            while True:
                reply = event(deadline)
                if reply["type"] == kind:
                    return reply
                if reply["type"] != "serial":
                    raise ValueError(f"expected Chromium {kind}, got {reply}")

        def exchange(phase, states):
            start = expect("start", SETUP_SECONDS)
            if start["phase"] != phase or start["imageBytes"] != sum(item["bytes"] for item in manifest["images"]):
                raise ValueError(f"Chromium did not select the four pinned images for {phase}")
            timings = session(receive, keys, states, BOOT_SECONDS, step_seconds=COMMAND_SECONDS, allow_cursor_query=True)
            print(f"Chromium {phase}: reached shell in {timings['boot']:.2f}s and passed serial checks", flush=True)
            return dict(start=start, timings=timings)

        update = exchange("cli-to-web", read_and_update)
        keys(POWER_OFF)
        send(dict(type="download-system"))
        download = expect("download", COMMAND_SECONDS)
        if download["filename"] != DOWNLOAD or download["bytes"] != SIZE:
            raise ValueError(f"unexpected Chromium download: {download}")
        print(f"Chromium downloaded {DOWNLOAD} ({SIZE} bytes) after poweroff", flush=True)

        send(dict(type="boot-control", phase="forced-poweroff"))
        forced = exchange("forced-poweroff", dirty)
        keys(b"poweroff -f\n", kind="force-poweroff")
        forced["result"] = expect("forced-poweroff", COMMAND_SECONDS)

        send(dict(type="boot-control", phase="manual-stop"))
        stopped = exchange("manual-stop", dirty)
        send(dict(type="discard-system"))
        stopped["result"] = expect("discarded", SETUP_SECONDS)
        send(dict(type="finish"))
        interactions = finish()
    return dict(update=update, forced_poweroff=forced, manual_stop=stopped, download=download,
                interactions=interactions)


def run(binary, dist, logs, manifest):
    seed = ALPINE_ROOT_IMAGE_DIR / SEED
    cli_disk = logs / "cli-to-web.raw"
    install = boot(binary, logs / "cli-install", "pty", seed, cli_disk, states=install_states())
    cli_hash = check_export(install, cli_disk, logs / "cli-install-e2fsck.log")
    print("Release CLI installed mawk and exported a clean ext4 disk", flush=True)
    browser_logs = logs / "browser"
    browser = browser_round_trip(dist, manifest, cli_disk, browser_logs)
    downloaded = browser_logs / "downloads" / DOWNLOAD
    download_hash = sha256(downloaded)
    if download_hash == cli_hash:
        raise ValueError("Chromium downloaded the unchanged CLI disk")
    check_filesystem(downloaded, logs / "web-export-e2fsck.log", ENV)

    readback = logs / "cli-readback.raw"
    cold = boot(binary, logs / "cli-readback", "pty", downloaded, readback, states=cold_states(WEB_CONFIG))
    readback_hash = check_export(cold, readback, logs / "cli-readback-e2fsck.log")
    for source, expected in ((seed, manifest["seed"]["sha256"]), (cli_disk, cli_hash), (downloaded, download_hash)):
        if sha256(source) != expected:
            raise ValueError(f"input disk was modified: {source}")
    return dict(manifest=manifest, cli_install=install, browser=browser, cli_readback=cold,
                cli_sha256=cli_hash, browser_sha256=download_hash, readback_sha256=readback_hash)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="path to the built release Hesper CLI")
    binary = parser.parse_args().binary.resolve(strict=True)
    require_tools("e2fsck")
    dist = production_dist()
    manifest = verified_manifest(blk=True, alpine=True, root_disk=True)
    verified_manifest(alpine=True)
    ALPINE_ROOT_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="browser-", dir=ALPINE_ROOT_LOGS))
    print(f"Alpine system disk browser logs: {logs}", flush=True)
    report = run(binary, dist, logs, manifest)
    path = logs / "report.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Native Hesper and Chromium exchanged a persistent Alpine system disk: {path}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
