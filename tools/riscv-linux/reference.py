"""Cold boot the hash-locked guest under the pinned QEMU and verify serial I/O."""

import argparse
import json
import os
from pathlib import Path
import selectors
import shutil
import socket
import struct
import subprocess
import sys
import tempfile

from alpine_acceptance import cold_states as alpine_cold_states, states as alpine_states
from build_guest import (ALPINE_CANDIDATE_DIR, ALPINE_IMAGE_DIR, ALPINE_LOGS, BLK_IMAGE_DIR, BLK_LOGS,
                         DRAM, DRAM_END, ENV, FDT, FDT_GROWTH, IMAGE_DIR, INITRD, LOGS, block_tree,
                         capture, logged, verified_manifest)
from build_qemu import QEMU, version
from disk_acceptance import check_proof, drive_disk, make_seed
from serial_acceptance import STEP_SECONDS, STEP_BYTES, drive, session
from sources import require_tools, sha256


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


def cpu_readback(request, cpu, logs):
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
    (logs / "qemu-effective-cpu.json").write_text(json.dumps(effective, indent=2) + "\n")


def tree_snapshot(request, logs, name, address, length):
    buffer = logs / f"{name}.raw"
    request("pmemsave", val=address, size=length, filename=str(buffer))
    raw = buffer.read_bytes()
    if len(raw) < 8:
        raise ValueError(f"truncated {name} at {address:#x}")
    magic, size = struct.unpack_from(">II", raw)
    # QEMU packs the incoming tree before OpenSBI expands it for fixups.
    if magic != 0xd00dfeed or not 40 <= size <= len(raw):
        raise ValueError(f"invalid {name} at {address:#x}")
    dtb = logs / f"{name}.dtb"
    dtb.write_bytes(raw[:size])
    logged(f"{name}-dts", "dtc", "-I", "dtb", "-O", "dts", dtb, logs=logs)
    return dtb


def alpine_tree(dtb, manifest, phase):
    automatic = phase == "automatic"
    cpu = "/cpus/cpu@0"
    intc = f"{cpu}/interrupt-controller"
    plic = "/soc/plic@c000000" if automatic else "/soc/interrupt-controller@c000000"
    uart = "/soc/serial@10000000"
    clint = "/soc/clint@2000000"
    base, *extra = manifest["isa"].split("_")
    if automatic:
        # QEMU's pinned rv64 model also describes generic privileged/platform
        # extensions and the Zca/Zcd split of C. None are added to the board DTB.
        extra = ["zic64b", "ziccamoa", "ziccif", "zicclsm", "ziccrse", *extra,
                 "za64rs", "zca", "zcd", "ssccptr", "sscounterenw", "sstvala", "sstvecd"]
    cpu_phandle = capture("fdtget", "-t", "x", dtb, intc, "phandle")
    plic_phandle = capture("fdtget", "-t", "x", dtb, plic, "phandle")
    m_external = "ffffffff" if phase == "handoff" else "b"
    hart_nodes = [node for node in capture("fdtget", "-l", dtb, "/cpus").splitlines() if node.startswith("cpu@")]
    if hart_nodes != ["cpu@0"]:
        raise ValueError(f"{dtb}: unexpected harts: {hart_nodes}")
    properties = [
        ("s", "/chosen", "stdout-path", uart),
        ("x", "/memory@80000000", "reg", "0 80000000 0 8000000"),
        ("u", "/cpus", "timebase-frequency", "10000000"),
        ("x", cpu, "reg", "0"), ("s", cpu, "status", "okay"),
        ("s", cpu, "mmu-type", "riscv,sv39"),
        ("s", cpu, "riscv,isa-base", "rv64i"),
        ("s", cpu, "riscv,isa", "_".join([base, *extra])),
        ("s", cpu, "riscv,isa-extensions", " ".join([*base[4:], *extra])),
        ("s", intc, "compatible", "riscv,cpu-intc"),
        ("s", clint, "compatible", "sifive,clint0 riscv,clint0"),
        ("x", clint, "reg", "0 2000000 0 10000"),
        ("x", clint, "interrupts-extended", f"{cpu_phandle} 3 {cpu_phandle} 7"),
        ("s", plic, "compatible", "sifive,plic-1.0.0 riscv,plic0"),
        ("x", plic, "reg", "0 c000000 0 600000"),
        ("x", plic, "interrupts-extended", f"{cpu_phandle} {m_external} {cpu_phandle} 9"),
        ("u", plic, "riscv,ndev", "95" if automatic else "10"),
        ("s", uart, "compatible", "ns16550a"),
        ("x", uart, "reg", "0 10000000 0 100"),
        ("u", uart, "clock-frequency", "3686400"),
        ("x", uart, "interrupt-parent", plic_phandle),
        ("u", uart, "interrupts", "10"),
    ]
    if not automatic:
        soc = set(capture("fdtget", "-l", dtb, "/soc").splitlines())
        if soc != {node.rsplit("/", 1)[1] for node in (clint, plic, uart)}:
            raise ValueError(f"{dtb}: unexpected guest devices: {sorted(soc)}")
        properties.extend([
            ("s", "/", "compatible", "hesper,riscv-linux"),
            ("s", "/chosen", "bootargs", manifest["bootargs"]),
            ("x", "/chosen", "linux,initrd-start", f"0 {INITRD:x}"),
            ("x", "/chosen", "linux,initrd-end", f"0 {int(manifest['images'][2]['end'], 16):x}"),
            ("u", uart, "reg-shift", "0"),
            ("u", uart, "reg-io-width", "1"),
        ])
    report = {}
    for kind, node, prop, expected in properties:
        actual = capture("fdtget", "-t", kind, dtb, node, prop)
        if actual != expected:
            raise ValueError(f"{dtb}: {node} {prop}: expected {expected}, got {actual}")
        report[f"{node}:{prop}"] = actual
    return report


