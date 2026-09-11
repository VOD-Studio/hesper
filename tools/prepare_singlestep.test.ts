// Exercises the real script against the real pinned SingleStepTests data;
// nothing here is mocked or stubbed. Self-sufficient: fetches on a cold cache
// and reuses it when warm, so only network access is required, no prior
// preparation step.
import { expect, test } from "bun:test";
import path from "node:path";

const script = path.join(import.meta.dirname, "prepare_singlestep.ts");

function run(args: string[], timeout = 30_000) {
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
  expect(stdout).toContain("Usage: bun tools/prepare_singlestep.ts [--full]");
});

test("rejects an unrecognized argument", () => {
  const { exitCode, stdout, stderr } = run(["--bogus"]);
  expect(stdout).toBe("");
  expect(exitCode).toBe(1);
  expect(stderr).toContain("Usage: bun tools/prepare_singlestep.ts");
});

test("verifies the checked-in 672-case fixture selection against the pinned manifest", () => {
  const { exitCode, stdout, stderr } = run([]);
  expect(stderr).toBe("");
  expect(exitCode).toBe(0);
  expect(stdout).toContain("Verified source hashes (original-order fixtures): 21 files, 672 cases");
});

test(
  "--full verifies the entire pinned 151-opcode, 1,510,000-case official corpus",
  () => {
    const { exitCode, stdout, stderr } = run(["--full"], 60_000);
    expect(stderr).toBe("");
    expect(exitCode).toBe(0);
    expect(stdout).toContain("Verified source hashes (full official corpus): 151 files, 1510000 cases");
  },
  60_000,
);
