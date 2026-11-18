"""Assemble fixed ISA probes and observe the real QEMU TCG hart."""

import argparse
import json
import socket
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from build_guest import ALPINE_LOGS, CPU_PROFILE, DRAM, ENV, LOGS, PROFILE, capture, command, logged
from build_qemu import QEMU, version
from sources import sha256


# RSP memory replies use two hex characters per byte. Stay below the packet limit.
RSP_MEMORY_CHUNK = 1024
FD_RECORD = struct.Struct("<6Q")  # result, fflags, a, b, c, rm


def qemu_signature(image, stop, signature, length, cpu_profile):
    host, child = socket.socketpair()
    with host, child:
        host.settimeout(10)
        argv = [QEMU, "-machine", "virt,aclint=off,aia=none", "-accel", "tcg", "-cpu", cpu_profile.read_text().strip(),
                "-smp", "1", "-m", "128M", "-display", "none", "-monitor", "none", "-serial", "none",
                "-bios", image, "-S", "-chardev", f"socket,id=rsp,fd={child.fileno()}", "-gdb", "chardev:rsp"]
        with image.with_name(f"{image.stem}-qemu.log").open("wb") as errors, subprocess.Popen(
                argv, env=ENV, stderr=errors, pass_fds=(child.fileno(),)) as guest, host.makefile("rb") as wire:
            child.close()

            def request(message):
                body = message.encode("ascii")
                host.sendall(b"$" + body + b"#" + f"{sum(body) & 255:02x}".encode("ascii"))
                if wire.read(2) != b"+$":
                    raise ValueError(f"unexpected GDB remote packet for {message}")
                reply = bytearray()
                while (byte := wire.read(1)) != b"#":
                    if not byte or len(reply) >= 4096:
                        raise ValueError(f"truncated or oversized GDB response for {message}")
                    reply.extend(byte)
                if int(wire.read(2), 16) != sum(reply) & 255:
                    raise ValueError(f"invalid GDB checksum for {message}")
                host.sendall(b"+")
                return bytes(reply)

            try:
                if request(f"Z1,{stop:x},4") != b"OK" or not request("c").startswith(b"T05"):
                    raise ValueError(f"QEMU did not reach probe breakpoint {stop:#x}")
                regs = bytes.fromhex(request("g").decode("ascii"))
                pc = struct.unpack_from("<Q", regs, 32 * 8)[0]
                if pc != stop:
                    raise ValueError(f"QEMU PC {pc:#x}; expected {stop:#x}")
                result = bytearray()
                for offset in range(0, length, RSP_MEMORY_CHUNK):
                    address = signature + offset
                    size = min(length - offset, RSP_MEMORY_CHUNK)
                    part = bytes.fromhex(request(f"m{address:x},{size:x}").decode("ascii"))
                    if len(part) != size:
                        raise ValueError(f"QEMU signature at {address:#x}: expected {size} bytes, got {len(part)}")
                    result.extend(part)
                return bytes(result)
            finally:
                if guest.poll() is None:
                    guest.terminate()
                    guest.wait(timeout=5)


def describe_fd_record(record):
    value, flags, a, b, c, rm = FD_RECORD.unpack(record)
    names = ("rne", "rtz", "rdn", "rup", "rmm")
    if rm < len(names):
        rounding = names[rm]
    elif 0x70 <= rm < 0x70 + len(names):
        rounding = f"dyn/{names[rm - 0x70]}"
    elif rm == 0xff:
        rounding = "none"
    else:
        rounding = f"{rm:#x}"
    return f"a={a:#018x} b={b:#018x} c={c:#018x} rm={rounding} bits={value:#018x} fflags={flags:#04x}"