def startup_tree(request, manifest, logs, fdt_bytes):
    reset = logs / "qemu-reset-addrs.raw"
    # virt's RV64 reset ROM loads t0/a1 from these two consecutive dwords.
    request("pmemsave", val=0x1018, size=16, filename=str(reset))
    raw = reset.read_bytes()
    if len(raw) != 16:
        raise ValueError("truncated QEMU reset ROM address pair")
    firmware, address = struct.unpack("<QQ", raw)
    expected_fdt = DRAM_END - 0x200000  # QEMU places the packed DTB on a 2 MiB boundary.
    if firmware != DRAM or address != expected_fdt:
        raise ValueError(f"unexpected QEMU reset ROM firmware/DTB addresses: {firmware:#x}/{address:#x}")
    dtb = tree_snapshot(request, logs, "qemu-startup", address, fdt_bytes)
    return dict(firmware=f"{firmware:#x}", address=f"{address:#x}",
                properties=alpine_tree(dtb, manifest, "startup"))


def runtime_tree(request, manifest, logs, initrd_bytes, fdt_bytes):
    fw = manifest["images"][0]
    # OpenSBI copies the incoming tree to the locked firmware handoff address.
    request("stop")
    dtb = tree_snapshot(request, logs, "qemu-handoff", FDT, fdt_bytes + FDT_GROWTH)
    for key, expected in (("start", INITRD), ("end", INITRD + initrd_bytes)):
        actual = capture("fdtget", "-t", "x", dtb, "/chosen", f"linux,initrd-{key}")
        if actual != f"0 {expected:x}":
            raise ValueError(f"runtime initrd {key}: expected {expected:#x}, got {actual}")
    bootargs = capture("fdtget", "-t", "s", dtb, "/chosen", "bootargs")
    if bootargs != manifest["bootargs"]:
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
    return dict(initrd_start=f"{INITRD:#x}", initrd_end=f"{INITRD + initrd_bytes:#x}",
                bootargs=bootargs, firmware_no_map_end=f"{covered:#x}")


BUS = "virtio-mmio-bus.0"


def disk_tree(dtb, plic):
    phandle = capture("fdtget", "-t", "x", dtb, plic, "phandle")
    properties = {}
    for kind, node, prop, expected in block_tree(phandle):
        actual = capture("fdtget", "-t", kind, dtb, node, prop)
        if actual != expected:
            raise ValueError(f"{dtb}: {node} {prop}: expected {expected}, got {actual}")
        properties[prop] = actual
    return properties


