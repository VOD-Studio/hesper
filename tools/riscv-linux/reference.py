"""Cold boot the hash-locked guest under the pinned QEMU and verify serial I/O."""

import json
import os
import selectors
import socket
import struct
import subprocess
import sys

from build_guest import (
    CACHE, CPU_PROFILE, DRAM, ENV, FDT, FDT_GROWTH, IMAGE_LOCK, IMAGE_NAMES, INITRD, ISA, LOCK, LOGS, capture, logged,
)
from build_qemu import QEMU, version
from serial_acceptance import STEP_SECONDS, STEP_BYTES, drive
from sources import check_archive, require_tools, sha256

IMAGE_DIR = CACHE / "images"


def verified_manifest():
    manifest = json.loads(IMAGE_LOCK.read_bytes())
    actual = json.loads((IMAGE_DIR / "manifest.json").read_bytes())
    if actual != manifest or sha256(LOCK) != manifest["source_lock_sha256"]:
        raise ValueError("prepared manifest differs from the source or image lock")
    cpu = CPU_PROFILE.read_text().strip()
    if manifest["qemu_cpu"] != cpu or manifest["isa"] != ISA:
        raise ValueError("prepared manifest differs from the CPU profile")
    if [image["path"] for image in manifest["images"]] != list(IMAGE_NAMES):
        raise ValueError("expected the four fixed image names in the image lock")
    for image in manifest["images"]:
        path = IMAGE_DIR / image["path"]
        if path.stat().st_size != image["bytes"]:
            raise ValueError(f"{path}: size mismatch")
        check_archive(path, image["sha256"])
    return manifest


def qmp(channel):
    channel.settimeout(5)
    wire = channel.makefile("rb")
    hello = json.loads(wire.readline())
    if "QMP" not in hello:
        raise ValueError("missing QMP greeting")

    def request(operation, **arguments):
        channel.sendall(json.dumps(dict(execute=operation, arguments=arguments)).encode("ascii") + b"\n")
        while True:
            reply = json.loads(wire.readline())
            if "return" in reply:
                return reply["return"]
            if "error" in reply:
                raise ValueError(f"QMP {operation}: {reply['error']}")

    request("qmp_capabilities")
    return request


def cpu_readback(request, cpu):
    cores = request("query-cpus-fast")
    if len(cores) != 1 or cores[0]["cpu-index"] != 0:
        raise ValueError("expected exactly one QEMU hart at index 0")
    effective = {}
    for field in cpu.split(",")[1:]:
        prop, raw = field.split("=", 1)
        expected = {"on": True, "off": False, "0": 0}.get(raw, raw)
        value = request("qom-get", path=cores[0]["qom-path"], property=prop)
        if value != expected:
            raise ValueError(f"QEMU CPU {prop}: expected {expected}, got {value}")
        effective[prop] = value
    (LOGS / "qemu-effective-cpu.json").write_text(json.dumps(effective, indent=2) + "\n")


def runtime_tree(request, manifest):
    fw, _, initrd, fdt = manifest["images"]
    buffer = LOGS / "qemu-handoff.raw"
    request("stop")
    request("pmemsave", val=FDT, size=fdt["bytes"] + FDT_GROWTH, filename=str(buffer))
    raw = buffer.read_bytes()
    magic, size = struct.unpack_from(">II", raw)
    # QEMU packs the incoming tree before OpenSBI expands it for fixups.
    if magic != 0xd00dfeed or not 40 <= size <= len(raw):
        raise ValueError(f"invalid runtime FDT at {FDT:#x}")
    dtb = LOGS / "qemu-handoff.dtb"
    dtb.write_bytes(raw[:size])
    logged("qemu-handoff-dts", "dtc", "-I", "dtb", "-O", "dts", dtb)
    for key, expected in (("start", INITRD), ("end", INITRD + initrd["bytes"])):
        actual = capture("fdtget", "-t", "x", dtb, "/chosen", f"linux,initrd-{key}")
        if actual != f"0 {expected:x}":
            raise ValueError(f"runtime initrd {key}: expected {expected:#x}, got {actual}")
    if capture("fdtget", "-t", "s", dtb, "/chosen", "bootargs") != manifest["bootargs"]:
        raise ValueError("runtime bootargs differ from the image lock")
    covered = DRAM
    ranges = []
    for child in capture("fdtget", "-l", dtb, "/reserved-memory").splitlines():
        node = f"/reserved-memory/{child}"
        if "no-map" in capture("fdtget", "-p", dtb, node).splitlines():
            hi, lo, len_hi, len_lo = (int(cell, 16) for cell in capture("fdtget", "-t", "x", dtb, node, "reg").split())
            base = (hi << 32) | lo
            ranges.append((base, base + ((len_hi << 32) | len_lo)))
    for start, end in sorted(ranges):
        if start <= covered < end:
            covered = end
    if covered < int(fw["end"], 16):
        raise ValueError(f"runtime FDT no-map coverage ends too early at {covered:#x}")
    return dict(initrd_start=f"{INITRD:#x}", initrd_end=f"{INITRD + initrd['bytes']:#x}", firmware_no_map_end=f"{covered:#x}")


