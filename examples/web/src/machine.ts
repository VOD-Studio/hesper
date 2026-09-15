import type {
  RegistersSnapshot,
  CycleSnapshot,
  StepSnapshot,
} from "./generated/cpu6502/hesper_cpu6502";
import catalogue from "./generated/catalogue.json";

export const presets = catalogue.presets;
export const demo = catalogue;
export type Preset = (typeof presets)[number];
export type Mode = "apple1" | "cpu";
export type Registers = RegistersSnapshot;
export type Block = { address: number; bytes: Uint8Array };
export type Boot = {
  rom: Uint8Array;
  expansion: boolean;
  blocks: Block[];
  name: string;
  startup: string;
  presetId?: string;
};
export type Command =
  | { type: "boot"; boot: Boot }
  | { type: "gate"; mode: Mode; blocked: boolean }
  | { type: "toggle"; mode: Mode }
  | { type: "reset"; mode: Mode }
  | { type: "reboot" | "clear" | "stop" }
  | { type: "text"; text: string }
  | { type: "key"; byte: number }
  | { type: "cpu"; action: "restart" | "step" | "cycle" }
  | { type: "memory"; address: number };
export type Trace = {
  cycle: number;
  bus: CycleSnapshot["bus"];
  stalled: boolean;
  completed: StepSnapshot | null;
};
export type Snapshot = {
  mode: Mode;
  loaded: boolean;
  running: boolean;
  paused: boolean;
  done: boolean;
  registers: Registers | null;
  cycles: bigint;
  ticks: bigint;
  frames: bigint;
  screen?: Uint8Array;
  cursor?: { row: number; column: number; visible: boolean };
  pending?: boolean;
  name: string;
  startup: string;
  presetId?: string;
  expansion?: boolean;
  result?: Uint8Array;
  memory?: Uint8Array;
  memoryAddress?: number;
  trace?: Trace[];
  steps?: number;
};
export type Reply =
  | { type: "snapshot"; snapshot: Snapshot }
  | { type: "ready" }
  | { type: "notice"; text: string; error?: boolean };
export const hex = (value: number, width = 2) =>
  value.toString(16).toUpperCase().padStart(width, "0");
export function parseAddress(text: string) {
  const value = text.trim();
  if (!/^(?:\$[0-9a-f]+|0x[0-9a-f]+|[0-9]+)$/i.test(value))
    throw new Error("地址支持十进制、$8000 或 0x8000。");
  const address = value.startsWith("$")
    ? Number.parseInt(value.slice(1), 16)
    : Number(value);
  if (!Number.isSafeInteger(address) || address < 0 || address > 65535)
    throw new Error("地址必须在 $0000–$FFFF 之间。");
  return address;
}
export function normalizePaste(text: string) {
  const normalized = text.replace(/\r\n?/g, "\n");
  let skipped = 0;
  const chars = Array.from(normalized).filter((char) => {
    if (char === "\n" || /^[\x20-\x7e]$/.test(char)) return true;
    skipped++;
    return false;
  });
  if (chars.length > 4096) throw new Error("一次最多输入 4096 个字符。");
  return { text: chars.join("").replace(/\n/g, "\r"), skipped };
}
export async function readBytes(path: string) {
  const response = await fetch(`${import.meta.env.BASE_URL}${path}`);
  if (!response.ok)
    throw new Error(`资源加载失败：${response.status} (${path})`);
  return new Uint8Array(await response.arrayBuffer());
}
export function blankSnapshot(mode: Mode): Snapshot {
  return {
    mode,
    loaded: false,
    running: false,
    paused: true,
    done: false,
    registers: null,
    cycles: 0n,
    ticks: 0n,
    frames: 0n,
    name: "",
    startup: "",
  };
}

// Match the TUI: non-printable host projections occupy one blank cell.
export function screenText(screen: Uint8Array) {
  if (screen.length !== 960)
    throw new Error("Apple-1 screen must contain 40 × 24 cells");
  return Array.from({ length: 24 }, (_, row) =>
    Array.from(screen.slice(row * 40, row * 40 + 40), (byte) =>
      byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : " ",
    ).join(""),
  ).join("\n");
}