def compare_fd(executable, image, fuel, stop, signature, expected, cases):
    if not cases or len(expected) != len(cases) * FD_RECORD.size:
        raise ValueError(f"FD signature length {len(expected)} does not match {len(cases)} cases")
    argv = [executable, image, str(fuel), f"{stop:x}", f"{signature:x}", str(len(expected)), "--fd"]
    for mode, extra in (("batch", []), ("step", ["--step"])):
        output = capture(*argv, *extra, timeout=20)
        image.with_name(f"{image.stem}-hesper-{mode}.hex").write_text(output + "\n")
        actual = bytes.fromhex(output)
        if len(actual) != len(expected):
            raise ValueError(f"Hesper {mode} signature length {len(actual)}; expected {len(expected)}")
        if actual == expected:
            continue
        offset = next(i for i, (want, got) in enumerate(zip(expected, actual, strict=True)) if want != got)
        case = offset // FD_RECORD.size
        start = case * FD_RECORD.size
        end = start + FD_RECORD.size
        pc, label = cases[case]
        operation = label.split("_", 3)[3]
        raise ValueError(
            f"Hesper {mode} first mismatch at signature byte {offset:#x}, case {case + 1} ({operation}, code {pc:#x}):\n"
            f"  QEMU   {describe_fd_record(expected[start:end])}\n"
            f"  Hesper {describe_fd_record(actual[start:end])}"
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cross_compile", help="absolute Buildroot tool prefix ending in '-'")
    parser.add_argument("--probe", choices=("im", "ac", "fd"), default="im")
    parser.add_argument("--hesper", type=Path, help="built release isa_probe executable (required for fd)")
    args = parser.parse_args()
    fd = args.probe == "fd"
    if fd != (args.hesper is not None):
        parser.error("--hesper is required for --probe fd and not used for integer probes")
    prefix = args.cross_compile
    name = f"isa-{args.probe}"
    qemu = version()
    base = ALPINE_LOGS if fd else LOGS
    base.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="fd-reference-", dir=base)) if fd else base
    obj, elf, image = [logs / f"{name}.{suffix}" for suffix in ("o", "elf", "bin")]
    march, abi = ("rv64imafdc_zicsr_zifencei", "lp64d") if fd else ("rv64imac_zicsr_zifencei", "lp64")
    command(prefix + "as", f"-march={march}", f"-mabi={abi}", "-o", obj, PROFILE / f"{name}.S")
    command(prefix + "ld", "--no-relax", "-Ttext", f"{DRAM:#x}", "-e", "_start", "-o", elf, obj)
    command(prefix + "objcopy", "-O", "binary", "-j", ".text", elf, image)
    logged(f"{name}-disassembly", prefix + "objdump", "-d", "-M", "no-aliases,numeric", elf, logs=logs)
    symbols = {}
    for row in capture(prefix + "nm", "-n", elf).splitlines():
        address, _, symbol = row.split()
        symbols[symbol] = int(address, 16)
    entry, stop, signature = [symbols[name] for name in ("_start", "probe_end", "signature")]
    if entry != DRAM or stop <= entry or (stop - entry) % 2:
        raise ValueError("expected an aligned probe at the DRAM base")
    program = image.read_bytes()
    offset = fuel = 0
    while entry + offset < stop:
        offset += 4 if program[offset] & 3 == 3 else 2
        fuel += 1
    if entry + offset != stop:
        raise ValueError("probe breakpoint is not on an instruction boundary")
    length = symbols["signature_end"] - signature
    if length <= 0 or signature < stop or symbols["signature_end"] > DRAM + len(program):
        raise ValueError("expected a nonempty signature after the probe in the loaded image")
    cpu_profile = PROFILE / "qemu-cpu-alpine.txt" if fd else CPU_PROFILE
    result = qemu_signature(image, stop, signature, length, cpu_profile)
    if fd:
        cases = sorted((pc, symbol) for symbol, pc in symbols.items() if symbol.startswith("fd_case_"))
        image.with_name(f"{name}-qemu.hex").write_text(result.hex() + "\n")
        compare_fd(args.hesper.resolve(), image, fuel, stop, signature, result, cases)
    report = dict(qemu=qemu, image=image.name, sha256=sha256(image), entry=f"{entry:#x}", stop=f"{stop:#x}",
                  fuel=fuel, address=f"{signature:#x}", bytes=length, signature=result.hex())
    if fd:
        report.update(cases=len(cases), hesper_modes=["batch", "step"])
    output = json.dumps(report, indent=2) + "\n"
    report_path = logs / f"{name}-reference.json"
    report_path.write_text(output)
    if fd:
        print(f"QEMU and Hesper (batch, step) agree on {len(cases)} cases, {length} bytes: {report_path}")
    else:
        print(output, end="")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, struct.error, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        sys.exit(f"error: {error}")
