// Host-only, explicitly requested ROM download; never embed or commit the image.
// Public availability does not grant redistribution rights. See
// crates/apple1/tests/data/README.md. Run directly with Bun; no dependencies.
import path from "node:path";

const source =
  "https://raw.githubusercontent.com/alangarf/apple-one/master/roms/wozmon.hex";
const expectedSize = 256;
const expectedSha256 =
  "e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25";
const defaultRom = path.resolve(import.meta.dirname, "../.cache/apple1/wozmon.bin");
const usage = `Usage: bun tools/prepare_wozmon.ts [--verify] [ROM]

Download the Woz Monitor HEX transcription, decode and verify before writing ROM.
Default ROM: ${defaultRom}
  --verify   Verify an existing binary without downloading or writing.
  --help     Show this help.
Only use the ROM if you have the rights to do so; do not commit or redistribute it.`;

function verify(data: Uint8Array): string {
  if (data.length !== expectedSize) {
    throw new Error(`ROM is ${data.length} bytes, expected ${expectedSize}`);
  }
  const digest = new Bun.CryptoHasher("sha256").update(data).digest("hex");
  if (digest !== expectedSha256) {
    throw new Error(
      `SHA-256 mismatch\n  got:      ${digest}\n  expected: ${expectedSha256}`,
    );
  }
  return digest;
}

async function main(): Promise<void> {
  const args = Bun.argv.slice(2);
  if (args.length === 1 && args[0] === "--help") {
    console.log(usage);
    return;
  }
  const verifyOnly = args[0] === "--verify";
  if (verifyOnly) args.shift();
  if (args.length > 1 || args.some((arg) => !arg || arg.startsWith("-"))) {
    throw new Error(usage);
  }
  const rom = args[0] ?? defaultRom;
  let data: Uint8Array;
  if (verifyOnly) {
    data = new Uint8Array(await Bun.file(rom).arrayBuffer());
  } else {
    const response = await fetch(source, { signal: AbortSignal.timeout(30_000) });
    if (!response.ok) {
      throw new Error(`Download failed: HTTP ${response.status} from ${source}`);
    }
    const tokens = (await response.text()).trim().split(/\s+/);
    if (
      tokens.length !== expectedSize ||
      tokens.some((token) => !/^[0-9a-fA-F]{2}$/.test(token))
    ) {
      throw new Error(`Invalid HEX: expected ${expectedSize} two-digit byte tokens`);
    }
    data = Uint8Array.from(tokens, (token) => Number.parseInt(token, 16));
  }
  const digest = verify(data);
  // A bad response must never replace a previously usable ROM.
  if (!verifyOnly) await Bun.write(rom, data);
  console.log(`OK: ${rom} (${data.length} bytes, SHA-256 ${digest})`);
}

try {
  await main();
} catch (error) {
  console.error(`error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
