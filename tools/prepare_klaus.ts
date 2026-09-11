// Explicitly fetch pinned upstream Klaus Dormann test data and verify integrity.
// Caches files under .cache/cpu6502/klaus/. Never rewrites tracked repository
// files. Run with Bun; no dependencies. Building the decimal and interrupt
// images additionally needs a system `make`, a C compiler, and `tar` (used only
// to unpack the pinned, hash-verified cc65 source archive).
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const CC65 = "555282497c3ecf8b313d87d5973093af19c35bd5"; // Upstream tag V2.19
const DECIMAL_HASH = "03798ab778456cc350044fdbe28b4078278648892712b994cdbdda09018674e7";
const INTERRUPT_HASH = "ecc829d494fd1f4b4262ac5c58e8cb3925f7aa7570ce815e9a3214b6f66a3c58";
const CONFIG =
  'MEMORY { ZP: start = $0000, size = $0100, file = ""; RAM: start = $0200, size = $FE00, file = %O; }\n' +
  "SEGMENTS { ZEROPAGE: load = ZP, type = zp; CODE: load = RAM, type = rw; }\n";

const REVISION = "7954e2dbb49c469ea286070bf46cdd71aeb29e4b";
const RAW_BASE_URL = `https://raw.githubusercontent.com/Klaus2m5/6502_65C02_functional_tests/${REVISION}`;

interface PinnedFile {
  relPath: string;
  sha256: string;
  size: number;
  description: string;
}

// Pinned upstream files.
const PINNED_FILES: PinnedFile[] = [
  {
    relPath: "bin_files/6502_functional_test.bin",
    sha256: "fa12bfc761e6f9057e4cc01a665a7b800ff01ae91f598af1e39a1201d01953fd",
    size: 65536,
    description: "Pre-assembled NMOS functional test image",
  },
  {
    relPath: "bin_files/6502_functional_test.lst",
    sha256: "a85bf71692a7087182f3a574830e74d420d166032f37d385e34940e7bb8d49e3",
    size: 728468,
    description: "Functional test listing file",
  },
  {
    relPath: "6502_functional_test.a65",
    sha256: "f2665bd02288866c2b210b908e3f387926b4c9f0e0af5ad5513c474361ad1265",
    size: 148497,
    description: "Functional test source",
  },
  {
    relPath: "6502_decimal_test.a65",
    sha256: "dfbe4b907c5821d47d9f7f74eeb51197cf4b9f5260375bc1c6c48c008220e1a0",
    size: 9186,
    description: "Bruce Clark decimal test source",
  },
  {
    relPath: "6502_interrupt_test.a65",
    sha256: "3d794b23a1740e650483990a12f9aa8563b915b48668ae86f51076bee450ecf6",
    size: 31198,
    description: "Interrupt test source",
  },
  {
    relPath: "license.txt",
    sha256: "8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903",
    size: 35147,
    description: "Upstream GPL-3.0-or-later license",
  },
];

const usage = `Usage: bun tools/prepare_klaus.ts [--binary-only] [--check-only]

Explicitly fetch pinned upstream Klaus Dormann test data and verify integrity.
  --binary-only   Fetch/verify only the pre-assembled functional test binary.
  --check-only    Verify integrity of cached files without downloading missing ones.
  --help          Show this help.`;

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

function writeAtomic(target: string, data: Uint8Array | string): void {
  const temporary = `${target}.tmp`;
  fs.writeFileSync(temporary, data);
  fs.renameSync(temporary, target);
}

// Mirrors Python's default str.split(): trims, then splits on whitespace runs.
function pysplit(line: string): string[] {
  const trimmed = line.trim();
  return trimmed === "" ? [] : trimmed.split(/\s+/);
}

// Mirrors Python's str.splitlines(): no trailing empty entry for a final newline.
function splitlines(text: string): string[] {
  if (text === "") return [];
  const lines = text.split(/\r\n|\r|\n/);
  if (/[\r\n]$/.test(text)) lines.pop();
  return lines;
}

