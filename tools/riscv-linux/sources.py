"""Fetch or check L1's release inputs. Does not build or start a guest."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit

PROFILE = Path(__file__).resolve().parent
CACHE = Path(os.environ.get("RISCV_LINUX_CACHE", PROFILE.parent.parent / ".cache/riscv-linux")).resolve()
LOCK = PROFILE / "sources.lock.json"
RELEASES = {"buildroot", "opensbi", "linux", "qemu"}


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def require_tools(*commands):
    missing = [command for command in commands if not shutil.which(command)]
    if missing:
        raise ValueError(f"missing host tools: {', '.join(missing)}")


def releases():
    inputs = json.loads(LOCK.read_bytes())["sources"]
    if len(inputs) != len(RELEASES) or {entry["name"] for entry in inputs} != RELEASES:
        raise ValueError(f"{LOCK}: expected one locked release each for Buildroot, OpenSBI, Linux and QEMU")
    archives = set()
    for entry in inputs:
        archive = entry["archive"]
        url = urlsplit(entry["url"])
        if (not re.fullmatch(r"[\w.-]+\.tar\.(xz|gz)", archive) or archive in archives
                or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"])
                or url.scheme != "https" or not url.hostname):
            raise ValueError(f"{LOCK}: invalid source lock entry for {entry['name']}")
        archives.add(archive)
    return inputs


def check_archive(path, expected):
    actual = sha256(path)
    if actual != expected:
        raise ValueError(f"{path}: SHA-256 mismatch; expected {expected}, got {actual}")


def download(entry, archive):
    # A failed or interrupted download must never become a named cache input.
    with tempfile.TemporaryDirectory(prefix="download-", dir=archive.parent) as temporary:
        partial = Path(temporary) / archive.name
        subprocess.run([
            "curl", "--fail", "--silent", "--show-error", "--location",
            "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "20",
            "--max-time", "300", "--max-filesize", str(entry.get("bytes", 512 * 1024 * 1024)),
            "--output", partial, entry["url"],
        ], check=True)
        if "bytes" in entry and partial.stat().st_size != entry["bytes"]:
            raise ValueError(f"{archive}: size mismatch")
        check_archive(partial, entry["sha256"])
        partial.replace(archive)


def archives(fetch=False):
    inputs = releases()
    directory = CACHE / "src"
    if fetch:
        require_tools("curl")
        directory.mkdir(parents=True, exist_ok=True)
    paths = {}
    for entry in inputs:
        archive = directory / entry["archive"]
        if archive.is_symlink():
            raise ValueError(f"{archive}: source cache entry must not be a symlink")
        if not archive.exists():
            if not fetch:
                raise FileNotFoundError(f"{archive}: run make riscv-linux-sources first")
            download(entry, archive)
        else:
            check_archive(archive, entry["sha256"])
        paths[entry["name"]] = archive
        print(f"{entry['sha256']}  {archive}", flush=True)
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-only", action="store_true", help="validate the cache without using the network")
    args = parser.parse_args()
    try:
        if not args.verify_only:
            require_tools("make", "gcc", "g++", "git", "perl", "tar", "wget", "cpio", "unzip",
                          "rsync", "patch", "file", "bzip2", "xz")
        archives(fetch=not args.verify_only)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")


if __name__ == "__main__":
    main()
