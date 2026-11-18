"""Prepare signed Alpine distribution inputs and pack them into an uncompressed initramfs."""

import argparse
from hashlib import sha256 as digest
import json
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys
import tarfile
import tempfile
from typing import NamedTuple
from urllib.parse import urlsplit

from newc import write_newc
from sources import CACHE, PROFILE, check_archive, download, require_tools

ALPINE_SOURCE_LOCK = PROFILE / "alpine-sources.lock.json"
INTERP = "lib/ld-musl-riscv64.so.1"
# M/A/C include Zmmul, Zaamo/Zalrsc and Zca; C with D includes Zcd.
ELF_ARCH = ("rv64i2p1_m2p0_a2p1_f2p2_d2p2_c2p0_zicsr2p0_zifencei2p0"
            "_zmmul1p0_zaamo1p0_zalrsc1p0_zca1p0_zcd1p0")


class Member(NamedTuple):
    mode: int
    uid: int
    gid: int
    data: bytes = b""
    major: int = 0
    minor: int = 0


def source_lock():
    return json.loads(ALPINE_SOURCE_LOCK.read_bytes())


def inputs(fetch=False, *, extra=None):
    entries = source_lock()["sources"]
    if set(entries) != {"root", "signature", "package", "key"}:
        raise ValueError("expected one locked tar, signature, package and Alpine signing key")
    if extra:
        entries.update(extra)
    directory = CACHE / "src" / "alpine"
    if fetch:
        directory.mkdir(parents=True, exist_ok=True)
    paths = {}
    for name, entry in entries.items():
        archive = entry["archive"]
        url = urlsplit(entry["url"])
        if (not re.fullmatch(r"[\w][\w.-]*", archive) or url.scheme != "https" or not url.hostname
                or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]) or not 0 < entry["bytes"] <= 512 * 1024**2):
            raise ValueError(f"invalid Alpine source lock entry for {name}")
        path = directory / archive
        if path.is_symlink():
            raise ValueError(f"{path}: source cache entry must not be a symlink")
        if not path.exists():
            if not fetch:
                raise FileNotFoundError(f"{path}: run make riscv-linux-alpine-sources first")
            require_tools("curl")
            download(entry, path)
        if not path.is_file() or path.stat().st_size != entry["bytes"]:
            raise ValueError(f"{path}: size mismatch")
        check_archive(path, entry["sha256"])
        paths[name] = path
        print(f"{entry['sha256']}  {path}", flush=True)
    return paths


def verify_signature(paths):
    require_tools("gpg", "gpgv")
    wanted = source_lock()["primary_fingerprint"]
    with tempfile.TemporaryDirectory(prefix="alpine-keyring-") as temp:
        keyring = Path(temp) / "alpine.gpg"
        gpg = ["gpg", "--no-options", "--batch", "--homedir", temp, "--no-autostart"]
        with keyring.open("wb") as output:
            subprocess.run([*gpg, "--output", "-", "--dearmor", paths["key"]], stdout=output, check=True)
        listing = subprocess.check_output([*gpg, "--with-colons", "--show-keys", keyring], text=True)
        records = [line.split(":") for line in listing.splitlines()]
        fingerprints = [following[9] for record, following in zip(records, records[1:])
                        if record[0] == "pub" and following[0] == "fpr"]
        if fingerprints != [wanted] or sum(record[0] == "pub" for record in records) != 1:
            raise ValueError(f"Alpine signing key: expected sole primary {wanted}, got {fingerprints}")
        status = subprocess.check_output(["gpgv", "--homedir", temp, "--keyring", keyring,
                                          "--status-fd", "1", paths["signature"], paths["root"]], text=True)
        signatures = [line.split()[2:] for line in status.splitlines() if line.startswith("[GNUPG:] VALIDSIG ")]
        primaries = [fields[-1] if len(fields) == 10 else fields[0]
                     for fields in signatures if len(fields) in (9, 10)]
        if (len(signatures) != 1 or primaries != [wanted]
                or re.search(r"^\[GNUPG:\] (EXPSIG|EXPKEYSIG|REVKEYSIG|BADSIG|ERRSIG)\b", status, re.M)):
            raise ValueError(f"Alpine tar signature does not name primary {wanted}")


