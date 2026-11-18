"""Assemble fixed integer instruction probes and observe the real QEMU TCG hart."""

import argparse
import json
import socket
import struct
import subprocess
import sys

from build_guest import CPU_PROFILE, DRAM, ENV, LOGS, PROFILE, capture, command, logged
from build_qemu import QEMU, version
from sources import sha256


def qemu_signature(image, stop, signature, length):
    host, child = socket.socketpair()
    with host, child:
        host.settimeout(10)
        argv = [QEMU, "-machine", "virt,aclint=off,aia=none", "-accel", "tcg", "-cpu", CPU_PROFILE.read_text().strip(),
                "-smp", "1", "-m", "128M", "-display", "none", "-monitor", "none", "-serial", "none",
                "-bios", image, "-S", "-chardev", f"socket,id=rsp,fd={child.fileno()}", "-gdb", "chardev:rsp"]
        with (LOGS / "isa-im-qemu.log").open("wb") as errors, subprocess.Popen(
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
                result = bytes.fromhex(request(f"m{signature:x},{length:x}").decode("ascii"))
                if pc != stop or len(result) != length:
                    raise ValueError(f"QEMU PC {pc:#x}, signature length {len(result)}; expected {stop:#x}, {length}")
                return result.hex()
            finally:
                if guest.poll() is None:
                    guest.terminate()
                    guest.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cross_compile", help="absolute Buildroot tool prefix ending in '-'")
    prefix = parser.parse_args().cross_compile
    qemu = version()
    LOGS.mkdir(parents=True, exist_ok=True)
    obj, elf, image = [LOGS / f"isa-im.{suffix}" for suffix in ("o", "elf", "bin")]
    command(prefix + "as", "-march=rv64imac_zicsr_zifencei", "-mabi=lp64", "-o", obj, PROFILE / "isa-im.S")
    command(prefix + "ld", "--no-relax", "-Ttext", f"{DRAM:#x}", "-e", "_start", "-o", elf, obj)
    command(prefix + "objcopy", "-O", "binary", "-j", ".text", elf, image)
    logged("isa-im-disassembly", prefix + "objdump", "-d", "-M", "no-aliases,numeric", elf)
    symbols = {}
    for row in capture(prefix + "nm", "-n", elf).splitlines():
        address, _, name = row.split()
        symbols[name] = int(address, 16)
    entry, stop, signature = [symbols[name] for name in ("_start", "probe_end", "signature")]
    if entry != DRAM or stop <= entry or (stop - entry) % 4:
        raise ValueError("expected a straight-line uncompressed probe at the DRAM base")
    length = 32
    result = qemu_signature(image, stop, signature, length)
    report = dict(qemu=qemu, image=image.name, sha256=sha256(image), entry=f"{entry:#x}", stop=f"{stop:#x}",
                  fuel=(stop - entry) // 4, address=f"{signature:#x}", bytes=length, signature=result)
    output = json.dumps(report, indent=2) + "\n"
    (LOGS / "isa-im-reference.json").write_text(output)
    print(output, end="")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, struct.error, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        sys.exit(f"error: {error}")