def run(manifest, qemu_version):
    machine = "virt,aclint=off,aia=none"
    common = ["-accel", "tcg", "-cpu", manifest["qemu_cpu"], "-smp", "1", "-m", "128M", "-display", "none", "-monitor", "none"]
    builtin_dtb = LOGS / "qemu-default.dtb"
    logged("qemu-dumpdtb", QEMU, "-machine", f"{machine},dumpdtb={builtin_dtb}", *common,
           "-serial", "none", "-bios", "none", timeout=20)
    default_isa = capture("fdtget", "-t", "s", builtin_dtb, "/cpus/cpu@0", "riscv,isa")
    if default_isa.split("_")[0] != "rv64imac" or capture(
            "fdtget", "-t", "s", builtin_dtb, "/cpus/cpu@0", "mmu-type") != "riscv,sv39":
        raise ValueError(f"unexpected QEMU-generated CPU node: {default_isa}")
    with (LOGS / "qemu-serial.raw").open("wb") as output, (LOGS / "qemu-input.raw").open("wb") as inputs, (
            LOGS / "qemu-stderr.log").open("wb") as errors, selectors.DefaultSelector() as ready:
        host, child = socket.socketpair()
        with host, child:
            argv = [QEMU, "-machine", machine, *common, "-serial", "stdio", "-bios", IMAGE_DIR / "fw_jump.bin",
                    "-kernel", IMAGE_DIR / "Image", "-dtb", IMAGE_DIR / "board.dtb", "-device",
                    f"loader,file={IMAGE_DIR / 'initramfs.cpio'},addr={INITRD:#x},force-raw=on", "-S",
                    "-chardev", f"socket,id=hesper_qmp,fd={child.fileno()}", "-mon", "chardev=hesper_qmp,mode=control"]
            with subprocess.Popen(argv, env=ENV, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors,
                                  pass_fds=(child.fileno(),)) as guest:
                child.close()
                ready.register(guest.stdout, selectors.EVENT_READ)
                def receive(seconds):
                    if not ready.select(seconds):
                        raise TimeoutError("no prompt before deadline")
                    data = os.read(guest.stdout.fileno(), 8192)
                    output.write(data)
                    return data

                def send(command):
                    inputs.write(command)
                    guest.stdin.write(command)
                    guest.stdin.flush()

                try:
                    request = qmp(host)
                    cpu_readback(request, manifest["qemu_cpu"])
                    request("cont")
                    timings = drive(receive, send, manifest["kernelrelease"], manifest["isa"])
                    handoff = runtime_tree(request, manifest)
                    request("quit")
                    guest.wait(timeout=5)
                    if guest.returncode != 0:
                        raise ValueError(f"QEMU exited with status {guest.returncode}; see qemu-stderr.log")
                finally:
                    if guest.poll() is None:
                        guest.kill()
                        guest.wait(timeout=5)
    report = dict(qemu=qemu_version, qemu_sha256=sha256(QEMU), default_isa=default_isa,
                  kernelrelease=manifest["kernelrelease"],
                  serial_timing_seconds=timings, deadline_per_state_seconds=STEP_SECONDS,
                  byte_budget_per_state=STEP_BYTES, handoff=handoff)
    (LOGS / "qemu-reference.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"QEMU cold boot and three serial commands passed: {LOGS / 'qemu-reference.json'}")


def main():
    require_tools("dtc", "fdtget")
    if not QEMU.is_file():
        raise FileNotFoundError(f"{QEMU}: run make riscv-linux-qemu first")
    qemu_version = version()
    manifest = verified_manifest()
    LOGS.mkdir(parents=True, exist_ok=True)
    run(manifest, qemu_version)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        sys.exit(f"error: {error}")