def tar_members(path):
    entries = {}
    with tarfile.open(path, "r:gz", encoding="utf-8", errors="surrogateescape") as archive:
        for item in archive:
            name = item.name
            while name.startswith("./"):
                name = name[2:]
            if item.name in (".", "./") and item.isdir():
                name = "."
            if (not name or (name != "." and any(part in ("", ".", "..") for part in name.split("/")))
                    or name == "TRAILER!!!" or "\0" in name or name in entries):
                raise ValueError(f"{path}: invalid or duplicate entry {item.name!r}")
            if item.isdir():
                kind, data = stat.S_IFDIR, b""
            elif item.issym() and item.linkname and "\0" not in item.linkname:
                kind, data = stat.S_IFLNK, item.linkname.encode("utf-8", "surrogateescape")
            elif item.type in (tarfile.REGTYPE, tarfile.AREGTYPE):
                kind = stat.S_IFREG
                with archive.extractfile(item) as stream:
                    data = stream.read()
            else:
                raise ValueError(f"{path}: unsupported tar member {item.name!r} ({item.type!r})")
            if not (0 <= item.mode <= 0o7777 and 0 <= item.uid <= 0xffffffff and 0 <= item.gid <= 0xffffffff):
                raise ValueError(f"{path}: invalid metadata for {item.name!r}")
            entries[name] = Member(kind | item.mode, item.uid, item.gid, data)
    check_parents(entries)
    return entries


def check_parents(entries):
    for name in entries.keys() | {"."}:
        parent = name.rpartition("/")[0] or "."
        if parent not in entries or not stat.S_ISDIR(entries[parent].mode):
            raise ValueError(f"{name!r}: missing directory parent {parent!r}")


def cpio_members(path):
    contents = path.read_bytes()
    entries = {}
    offset = 0

    def aligned(end):
        padded = (end + 3) & ~3
        if padded > len(contents) or any(contents[end:padded]):
            raise ValueError(f"{path}: missing or nonzero newc padding at {end}")
        return padded

    while offset + 110 <= len(contents):
        if contents[offset:offset + 6] != b"070701":
            break
        header = contents[offset + 6:offset + 110]
        fields = [int(header[i:i + 8], 16) for i in range(0, len(header), 8)]
        ino, mode, uid, gid, links, mtime, size, dev1, dev2, major, minor, namesize, check = fields
        if (ino, links, mtime, dev1, dev2, check) != (len(entries) + 1, 1, 0, 0, 0, 0):
            raise ValueError(f"{path}: unexpected newc header at {offset}")
        name_end = offset + 110 + namesize
        raw_name = contents[offset + 110:name_end]
        if not raw_name.endswith(b"\0") or b"\0" in raw_name[:-1]:
            raise ValueError(f"{path}: invalid newc name at {offset}")
        name = raw_name[:-1].decode("utf-8", "surrogateescape")
        data_start = aligned(name_end)
        offset = aligned(data_start + size)
        member = Member(mode, uid, gid, contents[data_start:data_start + size], major, minor)
        if name == "TRAILER!!!":
            if member != Member(0, 0, 0) or any(contents[offset:]):
                raise ValueError(f"{path}: invalid newc trailer")
            return entries
        if name in entries:
            raise ValueError(f"{path}: duplicate newc member {name!r}")
        entries[name] = member
    raise ValueError(f"{path}: missing newc trailer")


def resolve_elf(entries, name):
    """Resolve guest links only; never extract or follow a path on the host."""
    for _ in range(40):
        entry = entries.get(name)
        if entry is not None and stat.S_ISREG(entry.mode):
            return name, entry.data
        if entry is None or not stat.S_ISLNK(entry.mode):
            break
        link = PurePosixPath(entry.data.decode("utf-8", "surrogateescape"))
        parts = [] if link.is_absolute() else name.split("/")[:-1]
        for part in link.parts[int(link.is_absolute()):]:
            if part == "..":
                if not parts:
                    raise ValueError(f"guest symlink escapes root: {name}")
                parts.pop()
            else:
                parts.append(part)
        name = "/".join(parts)
    raise ValueError(f"guest ELF does not resolve to a regular tar member: {name}")


