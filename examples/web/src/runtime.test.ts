import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import initApple from "./generated/apple1/hesper_apple1";
import initCpu from "./generated/cpu6502/hesper_cpu6502";
import { createRuntime } from "./runtime";
import {
  hex,
  normalizePaste,
  screenText,
  parseAddress,
  presets,
  type Boot,
  type Mode,
  type Reply,
  type Snapshot,
} from "./machine";

const bytes = (path: string) =>
  new Uint8Array(readFileSync(resolve(import.meta.dir, path)));
const rom = bytes("../public/machine/wozmon.bin");
function config(id = "basic-huston", expansion = false): Boot {
  const preset = presets.find((p) => p.id === id)!;
  return {
    rom,
    expansion,
    name: preset.name,
    startup: preset.startup,
    blocks: preset.blocks.map((b) => ({
      address: b.address,
      bytes: bytes(`../public/${b.path}`),
    })),
  };
}
const sessions: ReturnType<typeof createRuntime>[] = [];
function session() {
  const events: Reply[] = [];
  const runtime = createRuntime((event) => events.push(event));
  sessions.push(runtime);
  const state = (mode: Mode = "apple1") =>
    (
      events.findLast(
        (event) => event.type === "snapshot" && event.snapshot.mode === mode,
      ) as { snapshot: Snapshot }
    ).snapshot;
  const runUntil = (predicate: () => boolean, limit = 500) => {
    for (let i = 0; i < limit && !predicate(); i++) runtime.advance();
    expect(predicate()).toBe(true);
  };
  const screen = () => String.fromCharCode(...state().screen!);
  return { ...runtime, events, state, runUntil, screen };
}
beforeAll(async () => {
  await Promise.all([
    initApple({
      module_or_path: bytes("./generated/apple1/hesper_apple1_bg.wasm"),
    }),
    initCpu({
      module_or_path: bytes("./generated/cpu6502/hesper_cpu6502_bg.wasm"),
    }),
  ]);
});
afterAll(() => sessions.forEach((runtime) => runtime.dispose()));

