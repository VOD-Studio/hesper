// Explicitly fetch pinned upstream SingleStepTests 65x02 data and verify the
// checked-in selection. Never rewrites tracked fixtures or manifests.
// Run with Bun; no dependencies.
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const fixtures = path.join(root, "crates/cpu6502/tests/data/singlestep");
const opcodesPath = path.join(root, "crates/cpu6502/tests/data/opcodes.txt");

interface ManifestFile {
  opcode: string;
  source_sha256: string;
  source_count: number;
  fixture_sha256: string;
  selected_count: number;
}
interface Manifest {
  repository: string;
  revision: string;
  variant: string;
  files: ManifestFile[];
}

const usage = `Usage: bun tools/prepare_singlestep.ts [--full]

Explicitly fetch pinned upstream data and verify the checked-in selection.
  --full   Prepare all 151 official opcode files instead of the fixture selection.
  --help   Show this help.`;

function checkedHash(data: Uint8Array, expected: string, label: string): void {
  const actual = new Bun.CryptoHasher("sha256").update(data).digest("hex");
  if (actual !== expected) {
    throw new Error(`${label}: SHA-256 expected ${expected}, got ${actual}`);
  }
}

async function fetchCapped(url: string, maxBytes: number): Promise<Uint8Array> {
  const response = await fetch(url, { signal: AbortSignal.timeout(30_000) });
  if (!response.ok) {
    throw new Error(`Download failed: HTTP ${response.status} from ${url}`);
  }
  const reader = response.body!.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.length;
    if (total > maxBytes) {
      await reader.cancel();
      throw new Error(`${url}: source exceeds ${maxBytes} byte limit`);
    }
    chunks.push(value);
  }
  const data = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    data.set(chunk, offset);
    offset += chunk.length;
  }
  return data;
}

// Runs `fn` over `items` with at most `limit` in flight, results in input order
// (mirrors Python's ThreadPoolExecutor(max_workers=...).map).
async function mapLimit<T, R>(items: T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const results: R[] = new Array(items.length);
  let next = 0;
  async function worker(): Promise<void> {
    for (;;) {
      const index = next++;
      if (index >= items.length) return;
      results[index] = await fn(items[index]);
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  if (args.includes("--help")) {
    console.log(usage);
    return;
  }
  const full = args.includes("--full");
  if (args.some((arg) => arg !== "--full")) {
    throw new Error(usage);
  }

  const manifest: Manifest = JSON.parse(
    fs.readFileSync(path.join(fixtures, full ? "full-manifest.json" : "manifest.json"), "utf8"),
  );
  const { revision } = manifest;
  if (
    manifest.repository !== "https://github.com/SingleStepTests/65x02" ||
    manifest.variant !== "6502/v1" ||
    revision.length !== 40 ||
    !/^[0-9a-f]{40}$/.test(revision) ||
    manifest.files.length === 0
  ) {
    throw new Error("expected a pinned, nonempty NMOS 6502/v1 manifest");
  }
  const opcodes = manifest.files.map((f) => f.opcode);
  if (new Set(opcodes).size !== opcodes.length || opcodes.some((op) => !/^[0-9a-f]{2}$/.test(op))) {
    throw new Error("manifest has duplicate or invalid opcode filenames");
  }
  if (full) {
    const official = new Set(
      fs
        .readFileSync(opcodesPath, "utf8")
        .split(/\r\n|\r|\n/)
        .filter((line) => line && !line.startsWith("#"))
        .map((line) => line.trim().split(/\s+/)[0].toLowerCase()),
    );
    const selected = new Set(opcodes);
    const sameSet = official.size === selected.size && [...official].every((op) => selected.has(op));
    if (
      official.size !== 151 ||
      !sameSet ||
      manifest.files.some(
        (f) => f.source_count !== 10000 || f.selected_count !== 10000 || f.source_sha256 !== f.fixture_sha256,
      )
    ) {
      throw new Error("full manifest must preserve all 10000 cases for each official opcode");
    }
  }

  const cache = path.join(root, ".cache/cpu6502/singlestep", revision, "6502/v1");
  fs.mkdirSync(cache, { recursive: true });

  async function prepare(entry: ManifestFile): Promise<number> {
    const name = `${entry.opcode}.json`;
    const target = path.join(cache, name);
    let raw: Uint8Array;
    if (fs.existsSync(target)) {
      raw = fs.readFileSync(target);
    } else {
      const url = `https://raw.githubusercontent.com/SingleStepTests/65x02/${revision}/6502/v1/${name}`;
      raw = await fetchCapped(url, 16 * 1024 * 1024);
    }
    checkedHash(raw, entry.source_sha256, name);
    const cases = JSON.parse(Buffer.from(raw).toString("utf8"));
    const count = entry.selected_count;
    if (cases.length !== entry.source_count || !(count > 0 && count <= cases.length)) {
      throw new Error(`${name}: unexpected source/selection count`);
    }
    if (!full) {
      const selectedText =
        "[\n" +
        cases
          .slice(0, count)
          .map((c: unknown) => JSON.stringify(c))
          .join(",\n") +
        "\n]\n";
      checkedHash(new TextEncoder().encode(selectedText), entry.fixture_sha256, `${name} selection`);
      checkedHash(fs.readFileSync(path.join(fixtures, name)), entry.fixture_sha256, `${name} fixture`);
    }
    if (!fs.existsSync(target)) {
      const temporary = `${target}.tmp`;
      fs.writeFileSync(temporary, raw);
      fs.renameSync(temporary, target);
    }
    return count;
  }

  const counts = await mapLimit(manifest.files, 4, prepare);
  console.log(
    `Verified source hashes (${full ? "full official corpus" : "original-order fixtures"}): ` +
      `${counts.length} files, ${counts.reduce((a, b) => a + b, 0)} cases`,
  );
  console.log(`Source cache: ${cache}`);
}

try {
  await main();
} catch (error) {
  console.error(`error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
