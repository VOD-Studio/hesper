"""Authenticate the pinned Alpine APK and recover the official kernel and module."""

import argparse
import gzip
from hashlib import sha256
from io import BytesIO
import json
from pathlib import Path
import stat
import subprocess
import sys
import tarfile
import tempfile
import zlib

from alpine_root import Member, inputs, tar_members, verify_signature
from sources import PROFILE, archives, require_tools

FIRMWARE_SOURCES = {"buildroot", "opensbi"}
LOCK = PROFILE / "alpine-kernel.lock.json"
PIN = json.loads(LOCK.read_bytes())
RELEASE = PIN["release"]
MODULE = f"lib/modules/{RELEASE}/kernel/kernel/configs.ko"


def gzip_member(data, limit):
    stream = zlib.decompressobj(16 + zlib.MAX_WBITS)
    payload = stream.decompress(data, limit + 1)
    if len(payload) > limit or not stream.eof:
        raise ValueError("invalid or oversized Alpine APK gzip member")
    return payload, stream.unused_data


def read_member(archive, name):
    members = [item for item in archive.getmembers() if item.name == name]
    if len(members) != 1 or not members[0].isfile():
        raise ValueError(f"expected one regular Alpine APK member {name}")
    with archive.extractfile(members[0]) as stream:
        return stream.read()


def authenticated_data(paths):
    require_tools("openssl")
    signature_tar, remainder = gzip_member(paths["kernel"].read_bytes(), 64 * 1024)
    control_tar, compressed_data = gzip_member(remainder, 1024 * 1024)
    signed_control = remainder[:len(remainder) - len(compressed_data)]
    key_name = PIN["signing_key"]
    # The caller has authenticated the distribution tarball with GPG.
    key = tar_members(paths["root"])[f"etc/apk/keys/{key_name}"]
    if not stat.S_ISREG(key.mode):
        raise ValueError("Alpine APK key must be a regular file")
    with tarfile.open(fileobj=BytesIO(signature_tar)) as archive:
        signature = read_member(archive, f".SIGN.RSA.{key_name}")
    with tempfile.TemporaryDirectory(prefix="alpine-apk-") as temp:
        sig, pub = Path(temp) / "signature", Path(temp) / "key"
        sig.write_bytes(signature)
        pub.write_bytes(key.data)
        # APK v2 RSA signs the compressed control member using SHA-1.
        subprocess.run(["openssl", "dgst", "-sha1", "-verify", pub, "-signature", sig],
                       input=signed_control, check=True)
    with tarfile.open(fileobj=BytesIO(control_tar)) as archive:
        metadata = read_member(archive, ".PKGINFO").decode().splitlines()
    for expected in ("pkgname = linux-lts", "arch = riscv64",
                     f"pkgver = {PIN['source']['archive'].removeprefix('linux-lts-').removesuffix('.apk')}",
                     f"datahash = {sha256(compressed_data).hexdigest()}"):
        if metadata.count(expected) != 1:
            raise ValueError(f"Alpine APK control lacks {expected}")
    data_tar, trailing = gzip_member(compressed_data, 64 * 1024**2)
    if trailing:
        raise ValueError("trailing data after Alpine APK")
    return data_tar


def extract(paths):
    with tarfile.open(fileobj=BytesIO(authenticated_data(paths))) as archive:
        image = gzip.decompress(read_member(archive, "boot/vmlinuz-lts"))
        config = read_member(archive, f"boot/config-{RELEASE}")
        module = gzip.decompress(read_member(archive, MODULE + ".gz"))
    for name, payload in (("image", image), ("config", config), ("module", module)):
        if sha256(payload).hexdigest() != PIN[f"{name}_sha256"]:
            raise ValueError(f"Alpine {name} differs from its locked bytes")
    if (f"\0vermagic={RELEASE} ".encode() not in module or b"\0depends=\0" not in module
            or not {b"CONFIG_IKCONFIG=m", b"CONFIG_IKCONFIG_PROC=y"} <= set(config.splitlines())):
        raise ValueError("expected a standalone configs module for the pinned Alpine kernel")
    return image, module


def module_overlay(module):
    overlay = {MODULE: Member(stat.S_IFREG | 0o644, 0, 0, module)}
    parent = Path(MODULE).parent
    while parent != Path("lib"):
        overlay[parent.as_posix()] = Member(stat.S_IFDIR | 0o755, 0, 0)
        parent = parent.parent
    return overlay


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="verify cache only, without downloading")
    args = parser.parse_args()
    paths = inputs(fetch=not args.check, extra={"kernel": PIN["source"]})
    verify_signature(paths)
    extract(paths)
    archives(fetch=not args.check, names=FIRMWARE_SOURCES)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"error: {error}")
