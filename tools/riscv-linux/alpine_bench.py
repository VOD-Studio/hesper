"""Measure locked Alpine cold boots in the release CLI and production Chromium."""

import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time

from browser import COMMAND_SECONDS, SETUP_SECONDS, browser_session
from build_guest import ALPINE_BLK_IMAGE_DIR, ALPINE_IMAGE_DIR, ENV, verified_manifest
from disk_acceptance import SECTORS, make_seed
from native import ALPINE_MAX_FUEL, capture_serial, linux_command, require_exit
from serial_acceptance import result_line, session
from sources import CACHE, PROFILE, sha256

PROOF = b"HESPER_BENCH:42"
COMMAND = b"printf 'HESPER_BENCH:%s\\n' \"$((6 * 7))\"\n"
STATES = (("command", COMMAND, lambda output: result_line(output, PROOF)),)
ROOT = PROFILE.parent.parent


def capture(*argv):
    return subprocess.check_output(argv, env=ENV, cwd=ROOT, text=True).strip()


def write_report(path, report):
    next_path = path.with_suffix(".tmp")
    next_path.write_text(json.dumps(report, indent=2) + "\n")
    next_path.replace(path)


def describe(binary, dist, revision, diagnostic):
    return dict(revision=revision, binary=str(binary), binary_sha256=sha256(binary),
                site=str(dist), site_sha256={str(path.relative_to(dist)): sha256(path)
                                            for path in sorted(dist.rglob("*")) if path.is_file()},
                native_build="cargo build --locked --release -p hesper",
                web_build="bun run build" + (" --mode alpine-bench" if diagnostic else ""),
                native_compile_env={"HESPER_ALPINE_BENCH": "1", "CARGO_PROFILE_RELEASE_DEBUG": "1"}
                if diagnostic else {})


def native_boot(binary, images, seed, logs, timeout, diagnostic):
    argv = linux_command(binary, images=images, max_fuel=ALPINE_MAX_FUEL,
                         disk=(seed, logs / "export.raw") if seed else None)

    def exchange(receive, send):
        return session(receive, send, STATES, timeout, step_seconds=COMMAND_SECONDS, allow_cursor_query=True)

    run = capture_serial(argv, "pty", logs=logs, commands=(COMMAND,), exchange=exchange)
    require_exit(run, "quit")
    times = run["serial_timing_seconds"]
    offset = run["spawn_to_exchange_seconds"]
    run.update(prompt_seconds=offset + times["boot"], command_seconds=offset + times["command"])
    stages = {}
    for line in Path(run["diagnostics_log"]).read_text().splitlines():
        if line.startswith("linux-bench: "):
            event = json.loads(line.removeprefix("linux-bench: "))
            stages[event.pop("phase")] = event
    if diagnostic and stages.keys() != {"images", "load", "prompt"}:
        raise ValueError(f"missing native benchmark stages: {stages.keys()}")
    if not diagnostic and stages:
        raise ValueError("release timing requires a CLI built without HESPER_ALPINE_BENCH")
    if stages:
        prompt = stages["prompt"]
        prompt["retired_per_second"] = prompt["retired"] * 1000 / prompt["ms"]
    return dict(run, stages=stages)


