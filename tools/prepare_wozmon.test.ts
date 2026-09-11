import { expect, test } from "bun:test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

test("a 256-byte download with the wrong hash cannot overwrite an existing ROM", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hesper-wozmon-"));
  try {
    const rom = path.join(dir, "existing.bin");
    const previous = Buffer.from("previous user-supplied image");
    fs.writeFileSync(rom, previous);
    const preload = path.join(dir, "fetch.ts");
    // No real ROM fixture or network access: valid HEX, wrong fingerprint.
    fs.writeFileSync(
      preload,
      'globalThis.fetch = async () => new Response("00\\n".repeat(256));\n',
    );
    const result = Bun.spawnSync(
      [
        process.execPath,
        "--preload",
        preload,
        path.join(import.meta.dirname, "prepare_wozmon.ts"),
        rom,
      ],
      { stdout: "pipe", stderr: "pipe", timeout: 10_000 },
    );
    expect(result.exitCode).toBe(1);
    expect(fs.readFileSync(rom)).toEqual(previous);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
