// Exercises the real script against the real pinned Visual6502 revD model;
// nothing here is mocked or stubbed. Self-sufficient: fetches on a cold cache
// and reuses it when warm, so only network access is required, no prior
// preparation step. Takes no CLI arguments (matches the original script).
import { expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const script = path.join(import.meta.dirname, "prepare_visual6502.ts");
const root = path.resolve(import.meta.dirname, "..");
const manifest: { revision: string; files: Record<string, string> } = JSON.parse(
  fs.readFileSync(path.join(root, "crates/cpu6502/tests/data/visual6502/manifest.json"), "utf8"),
);

function run(timeout = 15_000) {
  const result = Bun.spawnSync([process.execPath, script], {
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

test("fetches and verifies every pinned model file, then reuses the cache", () => {
  const first = run();
  expect(first.stderr).toBe("");
  expect(first.exitCode).toBe(0);
  expect(first.stdout).toContain(`Verified Visual6502 revD @ ${manifest.revision}: 7 files`);

  const cache = path.join(root, ".cache/cpu6502/visual6502", manifest.revision);
  for (const [name, digest] of Object.entries(manifest.files)) {
    const onDisk = fs.readFileSync(path.join(cache, name));
    expect(new Bun.CryptoHasher("sha256").update(onDisk).digest("hex")).toBe(digest);
  }

  // Second run must hit the now-warm cache and report the identical result.
  const second = run();
  expect(second.exitCode).toBe(0);
  expect(second.stdout).toBe(first.stdout);
});