describe("Web host with actual Wasm packages", () => {
  test("CPU demo, cycle/step, bounded trace and reset retain RAM", () => {
    const s = session();
    expect(s.state("cpu").registers!.pc).toBe(0x8000);
    expect(s.state("cpu").cycles).toBe(7n);
    s.handle({ type: "cpu", action: "cycle" });
    expect(s.state("cpu").cycles).toBe(8n);
    s.handle({ type: "cpu", action: "step" });
    expect(s.state("cpu").registers!.pc).toBe(0x8001);
    s.handle({ type: "gate", mode: "cpu", blocked: false });
    s.handle({ type: "toggle", mode: "cpu" });
    s.runUntil(() => s.state("cpu").done);
    expect(Array.from(s.state("cpu").memory!.slice(0, 10))).toEqual([
      0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
    ]);
    // Same 147 instruction cycles + 7 RESET cycles as crates/cli/tests/demo.rs.
    expect(s.state("cpu").cycles).toBe(154n);
    expect(s.state("cpu").steps).toBe(54);
    expect(s.state("cpu").trace!.length).toBeLessThanOrEqual(256);
    expect(
      s.state("cpu").trace!.some((row) => row.bus.direction === "write"),
    ).toBe(true);
    s.handle({ type: "reset", mode: "cpu" });
    expect(s.state("cpu").registers!.pc).toBe(0x8000);
    expect(s.state("cpu").memory![9]).toBe(9);
    s.handle({ type: "memory", address: 65535 });
    expect(s.state("cpu").memory!.length).toBe(1);
  });
  test("BASIC input, modal/background gating, pause, reset and reboot", () => {
    const s = session();
    s.handle({ type: "boot", boot: config() });
    s.handle({ type: "text", text: "E000R\rPRINT 123+456\r" });
    s.runUntil(() => /579/.test(s.screen()));
    expect(s.screen().length).toBe(960);
    const before = s.state().cycles;
    s.handle({ type: "gate", mode: "apple1", blocked: true });
    s.advance();
    expect(s.state().cycles).toBe(before);
    s.handle({ type: "gate", mode: "cpu", blocked: false });
    s.advance();
    expect(s.state().cycles).toBe(before);
    s.handle({ type: "gate", mode: "apple1", blocked: false });
    s.handle({ type: "toggle", mode: "apple1" });
    s.advance();
    expect(s.state().cycles).toBe(before);
    s.handle({ type: "clear" });
    expect(s.screen().trim()).toBe("");
    expect(s.state().cycles).toBe(before);
    s.handle({ type: "toggle", mode: "apple1" });
    s.handle({ type: "reset", mode: "apple1" });
    s.handle({ type: "text", text: "0000:42\r0000\r" });
    s.runUntil(() => s.screen().includes("0000: 42"));
    s.handle({ type: "reset", mode: "apple1" });
    s.handle({ type: "clear" });
    s.handle({ type: "text", text: "0000\r" });
    s.runUntil(() => s.screen().includes("0000: 42"));
    s.handle({ type: "reboot" });
    s.handle({ type: "text", text: "0000\r" });
    s.runUntil(() => s.screen().includes("0000: 00"));
  });
  test("failed replacement preserves session, bounded input fails atomically", () => {
    const s = session();
    s.handle({ type: "boot", boot: config() });
    const before = s.state();
    s.handle({
      type: "boot",
      boot: {
        ...config(),
        blocks: [{ address: 0xd010, bytes: Uint8Array.of(1) }],
      },
    });
    expect(s.state().name).toBe(before.name);
    expect(s.state().cycles).toBe(before.cycles);
    expect(s.events.some((e) => e.type === "notice" && e.error)).toBe(true);
    s.handle({ type: "text", text: "X".repeat(4097) });
    s.handle({ type: "text", text: "E000R\rPRINT 77+1\r" });
    s.runUntil(() => /78/.test(s.screen()));
  });
  test("all presets retain complete blocks; expansion requirement is enforced", () => {
    const s = session();
    expect(presets.length).toBe(42);
    for (const preset of presets) {
      const boot = config(preset.id, true);
      for (const [index, b] of boot.blocks.entries())
        expect(b.bytes.length).toBe(preset.blocks[index].size);
      s.handle({ type: "boot", boot });
      expect(s.state().name).toBe(preset.name);
    }
    s.handle({ type: "boot", boot: config("little-tower", false) });
    expect(s.events.at(-3)).toMatchObject({ type: "notice", error: true });
    s.handle({ type: "boot", boot: config("blackjack") });
    s.handle({ type: "text", text: "E2B3R\rLIST\r" });
    s.runUntil(() => /BLACKJACK/.test(s.screen()));
  });
  test("input normalization and addresses reject malformed or unbounded values", () => {
    expect(normalizePaste("a\r\nb\rc\n中文\t")).toEqual({
      text: "a\rb\rc\r",
      skipped: 3,
    });
    expect(() => normalizePaste("x".repeat(4097))).toThrow();
    for (const value of ["$e000", "0xE000", "57344"])
      expect(parseAddress(value)).toBe(0xe000);
    for (const value of ["-1", "0x10000", "1.5", "", "Infinity", "0x", "1e3"])
      expect(() => parseAddress(value)).toThrow();
    expect(hex(65535, 4)).toBe("FFFF");
    const cells = new Uint8Array(960).fill(65);
    cells[0] = 127;
    cells[40] = 10;
    const lines = screenText(cells).split("\n");
    expect(lines).toHaveLength(24);
    expect(lines.every((line) => line.length === 40)).toBe(true);
    expect(lines[0][0]).toBe(" ");
    expect(lines[1][0]).toBe(" ");
  });
});
