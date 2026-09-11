// Exercises the real script; nothing here is mocked or stubbed. The
// `--binary-only` path is real and self-sufficient (fetches on a cold cache,
// reuses it when warm), so no prior preparation step is required — only
// network access on a cold cache. The full decimal/interrupt build (`make`
// plus ca65/ld65) is heavy and networked; it is exercised manually per
// AGENTS.md, not by this regression test.
import { expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const script = path.join(import.meta.dirname, "prepare_klaus.ts");
const root = path.resolve(import.meta.dirname, "..");
const REVISION = "7954e2dbb49c469ea286070bf46cdd71aeb29e4b";
const BINARY_SHA256 = "fa12bfc761e6f9057e4cc01a665a7b800ff01ae91f598af1e39a1201d01953fd";

function run(args: string[], timeout = 15_000) {
  const result = Bun.spawnSync([process.execPath, script, ...args], {
    stdout: "pipe",
    stderr: "pipe",
    timeout,
  });
  return {
    exitCode: result.exitCode,
    stdout: result.stdout.toString(),
    stderr: result.stderr.toString(),
  };
}

test("--help prints usage without touching the network or cache", () => {
  const { exitCode, stdout, stderr } = run(["--help"]);
  expect(stderr).toBe("");
  expect(exitCode).toBe(0);
  expect(stdout).toContain("Usage: bun tools/prepare_klaus.ts [--binary-only] [--check-only]");
});

test("rejects an unrecognized argument", () => {
  const { exitCode, stdout, stderr } = run(["--bogus"]);
  expect(stdout).toBe("");
  expect(exitCode).toBe(1);
  expect(stderr).toContain("Usage: bun tools/prepare_klaus.ts");
});

test("fetches and verifies the pinned functional test binary, then reuses the cache", () => {
  const first = run(["--binary-only"]);
  expect(first.stderr).toBe("");
  expect(first.exitCode).toBe(0);
  expect(first.stdout).toContain(`Verified Klaus Dormann NMOS test data @ ${REVISION}`);
  expect(first.stdout).toContain("bin_files/6502_functional_test.bin (65536 B)");

  const target = path.join(root, ".cache/cpu6502/klaus", REVISION, "bin_files/6502_functional_test.bin");
  const onDisk = fs.readFileSync(target);
  expect(onDisk.length).toBe(65536);
  expect(new Bun.CryptoHasher("sha256").update(onDisk).digest("hex")).toBe(BINARY_SHA256);

  // Second run must hit the now-warm cache and report the identical result.
  const second = run(["--binary-only"]);
  expect(second.exitCode).toBe(0);
  expect(second.stdout).toBe(first.stdout);
});