def browser_boot(dist, images, manifest, seed, logs, timeout, diagnostic):
    argv = ["bun", PROFILE / "browser_session.mjs", dist, images, logs, "alpine-bench"]
    if seed:
        argv.append(seed)
    with browser_session(argv, logs, len(STATES)) as (event, receive, send, keys, finish, _):
        start = event(time.monotonic() + SETUP_SECONDS)
        if start["type"] != "start" or start["imageBytes"] != sum(item["bytes"] for item in manifest["images"]):
            raise ValueError("browser did not select the four locked images")
        session(receive, keys, STATES, timeout, step_seconds=COMMAND_SECONDS, allow_cursor_query=True)
        send(dict(type="finish"))
        done = finish()
    timing = done["timing"]
    stages = {event["phase"]: event for event in done["stages"]}
    if diagnostic and stages.keys() != {"images", "load", "prompt"}:
        raise ValueError(f"missing Chromium benchmark stages: {stages.keys()}")
    if not diagnostic and stages:
        raise ValueError("release timing requires a site built in the default Vite mode")
    if stages:
        prompt = stages["prompt"]
        for counter in ("spent", "retired", "time"):
            prompt[counter] = int(prompt[counter])
        prompt["retired_per_second"] = prompt["retired"] * 1000 / prompt["ms"]
        prompt["worker_to_xterm_ms"] = timing["prompt"] - prompt["posted"]
    return dict(browser=start["browser"], prompt_seconds=(timing["prompt"] - timing["clicked"]) / 1000,
                command_seconds=(timing["command"] - timing["clicked"]) / 1000,
                stages=stages, fuel=int(done["fuel"]), retired=int(done["retired"]), tick=int(done["tick"]),
                serial_log=str(logs / "browser-serial.raw"))


def runs(hosts, variants, samples, *, warmup):
    for index in range(-int(warmup), samples):
        for host in hosts:
            for disk in (False, True) if index % 2 == 0 else (True, False):
                order = variants if (index + disk) % 2 == 0 else reversed(variants)
                for variant in order:
                    yield index, host, disk, variant


