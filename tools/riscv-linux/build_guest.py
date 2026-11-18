"""Build the pinned Linux guests and verify their images before recording a manifest."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import time

import alpine_root
from sources import CACHE, LOCK, PROFILE, archives, check_archive, require_tools, sha256

DRAM = 0x80000000
KERNEL = 0x80200000
INITRD = 0x84000000
FDT = 0x87800000
DRAM_END = 0x88000000
BOOTARGS = "console=ttyS0 rdinit=/init"
ISA = "rv64imac_zicntr_zicsr_zifencei"
# Binutils 2.40 encodes the counter CSRs but predates the Zicntr ISA attribute.
FW_ISA = "rv64imac_zicsr_zifencei"
FDT_GROWTH = 0x10000  # Space beyond totalsize for OpenSBI's in-place fixups.
IMAGE_NAMES = ("fw_jump.bin", "Image", "initramfs.cpio", "board.dtb")
IMAGE_LOCK = PROFILE / "images.lock.json"
IMAGE_DIR = CACHE / "images"
BLK_IMAGE_LOCK = PROFILE / "images-blk.lock.json"
BLK_IMAGE_DIR = CACHE / "images-blk"
BLK_LOGS = CACHE / "logs-blk"
CPU_PROFILE = PROFILE / "qemu-cpu.txt"
LOGS = CACHE / "logs"
ALPINE_ISA = "rv64imafdc_zicntr_zicsr_zifencei"
ALPINE_CPU_PROFILE = PROFILE / "qemu-cpu-alpine.txt"
ALPINE_IMAGE_LOCK = PROFILE / "images-alpine.lock.json"
ALPINE_IMAGE_DIR = CACHE / "images-alpine"
ALPINE_CANDIDATE_DIR = CACHE / "images-alpine-candidate"
ALPINE_LOGS = CACHE / "logs-alpine"
ENV = {key: value for key, value in os.environ.items() if key not in ("MAKEFLAGS", "MFLAGS", "MAKEOVERRIDES")}
ENV.update(LC_ALL="C", TZ="UTC", GIT_CEILING_DIRECTORIES=str(CACHE))


def command(*args, env_override=ENV, **kwargs):
    return subprocess.run(args, check=True, env=env_override, **kwargs)


def capture(*args, **kwargs):
    return command(*args, stdout=subprocess.PIPE, text=True, **kwargs).stdout.strip()


def logged(name, *args, logs=LOGS, **kwargs):
    log = logs / f"{name}.log"
    print(f"{name}: {log}", flush=True)
    with log.open("w") as stream:
        command(*args, stdout=stream, stderr=subprocess.STDOUT, **kwargs)


def unpack(archive):
    # Version and digest are already validated by archives(). Extract from the
    # pinned tarball, not a surrounding git checkout or an installed SDK.
    root = archive.name.split(".tar.")[0]
    parent = CACHE / "build" / "sources" / sha256(archive)
    source = parent / root
    if not source.exists():
        parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=parent) as temp:
            command("tar", "--no-same-owner", "--no-same-permissions", "-xf", archive, "-C", temp)
            (Path(temp) / root).rename(source)
    return source


def configs(path):
    return dict(line.split("=", 1) for line in path.read_text().splitlines() if line and not line.startswith("#"))


def check_config(actual, requested, extra):
    wanted = {}
    for fragment in requested:
        wanted.update(configs(fragment))
        for key in re.findall(r"^# (\w+) is not set$", fragment.read_text(), re.MULTILINE):
            wanted[key] = "n"
    wanted.update(extra)
    found = configs(actual)
    for key, expected in wanted.items():
        value = found.get(key, "n")
        if value != expected:
            raise ValueError(f"{actual}: {key}: expected {expected}, got {value}")


def check_elf(path, readelf, name, logs):
    report = capture(readelf, "-h", "-A", "-lW", path)
    (logs / f"{name}-elf.txt").write_text(report + "\n")
    if any(field not in report for field in ("ELF64", "little endian", "RISC-V", "soft-float ABI")):
        raise ValueError(f"{path}: expected little-endian ELF64/RISC-V with lp64 ABI")

    if re.search(r"^\s*INTERP\s", report, re.MULTILINE):
        raise ValueError(f"{path}: unexpected program interpreter")
    # GCC's ELF attributes list Zmmul separately even when M already covers it.
    arch = re.search(r'Tag_RISCV_arch:\s+"(rv64[^"]+)"', report)
    if not arch or not re.fullmatch(r"rv64i\d+p\d+(?:_(?:m|a|c|zicsr|zifencei|zicntr|zmmul)\d+p\d+)+", arch[1]):
        raise ValueError(f"{path}: unsupported ISA attributes; see {name}-elf.txt")
    return report


def check_cpio(path, blk):
    entries = {}
    with path.open("rb") as stream:
        while True:
            header = stream.read(110)
            if len(header) != 110 or header[:6] != b"070701":
                raise ValueError(f"{path}: not an uncompressed newc archive")
            fields = [int(header[i:i + 8], 16) for i in range(6, 110, 8)]
            size, name_size = fields[6], fields[11]
            name = stream.read(name_size).rstrip(b"\0").decode().removeprefix("./")
            if name == "TRAILER!!!":
                break
            stream.seek(-stream.tell() % 4, os.SEEK_CUR)
            mode = fields[1]
            payload = stream.read(size) if name == "init" or stat.S_ISLNK(mode) else None
            if payload is None:
                stream.seek(size, os.SEEK_CUR)
            stream.seek(-stream.tell() % 4, os.SEEK_CUR)
            entries[name] = (mode, fields[9], fields[10], payload)
    init = entries.get("init")
    console = entries.get("dev/console")
    shell = entries.get("bin/sh")
    if not init or init[0] != stat.S_IFREG | 0o755 or init[3] != (PROFILE / "rootfs/init").read_bytes():
        raise ValueError(f"{path}: missing 0755 /init with the locked overlay content")
    if not console or not stat.S_ISCHR(console[0]) or console[1:3] != (5, 1):
        raise ValueError(f"{path}: /dev/console is not character device 5:1")
    if (not shell or not stat.S_ISLNK(shell[0]) or shell[3] not in (b"busybox", b"/bin/busybox")
            or "bin/busybox" not in entries):
        raise ValueError(f"{path}: /bin/sh does not link to BusyBox")
    applets = ("mount", "uname", "cat", "printf") + (("umount", "sync") if blk else ())
    for applet in applets:
        if not any(name in entries for name in (f"bin/{applet}", f"usr/bin/{applet}")):
            raise ValueError(f"{path}: missing {applet} applet")


def guest_compiler(out):
    gccs = list((out / "host/bin").glob("riscv64-*-linux-musl-gcc"))
    if len(gccs) != 1:
        raise ValueError(f"{out}: expected exactly one Buildroot RISC-V musl compiler, found {gccs}")
    return gccs[0]


def rootfs(source, out, jobs, logs, blk):
    config = PROFILE / "buildroot.defconfig"
    make = ("make", "-C", source, f"O={out}", f"BR2_DEFCONFIG={config}",
            "SOURCE_DATE_EPOCH=1742837000",
            f"HESPER_RISCV_PROFILE={PROFILE}", f"BR2_DL_DIR={CACHE / 'src/buildroot-dl'}", f"BR2_JLEVEL={jobs}")
    logged("buildroot-config", *make, "defconfig", logs=logs)
    check_config(out / ".config", (config,), {"BR2_RISCV_ISA_RVI": "y", "BR2_GCC_TARGET_ABI": '"lp64"',
                                           "BR2_RISCV_ISA_RVF": "n", "BR2_RISCV_ISA_RVD": "n", "BR2_RISCV_ISA_RVV": "n"})
    logged("buildroot", *make, logs=logs)
    gcc = guest_compiler(out)
    prefix = str(gcc).removesuffix("gcc")
    compiler = capture(gcc, "--version").splitlines()[0]
    check_elf(out / "target/bin/busybox", prefix + "readelf", "busybox", logs)
    cpio = out / "images/rootfs.cpio"
    with cpio.open("rb") as stream:
        logged("initramfs-list", "cpio", "-itv", stdin=stream, logs=logs)
    check_cpio(cpio, blk)
    return cpio, prefix, compiler


def firmware(source, out, prefix, jobs, logs):
    logged("opensbi", "make", "-C", source, f"O={out}", f"-j{jobs}", f"CROSS_COMPILE={prefix}",
           "PLATFORM=generic", "PLATFORM_RISCV_XLEN=64", f"PLATFORM_RISCV_ISA={FW_ISA}",
           "PLATFORM_RISCV_ABI=lp64", f"FW_TEXT_START={DRAM:#x}", "FW_JUMP=y", f"FW_JUMP_ADDR={KERNEL:#x}",
           f"FW_JUMP_FDT_ADDR={FDT:#x}", "OPENSBI_VERSION_GIT=", "BUILD_INFO=n", "V=1", logs=logs)
    board = out / "platform/generic/firmware"
    binary = board / "fw_jump.bin"
    elf = board / "fw_jump.elf"
    report = check_elf(elf, prefix + "readelf", "opensbi", logs)
    segments = re.findall(r"^\s*LOAD\s+\S+\s+\S+\s+(0x[0-9a-f]+)\s+\S+\s+(0x[0-9a-f]+)", report, re.MULTILINE)
    if not segments or any(int(base, 16) < DRAM for base, _ in segments):
        raise ValueError(f"{elf}: LOAD segment below DRAM or no LOAD segments")
    symbols = capture(prefix + "nm", "-n", elf)
    (logs / "opensbi-symbols.txt").write_text(symbols + "\n")
    end = re.search(r"^([0-9a-f]+)\s+\w\s+_fw_end$", symbols, re.MULTILINE)
    if not end:
        raise ValueError(f"{elf}: missing _fw_end")
    entry = re.search(r"Entry point address:\s+(0x[0-9a-f]+)", report)
    if not entry or int(entry[1], 16) != DRAM:
        raise ValueError(f"{elf}: firmware entry is not {DRAM:#x}")
    # v1.6 fw_base.S places the one hart's 8 KiB stack/scratch and the
    # generic heap after _fw_end; 64 KiB bounds its 0x9000 + TLB FIFO heap.
    resident = max(int(end[1], 16) + 0x2000 + 0x10000,
                   max(int(base, 16) + int(size, 16) for base, size in segments),
                   DRAM + binary.stat().st_size)
    return binary, resident


def kernel_fragments(blk, alpine):
    fragments = [PROFILE / "linux.fragment"]
    if blk:
        fragments.append(PROFILE / "linux-blk.fragment")
    if alpine:
        fragments.append(PROFILE / "linux-alpine.fragment")
    return fragments


KERNEL_OPTIONS = {"CONFIG_OF": "y", "CONFIG_RISCV_M_MODE": "n"}


def kernel(source, out, prefix, jobs, logs, blk, *, alpine=False):
    env = ENV | dict(ARCH="riscv", CROSS_COMPILE=prefix, SOURCE_DATE_EPOCH="1731888000",
                     KBUILD_BUILD_TIMESTAMP="Mon Nov 18 00:00:00 UTC 2024",
                     KBUILD_BUILD_USER="hesper", KBUILD_BUILD_HOST="reference", KBUILD_BUILD_VERSION="1",
                     LOCALVERSION="")
    make = ("make", "-C", source, f"O={out}", f"-j{jobs}")
    fragments = kernel_fragments(blk, alpine)
    logged("linux-defconfig", *make, "defconfig", env_override=env, logs=logs)
    logged("linux-fragment", source / "scripts/kconfig/merge_config.sh", "-m", "-O", out,
           out / ".config", *fragments, cwd=source, env_override=env, logs=logs)
    logged("linux-olddefconfig", *make, "olddefconfig", env_override=env, logs=logs)
    check_config(out / ".config", fragments, KERNEL_OPTIONS)
    logged("linux-build", *make, "Image", env_override=env, logs=logs)
    release = capture(*make, "-s", "kernelrelease", env_override=env)
    image = out / "arch/riscv/boot/Image"
    return image, kernel_end(image), release


def kernel_end(image):
    with image.open("rb") as stream:
        header = stream.read(64)
    if len(header) != 64:
        raise ValueError(f"{image}: truncated RISC-V Image header")
    offset, size, flags, version = struct.unpack_from("<QQQI", header, 8)
    if header[56:60] != b"RSC\x05" or offset != KERNEL - DRAM or size == 0 or flags != 0 or version != 2:
        raise ValueError(f"{image}: invalid RISC-V Image header for this board")
    return KERNEL + max(size, image.stat().st_size)


def board_dtb(directory, initrd_end, logs, blk, *, alpine=False):
    dts = directory / "board.dts"
    dtb = directory / "board.dtb"
    source = (PROFILE / "board.dts").read_text().replace("@INITRD_END@", f"{initrd_end:#x}")
    if blk:
        source += (PROFILE / "board-blk.dtsi").read_text()
    if alpine:
        source += (PROFILE / "board-alpine.dtsi").read_text()
    dts.write_text(source)
    logged("dtc", "dtc", "-I", "dts", "-O", "dtb", "-p", "4096", "-o", dtb, dts, logs=logs)
    intc = "/cpus/cpu@0/interrupt-controller"
    plic = "/soc/interrupt-controller@c000000"
    uart = "/soc/serial@10000000"
    clint = "/soc/clint@2000000"
    cpu_phandle = capture("fdtget", "-t", "x", dtb, intc, "phandle")
    plic_phandle = capture("fdtget", "-t", "x", dtb, plic, "phandle")
    expected = [
        ("s", "/chosen", "bootargs", BOOTARGS), ("s", "/chosen", "stdout-path", uart),
        ("x", "/chosen", "linux,initrd-start", f"0 {INITRD:x}"),
        ("x", "/chosen", "linux,initrd-end", f"0 {initrd_end:x}"),
        ("x", "/memory@80000000", "reg", "0 80000000 0 8000000"),
        ("u", "/cpus", "timebase-frequency", "10000000"),
        ("x", "/cpus/cpu@0", "reg", "0"), ("s", "/cpus/cpu@0", "mmu-type", "riscv,sv39"),
        ("s", "/cpus/cpu@0", "riscv,isa", ALPINE_ISA if alpine else ISA),
        ("s", "/cpus/cpu@0", "riscv,isa-extensions",
         "i m a f d c zicntr zicsr zifencei" if alpine else "i m a c zicntr zicsr zifencei"),
        ("x", clint, "reg", "0 2000000 0 10000"),
        ("x", clint, "interrupts-extended", f"{cpu_phandle} 3 {cpu_phandle} 7"),
        ("x", plic, "reg", "0 c000000 0 600000"),
        ("x", plic, "interrupts-extended", f"{cpu_phandle} b {cpu_phandle} 9"),
        ("x", uart, "reg", "0 10000000 0 100"), ("u", uart, "clock-frequency", "3686400"),
        ("s", uart, "compatible", "ns16550a"), ("x", uart, "interrupt-parent", plic_phandle),
        ("u", uart, "interrupts", "10"), ("u", uart, "reg-shift", "0"), ("u", uart, "reg-io-width", "1"),
    ]
    if blk:
        expected.extend(block_tree(plic_phandle))
    if capture("fdtget", "-l", dtb, "/cpus") != "cpu@0":
        raise ValueError(f"{dtb}: expected a single CPU node")
    if capture("fdtget", "-l", dtb, "/reserved-memory"):
        raise ValueError(f"{dtb}: only OpenSBI may populate reserved-memory")
    with (logs / "fdt-readback.txt").open("w") as report:
        for kind, node, prop, value in expected:
            found = capture("fdtget", "-t", kind, dtb, node, prop)
            print(node, prop, found, file=report)
            if found != value:
                raise ValueError(f"{dtb}: {node} {prop}: expected {value}, got {found}")
    with dtb.open("rb") as stream:
        magic, total = struct.unpack(">II", stream.read(8))
    if magic != 0xd00dfeed or total != dtb.stat().st_size:
        raise ValueError(f"{dtb}: wrong FDT magic or totalsize")
    return dtb, FDT + total + FDT_GROWTH


def block_tree(plic_phandle):
    node = "/soc/virtio_mmio@10001000"
    return [("s", node, "compatible", "virtio,mmio"), ("x", node, "reg", "0 10001000 0 1000"),
            ("x", node, "interrupt-parent", plic_phandle), ("u", node, "interrupts", "1")]


def verified_manifest(blk=False, *, directory=None, alpine=False, candidate=False):
    if (alpine and blk) or (candidate and not alpine):
        raise ValueError("candidate verification requires the Alpine serial board")
    if alpine:
        lock, default_dir = ALPINE_IMAGE_LOCK, ALPINE_CANDIDATE_DIR if candidate else ALPINE_IMAGE_DIR
    else:
        lock, default_dir = (BLK_IMAGE_LOCK, BLK_IMAGE_DIR) if blk else (IMAGE_LOCK, IMAGE_DIR)
    directory = default_dir if directory is None else directory
    manifest = json.loads((directory / "manifest.json").read_bytes())
    if (not candidate and manifest != json.loads(lock.read_bytes())) or sha256(LOCK) != manifest["source_lock_sha256"]:
        raise ValueError("prepared manifest differs from the source or image lock")
    cpu_profile, isa = (ALPINE_CPU_PROFILE, ALPINE_ISA) if alpine else (CPU_PROFILE, ISA)
    if manifest["qemu_cpu"] != cpu_profile.read_text().strip() or manifest["isa"] != isa:
        raise ValueError("prepared manifest differs from the CPU profile")
    if manifest["bootargs"] != BOOTARGS:
        raise ValueError("prepared manifest differs from the boot arguments")
    if [image["path"] for image in manifest["images"]] != list(IMAGE_NAMES):
        raise ValueError("expected the four fixed image names in the image lock")
    for image in manifest["images"]:
        path = directory / image["path"]
        if path.stat().st_size != image["bytes"]:
            raise ValueError(f"{path}: size mismatch")
        check_archive(path, image["sha256"])
    if alpine:
        check_alpine_manifest(manifest, directory, candidate)
    return manifest


def check_alpine_manifest(manifest, directory, candidate):
    serial = json.loads(IMAGE_LOCK.read_bytes())
    if (manifest["alpine_source_lock_sha256"] != sha256(alpine_root.ALPINE_SOURCE_LOCK)
            or manifest["compiler"] != serial["compiler"] or manifest["kernelrelease"] != serial["kernelrelease"]
            or manifest["config_sha256"]["buildroot"] != serial["config_sha256"]["buildroot"]
            or not re.fullmatch(r"[0-9a-f]{64}", manifest["config_sha256"]["linux"])
            or manifest["images"][0] != serial["images"][0]):
        raise ValueError("Alpine manifest differs from its source lock, toolchain or serial firmware")
    if candidate:
        # This path is only for reference qualification, never a consumer boot lock.
        archives()
        alpine_root.inputs()
        config = guest_build_dir(alpine=True) / "linux/.config"
        check_config(config, kernel_fragments(False, True), KERNEL_OPTIONS)
        if manifest["config_sha256"]["linux"] != sha256(config):
            raise ValueError("Alpine candidate differs from its effective Linux config")
        if manifest["compiler"] != capture(guest_compiler(guest_build_dir() / "buildroot"), "--version").splitlines()[0]:
            raise ValueError("Alpine candidate differs from the serial guest compiler")
    spans = [(KERNEL, kernel_end(directory / "Image"), INITRD),
             (INITRD, INITRD + (directory / "initramfs.cpio").stat().st_size, FDT),
             (FDT, FDT + (directory / "board.dtb").stat().st_size + FDT_GROWTH, DRAM_END)]
    for artifact, (base, end, limit) in zip(manifest["images"][1:], spans):
        if artifact["address"] != f"{base:#x}" or artifact["end"] != f"{end:#x}" or not base < end <= limit:
            raise ValueError(f"{artifact['path']}: invalid Alpine physical span")


def guest_build_dir(blk=False, *, alpine=False):
    inputs = [LOCK, PROFILE / "buildroot.defconfig", PROFILE / "linux.fragment", PROFILE / "rootfs/init",
              PROFILE / "board.dts"]
    if blk:
        inputs.extend((PROFILE / "linux-blk.fragment", PROFILE / "board-blk.dtsi"))
    if alpine:
        inputs.extend((alpine_root.ALPINE_SOURCE_LOCK, ALPINE_CPU_PROFILE, PROFILE / "linux-alpine.fragment",
                       PROFILE / "board-alpine.dtsi"))
    stamp = hashlib.sha256("".join(sha256(path) for path in inputs).encode()).hexdigest()[:16]
    return CACHE / "build" / "guest" / stamp


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("serial", "blk", "alpine"), default="serial")
    parser.add_argument("--candidate", action="store_true", help="write an unlocked blk or Alpine build")
    args = parser.parse_args()
    blk, alpine = args.profile == "blk", args.profile == "alpine"
    if args.candidate and not (blk or alpine):
        parser.error("--candidate requires --profile blk or alpine")
    if alpine:
        logs, lock = ALPINE_LOGS, ALPINE_IMAGE_LOCK
        output = ALPINE_CANDIDATE_DIR if args.candidate else ALPINE_IMAGE_DIR
    else:
        logs = BLK_LOGS if blk else LOGS
        lock, output = (BLK_IMAGE_LOCK, BLK_IMAGE_DIR) if blk else (IMAGE_LOCK, IMAGE_DIR)
        if args.candidate:
            output = CACHE / "images-blk-candidate"
    require_tools("make", "gcc", "g++", "git", "perl", "tar", "wget", "cpio", "unzip", "rsync", "patch",
                  "file", "bzip2", "xz", "bc", "bison", "flex", "dtc", "fdtget")
    known = archives()
    jobs = int(os.environ.get("RISCV_LINUX_JOBS", len(os.sched_getaffinity(0))))
    if jobs < 1 or os.geteuid() == 0:
        raise ValueError("run as a non-root user and set RISCV_LINUX_JOBS to a positive count")
    start = time.monotonic()
    logs.mkdir(parents=True, exist_ok=True)
    build = guest_build_dir(blk, alpine=alpine)
    build.mkdir(parents=True, exist_ok=True)
    k_out = build / "linux"
    if alpine:
        serial = verified_manifest()
        br_out = guest_build_dir() / "buildroot"
        gcc = guest_compiler(br_out)
        prefix = str(gcc).removesuffix("gcc")
        compiler = capture(gcc, "--version").splitlines()[0]
        if compiler != serial["compiler"]:
            raise ValueError(f"{gcc}: compiler differs from serial image lock")
        fw, fw_end = IMAGE_DIR / "fw_jump.bin", int(serial["images"][0]["end"], 16)
        cpio = alpine_root.build_root(build, prefix, alpine_root.inputs(), logs)
    else:
        br_out = build / "buildroot"
        cpio, prefix, compiler = rootfs(unpack(known["buildroot"]), br_out, jobs, logs, blk)
        fw, fw_end = firmware(unpack(known["opensbi"]), build / "opensbi", prefix, jobs, logs)
    image, image_end, release = kernel(unpack(known["linux"]), k_out, prefix, jobs, logs, blk, alpine=alpine)
    rd_end = INITRD + cpio.stat().st_size
    ranges = [(DRAM, fw_end), (KERNEL, image_end), (INITRD, rd_end)]
    for index, ((base, end), limit) in enumerate(zip(ranges, (KERNEL, INITRD, FDT))):
        if not base < end <= limit:
            raise ValueError(f"{IMAGE_NAMES[index]}: invalid or overlapping physical span [{base:#x}, {end:#x})")
    dtb, dtb_end = board_dtb(build, rd_end, logs, blk, alpine=alpine)
    if dtb_end > DRAM_END:
        raise ValueError(f"{dtb}: FDT growth exceeds DRAM")
    ranges.append((FDT, dtb_end))
    artifacts = []
    paths = [fw, image, cpio, dtb]
    for name, path, (base, end) in zip(IMAGE_NAMES, paths, ranges):
        artifacts.append(dict(path=name, bytes=path.stat().st_size, sha256=sha256(path),
                              address=f"{base:#x}", end=f"{end:#x}"))
    # Kconfig stores the absolute filename used to seed Buildroot; it is not
    # an effective guest option and must not tie the lock to a checkout path.
    buildroot_config = re.sub(rb"^BR2_DEFCONFIG=.*\n", b"", (br_out / ".config").read_bytes(), flags=re.M)
    manifest = dict(source_lock_sha256=sha256(LOCK), bootargs=BOOTARGS, isa=ALPINE_ISA if alpine else ISA,
                    kernelrelease=release,
                    compiler=compiler, config_sha256={"buildroot": hashlib.sha256(buildroot_config).hexdigest(),
                                                      "linux": sha256(k_out / ".config")}, images=artifacts,
                    qemu_cpu=(ALPINE_CPU_PROFILE if alpine else CPU_PROFILE).read_text().strip())
    if alpine:
        manifest["alpine_source_lock_sha256"] = sha256(alpine_root.ALPINE_SOURCE_LOCK)
        if manifest["config_sha256"]["buildroot"] != serial["config_sha256"]["buildroot"]:
            raise ValueError("Buildroot toolchain config differs from serial image lock")
    if not args.candidate:
        expected = json.loads(lock.read_bytes())
        if manifest != expected:
            differing = [key for key in manifest.keys() | expected.keys() if manifest.get(key) != expected.get(key)]
            raise ValueError(f"{lock}: rebuilt manifest differs in {', '.join(sorted(differing))}; "
                             f"observed {json.dumps({key: manifest.get(key) for key in differing})}")
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output) as temp:
        staging = Path(temp)
        for name, path in zip(IMAGE_NAMES, paths):
            shutil.copyfile(path, staging / name)
        for name in IMAGE_NAMES:
            (staging / name).replace(output / name)
        (staging / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        (staging / "manifest.json").replace(output / "manifest.json")
    kind = "unlocked candidate" if args.candidate else "verified"
    print(f"{kind}: four guest images in {output} ({time.monotonic() - start:.1f} s)")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"error: {error}")