def check_elfs(entries, readelf, logs):
    if entries["bin/sh"].data != b"/bin/busybox" or entries["lib/libc.musl-riscv64.so.1"].data != b"ld-musl-riscv64.so.1":
        raise ValueError("guest shell or libc symlink differs from expected Alpine target")
    if resolve_elf(entries, "bin/sh")[0] != "bin/busybox" or resolve_elf(entries, "lib/libc.musl-riscv64.so.1")[0] != INTERP:
        raise ValueError("guest shell or libc does not resolve to the pinned ELF")
    hashes = source_lock()["elf_sha256"]
    pending = {"bin/busybox", "sbin/apk", INTERP}
    checked = set()
    with tempfile.TemporaryDirectory(prefix="alpine-readelf-", dir=logs) as temp:
        while pending:
            requested = min(pending)
            pending.remove(requested)
            path, data = resolve_elf(entries, requested)
            if path in checked:
                continue
            checked.add(path)
            if path in hashes and digest(data).hexdigest() != hashes[path]:
                raise ValueError(f"{path}: locked original ELF hash differs")
            elf = Path(temp) / "guest.elf"
            elf.write_bytes(data)
            report = subprocess.check_output([readelf, "-h", "-lW", "-A", "-d", elf], text=True)
            (logs / (path.replace("/", "_") + ".readelf")).write_text(report)
            for field in (r"Class:\s+ELF64", r"Data:.*little endian", r"Machine:\s+RISC-V", r"Flags:.*double-float ABI"):
                if not re.search(field, report):
                    raise ValueError(f"{path}: invalid Alpine ELF: expected {field}")
            if re.findall(r'Tag_RISCV_arch:\s+"([^"]+)"', report) != [ELF_ARCH]:
                raise ValueError(f"{path}: RISC-V ISA attributes differ from the Alpine guest profile")
            if path in ("bin/busybox", "sbin/apk"):
                if re.findall(r"Requesting program interpreter: (.*?)\]", report) != [f"/{INTERP}"]:
                    raise ValueError(f"{path}: wrong PT_INTERP")
            for library in re.findall(r"\(NEEDED\).*\[(.*?)\]", report):
                if not re.fullmatch(r"[\w][\w.-]*", library):
                    raise ValueError(f"{path}: invalid DT_NEEDED name {library!r}")
                found = next((f"{folder}/{library}" for folder in ("lib", "usr/local/lib", "usr/lib")
                              if f"{folder}/{library}" in entries), None)
                if found is None:
                    raise ValueError(f"{path}: no Alpine member for DT_NEEDED {library}")
                pending.add(found)


def build_root(directory, prefix, paths, logs, *, modules=None, disk=False):
    directory.mkdir(parents=True, exist_ok=True)
    entries = tar_members(paths["root"])
    if not stat.S_ISREG(entries["lib/apk/db/installed"].mode) or not stat.S_ISDIR(entries["etc/apk/keys"].mode):
        raise ValueError("Alpine database or key directory has an unexpected type")
    if "usr/bin/mawk" in entries:
        raise ValueError("Alpine root must not already contain mawk")
    check_elfs(entries, f"{prefix}readelf", logs)
    obj, probe = directory / "fp-context.o", directory / "fp-context"
    subprocess.run([f"{prefix}as", "-march=rv64imafdc_zicsr_zifencei", "-mabi=lp64d", "-o", obj,
                    PROFILE / "fp-context.S"], check=True)
    subprocess.run([f"{prefix}ld", "-static", "-e", "_start", "--build-id=none", "-o", probe, obj], check=True)
    overlay = {"init": Member(stat.S_IFREG | 0o755, 0, 0, (PROFILE / "rootfs/init").read_bytes()),
               "dev/console": Member(stat.S_IFCHR | 0o600, 0, 0, major=5, minor=1),
               "opt/hesper": Member(stat.S_IFDIR | 0o755, 0, 0),
               f"opt/hesper/{paths['package'].name}": Member(stat.S_IFREG | 0o644, 0, 0, paths["package"].read_bytes()),
               "opt/hesper/fp-context": Member(stat.S_IFREG | 0o755, 0, 0, probe.read_bytes())}
    if modules:
        overlay.update(modules)
    if disk:
        overlay["opt/hesper/serial-init"] = overlay["init"]
        overlay["init"] = Member(stat.S_IFREG | 0o755, 0, 0, (PROFILE / "rootfs/alpine-blk-init").read_bytes())
    if overlap := entries.keys() & overlay.keys():
        raise ValueError(f"Alpine overlay conflicts: {sorted(overlap)}")
    expected = entries | overlay
    check_parents(expected)
    owners = {name: (item.uid, item.gid) for name, item in expected.items()}
    ordered = sorted(expected, key=lambda name: (name != ".", name.count("/"), name))
    records = [(name, item.mode, item.data, item.major, item.minor) for name in ordered for item in (expected[name],)]
    cpio = directory / "initramfs.cpio"
    write_newc(cpio, [*records, ("TRAILER!!!", 0, b"", 0, 0)], owners=owners)
    actual = cpio_members(cpio)
    if list(actual) != ordered or actual != expected:
        bad = [name for name in actual.keys() | expected.keys() if actual.get(name) != expected.get(name)]
        raise ValueError(f"{cpio}: newc does not match originals and overlay: {sorted(bad)}")
    return cpio


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify cache only, without downloading")
    args = parser.parse_args()
    verify_signature(inputs(fetch=not args.check))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
