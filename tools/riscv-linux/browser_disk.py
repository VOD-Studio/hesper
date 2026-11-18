"""Round-trip a real ext2 disk between a fresh CLI and production Chrome."""

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
import time

from browser import BOOT_SECONDS, EXIT_SECONDS, SETUP_SECONDS, browser_session, production_dist
from build_guest import BLK_IMAGE_DIR, BLK_LOGS, PROFILE, verified_manifest
from disk_acceptance import commands, drive_disk, make_seed, check_proof
from native import run_serial
from serial_acceptance import result_line, session
from sources import require_tools, sha256

CLI_TO_WEB = "CLI_TO_WEB"
WEB_TO_CLI = "WEB_TO_CLI"
AFTER_RESUME = "WEB_AFTER_RESUME"
DISCARDED = "WEB_DISCARDED"
EXPORTS = ("first", "after-resume")
DOWNLOAD_NAME = "hesper-data.raw"
# One whole sector beyond the product limit, without any host allocation.
OVERSIZED_BYTES = 16 * 1024 * 1024 + 512


def run_browser(dist, manifest, cli_disk, browser_seed, logs):
    oversized = logs / "oversized.raw"
    with oversized.open("xb") as stream:
        stream.truncate(OVERSIZED_BYTES)
    for label in (*EXPORTS, "race"):
        (logs / f"download-{label}").mkdir()
    argv = ["bun", PROFILE / "browser_session.mjs", dist, BLK_IMAGE_DIR, logs, "disk",
            cli_disk, browser_seed, oversized]
    read_commands = commands(False, CLI_TO_WEB)
    capacity, initial_write = commands(True, WEB_TO_CLI)
    _, continued_write = commands(True, AFTER_RESUME)
    _, discarded_write = commands(True, DISCARDED)
    exports = {}

    with browser_session(argv, logs, len(read_commands) + 4) as (event, receive, send, keys, complete, _):
        def start(phase, seconds):
            reply = event(time.monotonic() + seconds)
            if (reply["type"] != "start" or reply["phase"] != phase
                    or reply["imageBytes"] != sum(item["bytes"] for item in manifest["images"])):
                raise ValueError(f"browser did not select the four pinned blk images for {phase}")
            return reply

        def require(lines, export=None):
            def check(output):
                if not all(result_line(output, line) for line in lines):
                    return False
                if export is not None:
                    send(dict(type="export", label=export))
                    reply = event(time.monotonic() + EXIT_SECONDS)
                    if (reply["type"] != "download" or reply["label"] != export
                            or reply["filename"] != DOWNLOAD_NAME):
                        raise ValueError(f"expected completed browser download for {export}")
                    exports[export] = reply
                return True
            return check

        opened = start("cli-to-web", SETUP_SECONDS)
        read = drive_disk(receive, keys, False, CLI_TO_WEB, boot_seconds=BOOT_SECONDS)
        send(dict(type="write-phase"))
        start("web-to-cli", SETUP_SECONDS)
        states = [("capacity", capacity[0], require(capacity[1])),
                  ("write", initial_write[0], require(initial_write[1], EXPORTS[0])),
                  ("write-after-resume", continued_write[0], require(continued_write[1], EXPORTS[1])),
                  ("write-before-discard", discarded_write[0], require(discarded_write[1]))]
        write = session(receive, keys, states, BOOT_SECONDS)
        send(dict(type="finish"))
        result = complete()
    return dict(browser=opened["browser"], url=opened["url"], cli_to_web_timings=read,
                web_to_cli_timings=write, downloads=exports, interactions=result, logs=str(logs))


def run(binary, dist, manifest, logs):
    cli_seed = logs / "cli-seed.raw"
    browser_seed = logs / "browser-seed.raw"
    cli_seed_report = make_seed(cli_seed)
    browser_seed_report = make_seed(browser_seed)
    if cli_seed_report["sha256"] == browser_seed_report["sha256"]:
        raise ValueError("independent ext2 seeds are identical")

    cli_disk = logs / "cli-to-web.raw"
    cli_write = run_serial(binary, manifest, "pty", disk=(cli_seed, cli_disk), write=True,
                           marker=CLI_TO_WEB, logs=logs / "cli-write")
    cli_written_hash = check_proof(cli_disk, CLI_TO_WEB)
    browser_logs = logs / "browser"
    browser_logs.mkdir()
    browser = run_browser(dist, manifest, cli_disk, browser_seed, browser_logs)

    first_download, next_download = (browser_logs / f"download-{label}" / DOWNLOAD_NAME for label in EXPORTS)
    first_hash = check_proof(first_download, WEB_TO_CLI)
    next_hash = check_proof(next_download, AFTER_RESUME)
    cli_readback = logs / "cli-readback.raw"
    cli_read = run_serial(binary, manifest, "pty", disk=(first_download, cli_readback), write=False,
                          marker=WEB_TO_CLI, logs=logs / "cli-read")
    readback_hash = check_proof(cli_readback, WEB_TO_CLI)
    for path, expected in ((cli_seed, cli_seed_report["sha256"]), (browser_seed, browser_seed_report["sha256"]),
                           (cli_disk, cli_written_hash), (first_download, first_hash), (next_download, next_hash)):
        if sha256(path) != expected:
            raise ValueError(f"disk source changed: {path}")
    return dict(manifest=manifest, seeds=dict(cli=cli_seed_report, browser=browser_seed_report),
                cli_write=cli_write, cli_read=cli_read, browser=browser,
                disk_sha256={str(path): digest for path, digest in ((cli_disk, cli_written_hash),
                              (first_download, first_hash), (next_download, next_hash), (cli_readback, readback_hash))})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("hesper", type=Path, help="built native Hesper CLI")
    binary = parser.parse_args().hesper.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError(f"not an executable Hesper CLI: {binary}")
    require_tools("mke2fs", "debugfs")
    dist = production_dist()
    manifest = verified_manifest(blk=True)
    BLK_LOGS.mkdir(parents=True, exist_ok=True)
    report = BLK_LOGS / "hesper-blk-browser.json"
    report.unlink(missing_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="browser-", dir=BLK_LOGS))
    result = run(binary, dist, manifest, logs)
    report.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Production Chrome and native CLI exchanged ext2 data disks: {report}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        sys.exit(f"error: {error}")
