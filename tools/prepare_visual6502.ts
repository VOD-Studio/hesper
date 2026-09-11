// Explicit, host-only fetch of the pinned Visual6502 revD reference model.
// Never imported by or linked into the CPU crate. Run with Bun; no dependencies.
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const manifestPath = path.join(root, "crates/cpu6502/tests/data/visual6502/manifest.json");

interface Manifest {
  revision: string;
  model: string;
  files: Record<string, string>;
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
      throw new Error(`${url}: response exceeds ${maxBytes} byte limit`);
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

async function main(): Promise<void> {
  const manifest: Manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const { revision } = manifest;
  if (revision !== "d8ecc129b34e0eaf320e0400fcf33329475bdb1e" || manifest.model !== "NMOS 6502 revD") {
    throw new Error("unexpected Visual6502 model or revision");
  }
  const cache = path.join(root, ".cache/cpu6502/visual6502", revision);
  fs.mkdirSync(cache, { recursive: true });
  const files = Object.entries(manifest.files);
  for (const [name, digest] of files) {
    if (path.basename(name) !== name) {
      throw new Error("invalid model filename");
    }
    const target = path.join(cache, name);
    const data = fs.existsSync(target)
      ? fs.readFileSync(target)
      : await fetchCapped(`https://raw.githubusercontent.com/trebonian/visual6502/${revision}/${name}`, 2 * 1024 * 1024);
    const actual = new Bun.CryptoHasher("sha256").update(data).digest("hex");
    if (actual !== digest) {
      throw new Error(`${name}: SHA-256 mismatch`);
    }
    if (!fs.existsSync(target)) {
      const temporary = `${target}.tmp`;
      fs.writeFileSync(temporary, data);
      fs.renameSync(temporary, target);
    }
  }
  console.log(`Verified Visual6502 revD @ ${revision}: ${files.length} files`);
}

try {
  await main();
} catch (error) {
  console.error(`error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