def disk_wiring(request, logs, builtin_dtb):
    tree = request("human-monitor-command", **{"command-line": "info qtree"}).replace("\r\n", "\n")
    (logs / "qemu-qtree.txt").write_text(tree)
    bus = f"bus: {BUS}\n"
    if bus not in tree or 'dev: virtio-blk-device, id "data-disk"' not in tree.split(bus, 1)[1].split("bus:", 1)[0]:
        raise ValueError(f"data-disk is not on {BUS}; see qemu-qtree.txt")
    registers = request("human-monitor-command", **{"command-line": "xp /3wx 0x10001000"})
    if registers.partition(":")[2].split() != ["0x74726976", "0x00000002", "0x00000002"]:
        raise ValueError(f"{BUS}: unexpected MMIO identity {registers!r}")
    return dict(bus=BUS, mmio=registers.strip(),
                qemu_tree=disk_tree(builtin_dtb, "/soc/plic@c000000"))


def run(manifest, qemu_version, *, logs=LOGS, images=IMAGE_DIR, disk=None, write=False,
        replacement=None, states=None, alpine=False, step_seconds=STEP_SECONDS):
    if replacement is None:
        initramfs = images / "initramfs.cpio"
        fdt = images / "board.dtb"
        initrd_bytes, fdt_bytes = (image["bytes"] for image in manifest["images"][2:])
    else:
        initramfs, fdt = replacement
        initrd_bytes, fdt_bytes = initramfs.stat().st_size, fdt.stat().st_size
    logs.mkdir(parents=True, exist_ok=True)
    machine = "virt,aclint=off,aia=none"
    common = ["-accel", "tcg", "-cpu", manifest["qemu_cpu"], "-smp", "1", "-m", "128M", "-display", "none", "-monitor", "none"]
    builtin_dtb = logs / "qemu-default.dtb"
    logged("qemu-dumpdtb", QEMU, "-machine", f"{machine},dumpdtb={builtin_dtb}", *common,
           "-serial", "none", "-bios", "none", timeout=20, logs=logs)
    default_isa = capture("fdtget", "-t", "s", builtin_dtb, "/cpus/cpu@0", "riscv,isa")
    if default_isa.split("_")[0] != manifest["isa"].split("_")[0] or capture(
            "fdtget", "-t", "s", builtin_dtb, "/cpus/cpu@0", "mmu-type") != "riscv,sv39":
        raise ValueError(f"unexpected QEMU-generated CPU node: {default_isa}")
    automatic = alpine_tree(builtin_dtb, manifest, "automatic") if alpine else None
    with (logs / "qemu-serial.raw").open("wb") as output, (logs / "qemu-input.raw").open("wb") as inputs, (
            logs / "qemu-stderr.log").open("wb") as errors, selectors.DefaultSelector() as ready:
        host, child = socket.socketpair()
        with host, child:
            argv = [QEMU, "-machine", machine, *common, "-serial", "stdio", "-bios", images / "fw_jump.bin",
                    "-kernel", images / "Image", "-dtb", fdt, "-device",
                    f"loader,file={initramfs},addr={INITRD:#x},force-raw=on", "-S",
                    "-chardev", f"socket,id=hesper_qmp,fd={child.fileno()}", "-mon", "chardev=hesper_qmp,mode=control"]
            if disk is not None:
                argv += ["-global", "virtio-mmio.force-legacy=false",
                         "-blockdev", json.dumps(dict(driver="file", filename=str(disk), **{"node-name": "data-file"})),
                         "-blockdev", "driver=raw,file=data-file,node-name=data-raw",
                         "-device", f"virtio-blk-device,drive=data-raw,bus={BUS},id=data-disk"]
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
                    cpu_readback(request, manifest["qemu_cpu"], logs)
                    startup = startup_tree(request, manifest, logs, fdt_bytes) if alpine else None
                    wiring = disk_wiring(request, logs, builtin_dtb) if disk is not None else None
                    request("cont")
                    if states is not None:
                        timings = session(receive, send, states, STEP_SECONDS, step_seconds=step_seconds,
                                          allow_cursor_query=alpine)
                    elif disk is None:
                        timings = drive(receive, send, manifest["kernelrelease"], manifest["isa"])
                    else:
                        timings = drive_disk(receive, send, write, "DISK_OK", boot_seconds=STEP_SECONDS)
                    handoff = runtime_tree(request, manifest, logs, initrd_bytes, fdt_bytes)
                    if alpine:
                        handoff["address"] = f"{FDT:#x}"
                        handoff["properties"] = alpine_tree(logs / "qemu-handoff.dtb", manifest, "handoff")
                    if wiring is not None:
                        wiring["guest_tree"] = disk_tree(logs / "qemu-handoff.dtb", "/soc/interrupt-controller@c000000")
                        handoff["disk"] = wiring
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
                  serial_timing_seconds=timings, deadline_per_state_seconds=step_seconds,
                  byte_budget_per_state=STEP_BYTES, handoff=handoff, exit_reason="QMP quit", exit_status=guest.returncode)
    if alpine:
        report.update(boot_deadline_seconds=STEP_SECONDS, automatic_tree=automatic, startup_tree=startup)
    (logs / "qemu-reference.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def run_disk(manifest, qemu_version):
    require_tools("mke2fs", "debugfs")
    BLK_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="reference-", dir=BLK_LOGS))
    seed = logs / "seed.raw"
    working = logs / "guest.raw"
    info = make_seed(seed)
    shutil.copyfile(seed, working)
    first = run(manifest, qemu_version, logs=logs / "first", images=BLK_IMAGE_DIR, disk=working, write=True)
    after_write = check_proof(working, "DISK_OK")
    second = run(manifest, qemu_version, logs=logs / "second", images=BLK_IMAGE_DIR, disk=working)
    after_read = check_proof(working, "DISK_OK")
    if sha256(seed) != info["sha256"] or after_write == info["sha256"] or after_read != after_write:
        raise ValueError(f"unexpected disk changes; see {logs}")
    report = dict(manifest=manifest, seed=info, after_write_sha256=after_write, after_read_sha256=after_read,
                  first_boot=first, second_boot=second)
    (logs / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"QEMU wrote and read back the disk in two cold boots: {logs / 'report.json'}")