def summarize(rows):
    groups = {}
    for row in rows:
        if row["index"] >= 0 and row["status"] == "pass":
            key = f'{row["host"]}/{"disk" if row["disk"] else "ram"}/{row["variant"]}'
            groups.setdefault(key, []).append(row["prompt_seconds"])
    summary = {}
    for key, values in groups.items():
        median = statistics.median(values)
        result = dict(seconds=values, median_seconds=median)
        if key.endswith("/candidate"):
            baseline = groups.get(key.removesuffix("/candidate") + "/baseline")
            if baseline:
                previous = statistics.median(baseline)
                result.update(saved_seconds=previous - median, saved_percent=100 * (previous - median) / previous)
        summary[key] = result
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("dist", type=Path)
    parser.add_argument("--revision", help="source revision of the supplied baseline artifacts (default: HEAD)")
    parser.add_argument("--candidate", type=Path, nargs=2, metavar=("BINARY", "DIST"))
    parser.add_argument("--candidate-revision")
    parser.add_argument("--seed", type=Path, help="reuse a generated 8 MiB benchmark seed")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--diagnostic", action="store_true", help="record stages using measurement builds")
    parser.add_argument("--host", choices=("native", "browser"), action="append")
    parser.add_argument("--disk", choices=("ram", "disk"), help="restrict to one boot profile")
    parser.add_argument("--boot-timeout", type=float, default=180)
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("the benchmark requires Linux PTYs and wait4 RSS units")
    if (args.samples < 1 or args.boot_timeout <= 0 or bool(args.candidate) != bool(args.candidate_revision)
            or args.diagnostic and args.candidate):
        parser.error("require positive samples, positive timeout, and a revision for a release candidate")
    hosts = args.host or ("native", "browser")
    if len(set(hosts)) != len(hosts):
        parser.error("each host can be selected once")
    variants = {"baseline": (args.binary.resolve(), args.dist.resolve())}
    if args.candidate:
        variants["candidate"] = tuple(path.resolve() for path in args.candidate)
    logs_root = ROOT / ".cache/riscv-linux"
    logs_root.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="alpine-bench-", dir=logs_root))
    report_path = logs / "report.json"
    print(f"Report: {report_path}", flush=True)
    rows = []
    report = dict(host=platform.platform(), cpu=next((line.split(":", 1)[1].strip()
                  for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name")),
                  platform.processor()), benchmark_driver_revision=capture("git", "rev-parse", "HEAD"),
                  tracked_changes=capture("git", "status", "--porcelain", "--untracked-files=no"),
                  diagnostic=args.diagnostic, samples=args.samples, warmup=not args.diagnostic,
                  boot_timeout_seconds=args.boot_timeout,
                  native_fuel_limit=ALPINE_MAX_FUEL, rustc=capture("rustc", "--version", "--verbose"),
                  relevant_build_environment={key: ENV[key] for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS",
                      "CARGO_PROFILE_RELEASE_DEBUG", "CARGO_TARGET_DIR") if key in ENV},
                  image_cache=str(CACHE), inputs={}, builds={}, runs=rows)
    errors = {}
    if "browser" in hosts:
        try:
            report["chromium"] = capture(os.environ.get("CHROME_BIN") or "google-chrome", "--version")
            report["bun"] = capture("bun", "--version")
        except (OSError, subprocess.SubprocessError) as error:
            errors["browser"] = str(error)
    for name, (binary, dist) in variants.items():
        revision = ((args.revision or report["benchmark_driver_revision"]) if name == "baseline"
                    else args.candidate_revision)
        try:
            report["builds"][name] = describe(binary, dist, revision, args.diagnostic)
        except OSError as error:
            errors[name] = str(error)
    profiles = {}
    for disk, images in ((False, ALPINE_IMAGE_DIR), (True, ALPINE_BLK_IMAGE_DIR)):
        if args.disk and disk != (args.disk == "disk"):
            continue
        try:
            manifest = verified_manifest(blk=disk, alpine=True, directory=images)
            report["inputs"][str(images)] = dict(manifest_sha256=sha256(images / "manifest.json"),
                                                  images=manifest["images"])
            profiles[disk] = (images, manifest)
        except (OSError, ValueError) as error:
            errors[disk] = str(error)
    seed = args.seed.resolve() if args.seed else logs / "seed.raw"
    if True in profiles:
        try:
            if not args.seed:
                make_seed(seed)
            if seed.stat().st_size != SECTORS * 512:
                raise ValueError("expected an 8 MiB seed disk")
            report["seed"] = dict(path=str(seed), sha256=sha256(seed))
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            errors[True] = str(error)

    write_report(report_path, report)
    for index, host, disk, variant in runs(hosts, tuple(variants), args.samples, warmup=not args.diagnostic):
        if args.disk and disk != (args.disk == "disk"):
            continue
        profile = "disk" if disk else "ram"
        directory = logs / ("warmup" if index < 0 else f"sample-{index + 1}") / host / profile / variant
        directory.mkdir(parents=True)
        row = dict(index=index, host=host, disk=disk, variant=variant, logs=str(directory))
        binary, dist = variants[variant]
        try:
            for key in (disk, host, variant):
                if key in errors:
                    raise ValueError(errors[key])
            images, manifest = profiles[disk]
            data_disk = seed if disk else None
            metrics = (native_boot(binary, images, data_disk, directory, args.boot_timeout, args.diagnostic)
                       if host == "native" else
                       browser_boot(dist, images, manifest, data_disk, directory, args.boot_timeout, args.diagnostic))
            row.update(metrics, status="pass")
            print(f"{directory.relative_to(logs)}: {row['prompt_seconds']:.3f} s", flush=True)
        except (OSError, EOFError, ValueError, subprocess.SubprocessError) as error:
            row.update(status="fail", error=str(error))
            (directory / "error.log").write_text(str(error) + "\n")
            print(f"{directory.relative_to(logs)}: {error}", file=sys.stderr, flush=True)
        rows.append(row)
        write_report(directory / "run.json", row)
        report["summary"] = summarize(rows)
        write_report(report_path, report)
    if "seed" in report:
        report["seed"]["after_sha256"] = sha256(seed)
        if report["seed"]["after_sha256"] != report["seed"]["sha256"]:
            report["error"] = "benchmark changed the input seed"
    write_report(report_path, report)
    for name, result in report["summary"].items():
        savings = (f", saved {result['saved_seconds']:.3f} s ({result['saved_percent']:.2f}%)"
                   if "saved_seconds" in result else "")
        print(f"{name}: median {result['median_seconds']:.3f} s, raw {result['seconds']}{savings}")
    return int("error" in report or any(row["status"] == "fail" for row in rows))


if __name__ == "__main__":
    sys.exit(main())
