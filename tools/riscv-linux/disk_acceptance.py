"""Shared real-guest data-disk protocol and host-side ext2 proof."""

import subprocess

from build_guest import ENV
from serial_acceptance import result_line, session
from sources import sha256

SECTORS = 16384
CAPACITY = b"cat /sys/class/block/vda/size\n"


def proof(marker):
    return f'echo "{marker}:$((6 * 7))"\n'.encode("ascii")


def commands(write, marker):
    script = proof(marker).rstrip(b"\n")
    mount = b"mkdir -p /mnt/data && mount -t ext2 "
    target = b"/dev/vda /mnt/data && "
    if write:
        command = (mount + target + b"printf '%s\\n' '" + script + b"' > /mnt/data/proof.sh && sync && "
                   b"umount /mnt/data && echo DISK_UNMOUNTED\n")
        expected = (b"DISK_UNMOUNTED",)
    else:
        command = mount + b"-o ro " + target + b"sh /mnt/data/proof.sh && umount /mnt/data && echo DISK_UNMOUNTED\n"
        expected = (f"{marker}:42".encode("ascii"), b"DISK_UNMOUNTED")
    return ((CAPACITY, (str(SECTORS).encode("ascii"),)), (command, expected))


def drive_disk(receive, send, write, marker, *, boot_seconds):
    def require(lines):
        return lambda output: all(result_line(output, line) for line in lines)

    states = [(name, command, require(lines)) for name, (command, lines) in
              zip(("capacity", "write" if write else "read"), commands(write, marker))]
    return session(receive, send, states, boot_seconds)


def make_seed(path):
    with path.open("xb") as stream:
        stream.truncate(SECTORS * 512)
    args = ["mke2fs", "-q", "-F", "-t", "ext2", "-b", "1024", str(path)]
    subprocess.run(args, env=ENV, check=True, timeout=10)
    version = subprocess.run(["mke2fs", "-V"], env=ENV, check=True, capture_output=True, text=True, timeout=10)
    return dict(arguments=args, version=version.stderr.strip(), sectors=SECTORS, sha256=sha256(path))


def check_proof(path, marker):
    actual = subprocess.run(["debugfs", "-R", "cat /proof.sh", path], env=ENV, check=True,
                            stdout=subprocess.PIPE, timeout=10).stdout
    if actual != proof(marker):
        raise ValueError(f"{path}: wrong /proof.sh: {actual!r}")
    return sha256(path)