def run_alpine(manifest, qemu_version, candidate):
    ALPINE_LOGS.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="reference-candidate-" if candidate else "reference-", dir=ALPINE_LOGS))
    images = ALPINE_CANDIDATE_DIR if candidate else ALPINE_IMAGE_DIR
    first = run(manifest, qemu_version, logs=logs / "install", images=images, alpine=True,
                states=alpine_states(manifest), step_seconds=60)
    cold = run(manifest, qemu_version, logs=logs / "cold", images=images, alpine=True,
               states=alpine_cold_states(manifest), step_seconds=60)
    report = dict(manifest=manifest, candidate=candidate, install_boot=first, cold_boot=cold)
    report_path = logs / "report.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(f"QEMU booted Alpine, installed and ran signed mawk offline, then verified a clean cold boot: {report_path}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=("serial", "blk", "alpine"), default="serial")
    parser.add_argument("--candidate", action="store_true", help="qualify the unlocked Alpine candidate")
    args = parser.parse_args()
    if args.candidate and args.profile != "alpine":
        parser.error("--candidate requires --profile alpine")
    require_tools("dtc", "fdtget")
    if not QEMU.is_file():
        raise FileNotFoundError(f"{QEMU}: run make riscv-linux-qemu first")
    qemu_version = version()
    blk = args.profile == "blk"
    alpine = args.profile == "alpine"
    manifest = verified_manifest(blk, alpine=alpine, candidate=args.candidate)
    if alpine:
        run_alpine(manifest, qemu_version, args.candidate)
    elif blk:
        run_disk(manifest, qemu_version)
    else:
        run(manifest, qemu_version)
        print(f"QEMU cold boot and three serial commands passed: {LOGS / 'qemu-reference.json'}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        sys.exit(f"error: {error}")
