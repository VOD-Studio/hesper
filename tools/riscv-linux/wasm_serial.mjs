// Binary stdio host for the same Wasm package consumed by browser tooling.
import { createHash } from "node:crypto";
import { closeSync, fstatSync, openSync, readFileSync, readSync, writeSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { RiscvLinux } = require("../../target/wasm-packages/riscv-linux/nodejs/hesper_riscv_linux.js");
const lock = JSON.parse(readFileSync(new URL("images.lock.json", import.meta.url), "utf8"));
const LIMIT = 450_000_000;
const BATCH = 262_144;
const QUIT = 0x1d;

function image(directory, entry) {
  const path = join(directory, entry.path);
  const fd = openSync(path, "r");
  try {
    const stat = fstatSync(fd);
    if (!stat.isFile() || stat.size !== entry.bytes) throw new Error(`${path}: size mismatch`);
    const bytes = Buffer.allocUnsafe(entry.bytes);
    let count = 0;
    while (count < bytes.length) {
      const size = readSync(fd, bytes, count, bytes.length - count, null);
      if (!size) throw new Error(`${path}: truncated during read`);
      count += size;
    }
    if (readSync(fd, Buffer.alloc(1), 0, 1, null)) throw new Error(`${path}: size changed during read`);
    if (createHash("sha256").update(bytes).digest("hex") !== entry.sha256) {
      throw new Error(`${path}: SHA-256 mismatch`);
    }
    return bytes;
  } finally { closeSync(fd); }
}

function boot(directory) {
  const [fw, kernel, initrd, fdt] = lock.images.map(entry => image(directory, entry));
  return RiscvLinux.boot(fw, BigInt(lock.images[0].end), kernel, initrd, fdt);
}

const directory = process.argv[2];
if (!directory) throw new Error("usage: wasm_serial.mjs <verified image directory>");
const machine = boot(directory);
let fuel = 0, pending = null, queued = false, stopped = false;

function stop(reason, error) {
  if (stopped) return;
  stopped = true;
  process.stdin.destroy();
  console.error(`linux: exit=${reason} fuel=${fuel}/${LIMIT} retired=${machine.retired} time=${machine.time}`);
  if (error) {
    console.error(String(error));
    console.error(machine.snapshot());
    process.exitCode = 1;
  }
  machine.free();
}

function schedule() {
  if (queued || stopped) return;
  queued = true;
  setImmediate(() => {
    queued = false;
    if (stopped) return;
    try {
      if (pending) {
        const count = machine.send(pending);
        pending = pending.subarray(count);
        if (pending.length === 0) {
          pending = null;
          process.stdin.resume();
        }
      }
      const spent = machine.run(Math.min(BATCH, LIMIT - fuel));
      fuel += spent;
      for (;;) {
        const bytes = machine.receive(4096);
        if (!bytes.length) break;
        let offset = 0;
        while (offset < bytes.length) offset += writeSync(1, bytes, offset, bytes.length - offset);
      }
      if (machine.stalledTraps >= 32) stop("trap-loop", "consecutive traps without retirement");
      else if (fuel === LIMIT) stop("fuel", "guest fuel exhausted");
      else if (spent > 0) schedule();
      else if (pending) stop("serial", "guest cannot accept queued input");
    } catch (error) { stop("error", error); }
  });
}

process.stdin.on("data", bytes => {
  if (bytes.length === 1 && bytes[0] === QUIT) { stop("quit"); return; }
  if (bytes.length > 65_536 || pending) { stop("serial", "host input queue exceeded"); return; }
  process.stdin.pause();
  pending = bytes;
  schedule();
});
process.stdin.on("end", () => stop("eof"));
process.stdin.on("error", error => stop("error", error));
schedule();
