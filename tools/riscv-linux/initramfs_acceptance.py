"""Literal commands and reference-fixture checks shared by the Linux gates."""

import json
import re

from serial_acceptance import result_line
from sources import sha256


def release_state(kernelrelease):
    return "uname", b"uname -r\n", lambda out: result_line(out, kernelrelease.encode("ascii"))


def custom_states(kernelrelease):
    return (
        ("proof-42", b"/bin/root-proof 6 7\n", lambda out: result_line(out, b"ROOT_OK:custom-v1:42")),
        ("proof-15", b"/bin/root-proof 3 5\n", lambda out: result_line(out, b"ROOT_OK:custom-v1:15")),
        release_state(kernelrelease),
    )


def default_states(kernelrelease):
    def missing(out):
        lines = out.splitlines()
        return (any(line.endswith(b"/bin/root-proof: not found") for line in lines)
                and not any(line.startswith(b"ROOT_OK:") for line in lines))

    return (("default-no-proof", b"/bin/root-proof 6 7\n", missing), release_state(kernelrelease))


def initrd_end_state(expected_end, record):
    def check(output):
        lines = [line for line in output.splitlines() if re.fullmatch(rb"[0-9a-f]{16}", line)]
        if len(lines) != 1 or int(lines[0], 16) != int(expected_end, 16):
            return False
        record(hex(int(lines[0], 16)))
        return True

    command = (b"/bin/busybox hexdump -v -e '8/1 \"%02x\" \"\\n\"' "
               b"/sys/firmware/devicetree/base/chosen/linux,initrd-end\n")
    return "initrd-end", command, check


def fixture_info(directory):
    path = directory / "initramfs.cpio"
    report_path = directory / "report.json"
    report = json.loads(report_path.read_bytes())
    info = report["initramfs"]
    reference = report["custom_boot"]
    if (path.stat().st_size != info["bytes"] or sha256(path) != info["sha256"]
            or reference["handoff"]["initrd_end"] != info["initrd_end"]
            or reference["exit_status"] != 0
            or not {"proof-42", "proof-15", "uname"} <= reference["serial_timing_seconds"].keys()):
        raise ValueError(f"{path}: fixture differs from successful QEMU reference {report_path}")
    return info
