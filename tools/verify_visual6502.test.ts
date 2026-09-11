// Exercises the real driver against the real pinned Visual6502 revD cache
// (see crates/cpu6502/tests/data/visual6502/manifest.json); nothing here is
// mocked or stubbed. Requires the cache populated by
// `bun tools/prepare_visual6502.ts`; run `bun test tools/verify_visual6502.test.ts`
// after preparing data, same precondition as the CPU's own full external checks.
import { expect, test } from "bun:test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const script = path.join(import.meta.dirname, "verify_visual6502.ts");

function run(args: string[], timeout = 5_000) {
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

test("reproduces a single pins case against the pinned revD reference model", () => {
  const { exitCode, stdout, stderr } = run([
    "--suite",
    "pins",
    "--case",
    "nop-irq-0",
  ]);
  expect(stderr).toBe("");
  expect(exitCode).toBe(0);
  expect(stdout).toContain("1 pins traces reproduced (nop-irq-0)");
});

test("reproduces a physical RESET case with non-default registers and stack wrap", () => {
  const { exitCode, stdout, stderr } = run([
    "--suite",
    "reset",
    "--case",
    "reset-registers-stack-wrap",
  ]);
  expect(stderr).toBe("");
  expect(exitCode).toBe(0);
  expect(stdout).toContain(
    "1 reset traces reproduced (reset-registers-stack-wrap)",
  );
});

test("rejects an unknown --suite", () => {
  const { exitCode, stdout, stderr } = run(["--suite", "bogus"]);
  expect(exitCode).toBe(1);
  expect(stdout).toBe("");
  expect(stderr).toContain("Usage: bun tools/verify_visual6502.ts");
});

test("rejects a --case that matches no scenario", () => {
  const { exitCode, stderr } = run(["--case", "does-not-exist"]);
  expect(exitCode).toBe(1);
  expect(stderr).toContain('No reference case matches "does-not-exist"');
});

test(
  "--record writes the full pins corpus as parseable, non-empty traces without touching the fixture",
  () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hesper-visual6502-"));
    try {
      const out = path.join(dir, "pins.json");
      const { exitCode, stdout, stderr } = run(
        ["--suite", "pins", "--record", out],
        20_000,
      );
      expect(stderr).toBe("");
      expect(exitCode).toBe(0);
      expect(stdout).toContain("Recorded 246 independent revD pins traces");
      const recorded = JSON.parse(fs.readFileSync(out, "utf8"));
      expect(recorded).toHaveLength(246);
      expect(recorded[0]).toMatchObject({ name: "nop-irq-0" });
      expect(recorded[0].cycles.length).toBeGreaterThan(0);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  },
  20_000,
);