function arraysEqual(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

// Applies a static, ordered set of directive-name rewrites anchored at line
// start, preserving the same regex shape (leading horizontal-or-full
// whitespace capture) the original Python translation used per family.
function renameDirectives(source: string, directives: Record<string, string>, whitespace: string): string {
  let result = source;
  for (const [old, replacement] of Object.entries(directives)) {
    result = result.replace(new RegExp(`^(${whitespace})${old}\\b`, "gm"), `$1${replacement}`);
  }
  return result;
}

// Canonical, order-preserving signature of every 6502 mnemonic statement in an
// adapter source, used to prove ca65 translation never dropped or reordered an
// original Klaus Dormann instruction (only assembler directives may change).
function instructions(source: string): string[] {
  const opcodesPath = path.join(root, "crates/cpu6502/tests/data/opcodes.txt");
  const ops = new Set(
    splitlines(fs.readFileSync(opcodesPath, "utf8"))
      .filter((line) => line && !line.startsWith("#"))
      .map((line) => pysplit(line)[1].toLowerCase()),
  );
  const result: string[] = [];
  for (const line of splitlines(source)) {
    const fields = pysplit(line.split(";")[0]);
    for (let index = 0; index < Math.min(2, fields.length); index++) {
      if (ops.has(fields[index].toLowerCase())) {
        const joined = fields
          .slice(index)
          .join("")
          .toLowerCase()
          .replaceAll("\\1", "arg")
          .replaceAll("ibit", "arg")
          .replaceAll("skip\\?", "@skip");
        result.push(joined);
        break;
      }
    }
  }
  return result;
}

async function prepareInterrupt(cache: string, assembler: string): Promise<void> {
  const original = fs.readFileSync(path.join(cache, "6502_interrupt_test.a65"), "utf8");
  let source = original.replace(/^(\w+)[ \t]+equ[ \t]+/gm, "$1 = ");
  source = source.replace(/^[ \t]*noopt[^\n]*$/gm, "");
  // Horizontal whitespace only: crossing a newline here would erase trap bodies.
  source = source.replace(/^(\w+)[ \t]+macro\b[^\n]*/gm, (_match, name: string) => {
    const withArg = ["I_set", "I_clr", "push_stat", "set_stat"].includes(name);
    return `.macro ${name}${withArg ? " arg" : ""}`;
  });
  source = source.replaceAll("\\1", "arg").replace(/\bibit\b/g, "arg");
  source = source.replaceAll("skip\\?", "@skip");
  source = renameDirectives(
    source,
    { if: ".if", else: ".else", endif: ".endif", endm: ".endmacro", db: ".byte", dw: ".word", include: ".include" },
    "[ \\t]*",
  );
  source = source.replace(/^(\w+[ \t]+)ds[ \t]+/gm, "$1.res ").replaceAll("!=", "<>");
  source = source.replace(/^[ \t]*(?:data|bss|code)[ \t]*$/gm, "");
  const segments: Record<string, string> = {
    zero_page: "ZEROPAGE",
    data_segment: "DATA",
    code_segment: "CODE",
    "\\$fffa": "VECTORS",
  };
  for (const [old, segment] of Object.entries(segments)) {
    source = source.replace(new RegExp(`^[ \\t]*org ${old}[ \\t]*$`, "gm"), `.segment "${segment}"`);
  }
  source = source.replace(/^[ \t]*end start[ \t]*$/gm, "");
  source = source.replaceAll(
    "        success         ;if you get here everything went well",
    "PASSED:\n        success         ;if you get here everything went well",
  );
  source = ".feature labels_without_colons\n.export start, PASSED, I_port\n" + source;
  if (instructions(original).length !== 505 || !arraysEqual(instructions(source), instructions(original))) {
    throw new Error("interrupt adapter must preserve all 505 original instruction statements");
  }
  checkedHash(
    new TextEncoder().encode(source),
    "f2ce31cba447eef9ad0a292a5d616b85e4b1ba1b947abaf168d3c00fcad0b1d9",
    "interrupt adapter",
  );
  fs.writeFileSync(path.join(cache, "interrupt-ca65.s"), source);
  fs.writeFileSync(
    path.join(cache, "interrupt.cfg"),
    'MEMORY { ZP: start=$000A,size=$00F6,file=""; RAM: start=$0000,size=$10000,file=%O,fill=yes; }\n' +
      "SEGMENTS { ZEROPAGE:load=ZP,type=zp; DATA:load=RAM,type=bss,start=$0200; CODE:load=RAM,type=rw,start=$0400; VECTORS:load=RAM,type=ro,start=$FFFA; }\n",
  );
  await Bun.$`${path.join(assembler, "ca65")} -o ${path.join(cache, "interrupt.o")} -l ${path.join(cache, "interrupt.lst")} ${path.join(cache, "interrupt-ca65.s")}`;
  await Bun.$`${path.join(assembler, "ld65")} -C ${path.join(cache, "interrupt.cfg")} -o ${path.join(cache, "interrupt.bin")} -Ln ${path.join(cache, "interrupt.lbl")} ${path.join(cache, "interrupt.o")}`;
  checkedHash(fs.readFileSync(path.join(cache, "interrupt.bin")), INTERRUPT_HASH, "interrupt image");
  const lbl = splitlines(fs.readFileSync(path.join(cache, "interrupt.lbl"), "utf8"));
  if (!arraysEqual(lbl, ["al 00BFFC .I_port", "al 0006F5 .PASSED", "al 000400 .start"])) {
    throw new Error("interrupt entry, success or feedback address changed");
  }
}

async function prepareDecimal(cacheDir: string, checkOnly: boolean): Promise<void> {
  if (checkOnly) {
    checkedHash(fs.readFileSync(path.join(cacheDir, "decimal.bin")), DECIMAL_HASH, "decimal image");
    checkedHash(fs.readFileSync(path.join(cacheDir, "interrupt.bin")), INTERRUPT_HASH, "interrupt image");
    return;
  }
  const toolCache = path.join(root, ".cache/cpu6502/cc65");
  const archive = path.join(toolCache, `${CC65}.tar.gz`);
  const raw = fs.existsSync(archive)
    ? fs.readFileSync(archive)
    : await fetchCapped(`https://codeload.github.com/cc65/cc65/tar.gz/${CC65}`, 16 * 1024 * 1024);
  checkedHash(raw, "62c77f00ef4141153a0ddecef06ca086c11c68f14d022beadeaf353d1d833ff1", "cc65 archive");
  if (!fs.existsSync(archive)) {
    fs.mkdirSync(toolCache, { recursive: true });
    fs.writeFileSync(archive, raw);
  }
  const sourceDir = path.join(toolCache, `cc65-${CC65}`);
  if (!fs.existsSync(sourceDir)) {
    // Archive hash is verified above; this unpacks a known-good pinned tree.
    await Bun.$`tar -xzf ${archive} -C ${toolCache}`;
  }
  // One dependency graph prevents duplicate concurrent builds of common objects.
  await Bun.$`make -C ${path.join(sourceDir, "src")} ca65 ld65 -j4 ${"BUILD_ID=Git 55528249"}`;

  const originalDecimal = fs.readFileSync(path.join(cacheDir, "6502_decimal_test.a65"), "utf8");
  let source = originalDecimal;
  for (const setting of ["cputype", "vld_bcd"]) {
    if (!new RegExp(`^${setting}\\s*=\\s*0\\b`, "m").test(source)) {
      throw new Error(`expected ${setting}=0 in pinned source`);
    }
  }
  let count = 0;
  source = source.replace(/^(chk_[anvzc]\s*=\s*)[01]/gm, (_match, prefix: string) => {
    count++;
    return `${prefix}1`;
  });
  if (count !== 5) {
    throw new Error("expected all five A/N/V/Z/C check switches");
  }
  // Translate directives only; all 6502 instructions and oracle code stay intact.
  source = source.replaceAll("end_of_test macro", ".macro end_of_test");
  source = source.replace(/^(\s*)db\s+/gm, "$1.byte ");
  source = source.replace(/^(\w+\s+)ds\s+/gm, "$1.res ");
  source = source.replace(/^\s*bss\s*$/gm, '.segment "ZEROPAGE"');
  source = source.replace(/^\s*code\s*$/gm, '.segment "CODE"');
  source = source.replace(/^\s*org\s+[^\n]+$/gm, "; Placement is defined by decimal.cfg.");
  source = renameDirectives(source, { if: ".if", endif: ".endif", endm: ".endmacro" }, "\\s*");
  source = source.replaceAll("!=", "<>");
  source = source.replace(/^\s*end\s+TEST\s*$/gm, "");
  source =
    "; Adapted from the pinned public-domain Bruce Clark test: ca65 directives and all flag checks.\n" +
    ".feature labels_without_colons\n.export TEST, DONE\n.exportzp ERROR\n" +
    source;
  if (!arraysEqual(instructions(source), instructions(originalDecimal))) {
    throw new Error("decimal adapter changed original instruction statements");
  }
  checkedHash(
    new TextEncoder().encode(source),
    "586f6f2da4fc8763630f73211356c6de5f8d47cbc01cc38fddcd761a6ed3ec39",
    "decimal adapter",
  );
  fs.writeFileSync(path.join(cacheDir, "decimal-ca65.s"), source);
  fs.writeFileSync(path.join(cacheDir, "decimal.cfg"), CONFIG);
  await Bun.$`${path.join(sourceDir, "bin/ca65")} -o ${path.join(cacheDir, "decimal.o")} -l ${path.join(cacheDir, "decimal.lst")} ${path.join(cacheDir, "decimal-ca65.s")}`;
  await Bun.$`${path.join(sourceDir, "bin/ld65")} -C ${path.join(cacheDir, "decimal.cfg")} -o ${path.join(cacheDir, "decimal.bin")} -Ln ${path.join(cacheDir, "decimal.lbl")} ${path.join(cacheDir, "decimal.o")}`;
  checkedHash(fs.readFileSync(path.join(cacheDir, "decimal.bin")), DECIMAL_HASH, "decimal image");
  const lbl = splitlines(fs.readFileSync(path.join(cacheDir, "decimal.lbl"), "utf8"));
  if (!arraysEqual(lbl, ["al 00024B .DONE", "al 00000B .ERROR", "al 000200 .TEST"])) {
    throw new Error("decimal entry, DONE or ERROR address changed");
  }
  await prepareInterrupt(cacheDir, path.join(sourceDir, "bin"));
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  if (argv.includes("--help")) {
    console.log(usage);
    return;
  }
  const args = new Set(argv);
  const binaryOnly = args.delete("--binary-only");
  const checkOnly = args.delete("--check-only");
  if (args.size > 0) throw new Error(usage);

  const cacheDir = path.join(root, ".cache/cpu6502/klaus", REVISION);
  const targets = binaryOnly ? PINNED_FILES.slice(0, 1) : PINNED_FILES;

  async function prepareEntry(entry: PinnedFile): Promise<string> {
    const target = path.join(cacheDir, entry.relPath);
    let raw: Uint8Array;
    if (fs.existsSync(target)) {
      raw = fs.readFileSync(target);
    } else if (checkOnly) {
      throw new Error(`${entry.relPath}: file missing in cache (${target})`);
    } else {
      fs.mkdirSync(path.dirname(target), { recursive: true });
      raw = await fetchCapped(`${RAW_BASE_URL}/${entry.relPath}`, entry.size * 2 + 1);
    }
    if (raw.length !== entry.size) {
      throw new Error(`${entry.relPath}: size expected ${entry.size}, got ${raw.length}`);
    }
    checkedHash(raw, entry.sha256, entry.relPath);
    if (!fs.existsSync(target)) {
      writeAtomic(target, raw);
    }
    return `${entry.relPath} (${entry.size} B)`;
  }

  const results = await mapLimit(targets, 4, prepareEntry);

  if (!binaryOnly) {
    await prepareDecimal(cacheDir, checkOnly);
  }

  console.log(
    `Verified Klaus Dormann NMOS test data @ ${REVISION}\n` +
      `Files (${results.length}): ${results.join(", ")}\n` +
      `Cache directory: ${cacheDir}`,
  );
}

try {
  await main();
} catch (error) {
  console.error(`error: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
}
