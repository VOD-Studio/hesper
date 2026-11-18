"""Transport-independent boot and command checks for the fixed Linux guest."""

import re
import time

PROMPT = b"hesper# "
STEP_SECONDS = 20
STEP_BYTES = 256 * 1024
COMMANDS = (
    b"uname -a\n",
    b"cat /proc/cpuinfo\n",
    b"printf 'HESPER_OK:%s\\n' \"$((6 * 7))\"\n",
)


def drive(receive, send, kernelrelease, isa):
    """receive(seconds) returns bytes or raises on timeout; send writes literal bytes."""
    started = time.monotonic()
    timings = {}

    def wait_prompt(state):
        deadline = time.monotonic() + STEP_SECONDS
        buffer = bytearray()
        try:
            while not buffer.endswith(PROMPT):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("deadline reached")
                chunk = receive(remaining)
                if not chunk:
                    raise EOFError("transport closed")
                buffer.extend(chunk)
                if len(buffer) > STEP_BYTES:
                    raise ValueError(f"more than {STEP_BYTES} bytes before prompt")
        except (OSError, EOFError, ValueError) as error:
            raise ValueError(f"{state}: {error}; completed={list(timings)}; serial tail={bytes(buffer[-2048:])!r}") from error
        return bytes(buffer).replace(b"\r\n", b"\n")

    wait_prompt("boot")
    timings["boot"] = time.monotonic() - started
    for index, (state, command) in enumerate(zip(("uname", "cpuinfo", "printf"), COMMANDS)):
        send(command)
        output = wait_prompt(state)
        if index == 0:
            release = re.escape(kernelrelease.encode("ascii"))
            ok = re.search(rb"^Linux\s+\S+ " + release + rb"\s+[^\n]*\briscv64\b[^\n]*$", output, re.M)
        elif index == 1:
            reported = re.search(rb"^isa[ \t]*:[ \t]*(\S+)$", output, re.M)
            # Linux reports Zca as implied by C even when the tree lists only C.
            ok = (reported and reported[1] in (isa.encode("ascii"), (isa + "_zca").encode("ascii"))
                  and re.search(rb"^processor[ \t]*:[ \t]*0$", output, re.M)
                  and re.search(rb"^hart[ \t]*:[ \t]*0$", output, re.M)
                  and re.search(rb"^mmu[ \t]*:[ \t]*sv39$", output, re.M))
        else:
            ok = re.search(rb"^HESPER_OK:42$", output, re.M)
        if not ok:
            raise ValueError(f"{state}: result mismatch; completed={list(timings)}; serial tail={output[-2048:]!r}")
        timings[state] = time.monotonic() - started
    return timings
