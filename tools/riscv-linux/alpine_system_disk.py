"""Build the Alpine ext4 seed and its switch-root initramfs."""

from io import BytesIO
from pathlib import Path
import stat
import subprocess
import tarfile
import tempfile

import alpine_kernel
from alpine_root import INTERP, Member, add_overlay, check_parents, ordered_paths, pack_newc, root_members
from sources import PROFILE, require_tools, sha256

SEED = "seed.raw"
SIZE = 16 * 1024**2
BLOCK_SIZE = 1024
EPOCH = 1700000000
UUID = "d46a1d4e-5cac-4f9d-810c-72ad44fc3a02"
HASH_SEED = "90848f2f-c15a-47a9-8c1a-603267b27030"
FEATURES = ("has_journal,ext_attr,dir_index,filetype,64bit,sparse_super,"
            "large_file,huge_file,dir_nlink,extra_isize,metadata_csum")
TAR_TYPES = {stat.S_IFDIR: tarfile.DIRTYPE, stat.S_IFREG: tarfile.REGTYPE, stat.S_IFLNK: tarfile.SYMTYPE}


def seed_tar(path, entries):
    check_parents(entries)
    with tarfile.open(path, "w", format=tarfile.PAX_FORMAT) as archive:
        for name in ordered_paths(entries):
            member = entries[name]
            record = tarfile.TarInfo(name)
            record.type = TAR_TYPES[stat.S_IFMT(member.mode)]
            record.mode = stat.S_IMODE(member.mode)
            record.uid, record.gid = member.uid, member.gid
            record.mtime = EPOCH
            if record.issym():
                record.linkname = member.data.decode("utf-8", "surrogateescape")
            elif record.isfile():
                record.size = len(member.data)
            archive.addfile(record, BytesIO(member.data) if record.isfile() else None)


def make_seed(disk, archive, config, env):
    disk.touch(exist_ok=False)
    subprocess.run([
        "mke2fs", "-q", "-t", "ext4", "-T", "ext4", "-b", str(BLOCK_SIZE), "-I", "256", "-N", "2048", "-m", "0",
        "-J", "size=1", "-L", "hesper-root", "-U", UUID,
        "-E", f"hash_seed={HASH_SEED},root_owner=0:0,root_perms=0755,lazy_itable_init=0,lazy_journal_init=0,nodiscard",
        "-d", archive, disk, str(SIZE // BLOCK_SIZE),
    ], env=env | {"MKE2FS_CONFIG": str(config), "E2FSPROGS_FAKE_TIME": str(EPOCH)}, check=True)


def check_filesystem(disk, log, env):
    with log.open("wb") as output:
        subprocess.run(["e2fsck", "-fn", disk], env=env, stdout=output, stderr=subprocess.STDOUT, check=True)


def build(directory, prefix, paths, logs, modules, env):
    require_tools("mke2fs", "e2fsck")
    entries = root_members(directory, prefix, paths, logs)
    early = {name: entries[name] for name in (
        ".", "bin", "bin/busybox", "bin/sh", "dev", "lib", INTERP, "lib/libc.musl-riscv64.so.1",
        "mnt", "opt", "opt/hesper", "proc", "sys",
    )}
    early.update(alpine_kernel.module_overlay({name: modules[name] for name in alpine_kernel.DISK_MODULES}))
    early["dev/console"] = Member(stat.S_IFCHR | 0o600, 0, 0, major=5, minor=1)
    early["init"] = Member(stat.S_IFREG | 0o755, 0, 0, (PROFILE / "rootfs/alpine-root-init").read_bytes())
    cpio = pack_newc(directory, early)

    entries = add_overlay(entries, alpine_kernel.module_overlay({alpine_kernel.MODULE: modules[alpine_kernel.MODULE]}))
    entries["etc/inittab"] = Member(stat.S_IFREG | 0o644, 0, 0, (PROFILE / "rootfs/alpine-root-inittab").read_bytes())
    entries["opt/hesper/shutdown"] = Member(
        stat.S_IFREG | 0o755, 0, 0, (PROFILE / "rootfs/alpine-root-shutdown").read_bytes())
    seed = directory / SEED
    with tempfile.TemporaryDirectory(prefix="ext4-", dir=directory) as temp:
        temp = Path(temp)
        config = temp / "mke2fs.conf"
        config.write_text(f"[defaults]\n\tbase_features = {FEATURES}\n[fs_types]\n\text4 = {{\n\t\tfeatures = extent\n\t}}\n")
        archive = temp / "root.tar"
        seed_tar(archive, entries)
        first, second = temp / "first.raw", temp / "second.raw"
        make_seed(first, archive, config, env)
        make_seed(second, archive, config, env)
        if first.stat().st_size != SIZE or sha256(first) != sha256(second):
            raise ValueError("Alpine ext4 seed builds differ or have an unexpected size")
        check_filesystem(first, logs / "seed-e2fsck.log", env)
        first.replace(seed)
    return cpio, seed
