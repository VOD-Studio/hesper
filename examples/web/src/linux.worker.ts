import imageLock from "../../../tools/riscv-linux/images.lock.json";
import diskImageLock from "../../../tools/riscv-linux/images-blk.lock.json";
import alpineImageLock from "../../../tools/riscv-linux/images-alpine-lts.lock.json";
import alpineLegacyLock from "../../../tools/riscv-linux/images-alpine.lock.json";
import { initSync, RiscvLinux } from "./generated/riscv-linux/hesper_riscv_linux";
import wasmUrl from "./generated/riscv-linux/hesper_riscv_linux_bg.wasm?url";
import { validDiskSize } from "./linux-disk";
import { DEFAULT_INITRAMFS_NAME, validInitramfsSize } from "./linux-initramfs";
import type { LinuxCommand, LinuxReply, LinuxState } from "./linux-messages";

const FUEL_BATCH = 65_536;
const STALLED_TRAP_LIMIT = 32;
const SERIAL_BATCH = 4096;
const INPUT_CAPACITY = 256 * 1024;
const REPORT_INTERVAL_MS = 250;
const GUEST_MEMORY_MIB = 128;

const pins = new Map(imageLock.images.map((pin) => [pin.path, pin]));
const diskPins = new Map(diskImageLock.images.map((pin) => [pin.path, pin]));
const alpinePins = new Map(alpineImageLock.images.map((pin) => [pin.path, pin]));
const alpineLegacyPins = new Map(alpineLegacyLock.images.map((pin) => [pin.path, pin]));
const tasks = new MessageChannel();
const usedIds = new Set<number>();
let wasmInitialized = false;
let active: Session | undefined;

type ImagePin = (typeof imageLock.images)[number];
type SelectedImages = Map<string, { file: File; pin: ImagePin }>;
type BootCommand = Extract<LinuxCommand, { type: "boot" }>;
type InputQueue = { bytes: Uint8Array; start: number; length: number };
type Session = {
  id: number;
  abort: AbortController;
  state: LinuxState;
  pauseRequested: boolean;
  exportPending: boolean;
  machine?: RiscvLinux;
  input?: InputQueue;
  fuel: bigint;
  batchPending: boolean;
  nextReport: number;
};

function post(reply: LinuxReply) {
  if (reply.type === "stdout" || reply.type === "disk") {
    // Both Wasm exports return copies with independently owned JS buffers.
    self.postMessage(reply, { transfer: [reply.bytes.buffer as ArrayBuffer] });
  } else {
    self.postMessage(reply);
  }
}

function detail(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

class InvalidInitramfs extends Error {}

function selectFiles(files: File[], pins: ReadonlyMap<string, ImagePin>, custom: boolean) {
  const selected: SelectedImages = new Map();
  for (const file of files) {
    const pin = pins.get(file.name);
    if (!pin) throw new Error(`Unexpected Linux image: ${file.name}.`);
    if (selected.has(file.name)) throw new Error(`Duplicate Linux image: ${file.name}.`);
    if (!(custom && file.name === DEFAULT_INITRAMFS_NAME) && file.size !== pin.bytes) {
      throw new Error(`${file.name}: expected ${pin.bytes} bytes, got ${file.size}.`);
    }
    selected.set(file.name, { file, pin });
  }
  for (const name of pins.keys()) {
    if (!(custom && name === DEFAULT_INITRAMFS_NAME) && !selected.has(name)) {
      throw new Error(`Missing Linux image: ${name}.`);
    }
  }
  return selected;
}

async function verifiedImage(
  name: string,
  selected: SelectedImages,
  subtle: SubtleCrypto,
  signal: AbortSignal,
) {
  const image = selected.get(name);
  if (!image) throw new Error(`Missing pinned Linux image: ${name}.`);
  const { file, pin } = image;
  signal.throwIfAborted();
  let buffer: ArrayBuffer;
  let digest: ArrayBuffer;
  try {
    buffer = await file.arrayBuffer();
    signal.throwIfAborted();
    digest = await subtle.digest("SHA-256", buffer);
  } catch (error) {
    throw new Error(`Cannot read or verify ${name} (check file access and available memory): ${detail(error)}`);
  }
  signal.throwIfAborted();
  const hash = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (hash !== pin.sha256) throw new Error(`${name}: SHA-256 does not match the pinned image.`);
  return { bytes: new Uint8Array(buffer), end: pin.end };
}

async function initializeWasm(signal: AbortSignal) {
  if (wasmInitialized) return;
  try {
    const response = await fetch(wasmUrl, { signal });
    signal.throwIfAborted();
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const binary = await response.arrayBuffer();
    signal.throwIfAborted();
    const module = await WebAssembly.compile(binary);
    signal.throwIfAborted();
    // Synchronous finalization prevents an aborted load from replacing the
    // binding's module-level exports after another session has started.
    initSync({ module });
    wasmInitialized = true;
  } catch (error) {
    throw new Error(`Cannot load the Linux WebAssembly resource: ${detail(error)}`);
  }
}

function alpineImagePins(files: File[]) {
  // The two locked Images have distinct lengths. Verify all four hashes
  // against the same lock before starting the selected kernel.
  const kernel = files.find((file) => file.name === "Image");
  return kernel && kernel.size === alpineLegacyPins.get("Image")?.bytes ? alpineLegacyPins : alpinePins;
}

async function loadMachine({ profile, files, initramfs, disk }: BootCommand, signal: AbortSignal) {
  if (profile !== "standard" && profile !== "alpine") {
    throw new Error("Unknown Linux boot profile.");
  }
  if (profile === "alpine" && (initramfs !== undefined || disk !== undefined)) {
    throw new Error("The Alpine profile does not support a custom initramfs or data disk.");
  }
  if (initramfs && !validInitramfsSize(initramfs.size)) {
    throw new InvalidInitramfs("A custom initramfs must be non-empty and no larger than 16 MiB.");
  }
  if (disk && !validDiskSize(disk.size)) {
    throw new Error("A data disk must be a positive multiple of 512 bytes and no larger than 16 MiB.");
  }
  const imagePins = profile === "alpine" ? alpineImagePins(files) : disk ? diskPins : pins;
  const selected = selectFiles(files, imagePins, initramfs !== undefined);
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) throw new Error("SHA-256 verification requires a secure browser context (HTTPS or localhost).");
  await initializeWasm(signal);
  signal.throwIfAborted();

  // Read and hash one local file at a time, retaining just the verified bytes
  // required by the Wasm boot call. The main thread only holds File handles.
  const firmware = await verifiedImage("fw_jump.bin", selected, subtle, signal);
  signal.throwIfAborted();
  const kernel = await verifiedImage("Image", selected, subtle, signal);
  signal.throwIfAborted();
  const fdt = await verifiedImage("board.dtb", selected, subtle, signal);
  signal.throwIfAborted();
  // Never read or hash the pinned cpio when a replacement was selected.
  const initrd = initramfs
    ? await readLocal(initramfs, "custom initramfs", signal)
    : (await verifiedImage(DEFAULT_INITRAMFS_NAME, selected, subtle, signal)).bytes;
  signal.throwIfAborted();
  const diskBytes = disk ? await readLocal(disk, "data disk", signal) : undefined;
  signal.throwIfAborted();
  const firmwareEnd = BigInt(firmware.end);
  try {
    if (profile === "alpine") {
      return RiscvLinux.bootImafdc(firmware.bytes, firmwareEnd, kernel.bytes, initrd, fdt.bytes);
    }
    if (initramfs) {
      const locked = imagePins.get(DEFAULT_INITRAMFS_NAME);
      if (!locked) throw new Error("Linux profile lacks a pinned initramfs length.");
      return RiscvLinux.bootWithInitramfs(firmware.bytes, firmwareEnd, kernel.bytes, initrd, fdt.bytes, locked.bytes, diskBytes);
    }
    if (diskBytes) {
      return RiscvLinux.bootWithDisk(firmware.bytes, firmwareEnd, kernel.bytes, initrd, fdt.bytes, diskBytes);
    }
    return RiscvLinux.boot(firmware.bytes, firmwareEnd, kernel.bytes, initrd, fdt.bytes);
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "InvalidInitramfs") {
      throw new InvalidInitramfs(error.message);
    }
    throw new Error(`Cannot initialize the Linux guest (${GUEST_MEMORY_MIB} MiB guest RAM plus Wasm and image memory required): ${detail(error)}`);
  }
}

async function readLocal(file: File, kind: string, signal: AbortSignal) {
  signal.throwIfAborted();
  let buffer: ArrayBuffer;
  try {
    buffer = await file.arrayBuffer();
  } catch (error) {
    const message = `Cannot read ${kind} ${file.name}: ${detail(error)}`;
    throw kind === "custom initramfs" ? new InvalidInitramfs(message) : new Error(message);
  }
  signal.throwIfAborted();
  return new Uint8Array(buffer);
}

function report(session: Session) {
  const { id: sessionId, machine, state, fuel } = session;
  post({ type: "state", sessionId, state, time: machine?.time ?? 0n, retired: machine?.retired ?? 0n, fuel });
  session.nextReport = performance.now() + REPORT_INTERVAL_MS;
}

function schedule(session: Session) {
  if (session.batchPending) return;
  session.batchPending = true;
  tasks.port2.postMessage(session.id);
}

function setState(session: Session, state: LinuxState) {
  session.state = state;
  report(session);
  if (state === "running") schedule(session);
}

function finish(session: Session, reason?: string, invalidInitramfs = false) {
  if (active !== session) return;
  active = undefined;
  session.abort.abort();
  session.input = undefined;
  const machine = session.machine;
  session.machine = undefined;
  let time = 0n;
  let retired = 0n;
  if (machine) {
    try {
      try {
        time = machine.time;
        retired = machine.retired;
      } finally {
        machine.free();
      }
    } catch (error) {
      const cleanup = `Cannot release the Linux guest: ${detail(error)}`;
      reason = reason === undefined ? cleanup : `${reason} ${cleanup}`;
    }
  }
  const { fuel } = session;
  const sessionId = session.id;
  if (reason !== undefined) post({ type: "error", sessionId, message: reason, invalidInitramfs });
  post({ type: "state", sessionId, state: reason === undefined ? "ended" : "failed", time, retired, fuel });
}

async function boot(session: Session, command: BootCommand) {
  try {
    const machine = await loadMachine(command, session.abort.signal);
    if (session.abort.signal.aborted || active !== session) {
      machine.free();
      return;
    }
    session.machine = machine;
    post({ type: "ready", sessionId: session.id });
    setState(session, session.pauseRequested ? "paused" : "running");
  } catch (error) {
    finish(session, detail(error), error instanceof InvalidInitramfs);
  }
}

function enqueue(session: Session, bytes: Uint8Array) {
  if (bytes.length === 0) return;
  const queued = session.input?.length ?? 0;
  if (bytes.length > INPUT_CAPACITY - queued) {
    throw new Error(`Serial input queue exceeds ${INPUT_CAPACITY} bytes.`);
  }
  const input = (session.input ??= { bytes: new Uint8Array(INPUT_CAPACITY), start: 0, length: 0 });
  const end = (input.start + input.length) % INPUT_CAPACITY;
  const first = Math.min(bytes.length, INPUT_CAPACITY - end);
  input.bytes.set(bytes.subarray(0, first), end);
  if (first < bytes.length) input.bytes.set(bytes.subarray(first), 0);
  input.length += bytes.length;
  if (session.state === "waiting") setState(session, "running");
}

function feedInput(input: InputQueue | undefined, machine: RiscvLinux) {
  if (!input?.length) return 0;
  const length = Math.min(input.length, SERIAL_BATCH, INPUT_CAPACITY - input.start);
  const accepted = machine.send(input.bytes.subarray(input.start, input.start + length));
  input.start = (input.start + accepted) % INPUT_CAPACITY;
  input.length -= accepted;
  return accepted;
}

// There is at most one queued batch per session. Each port message yields to
// control/input tasks without the delay imposed on chained timers.
tasks.port1.onmessage = ({ data: id }: MessageEvent<number>) => {
  const session = active;
  if (!session || id !== session.id) return;
  session.batchPending = false;
  const machine = session.machine;
  if (!machine || session.state !== "running") return;
  try {
    const accepted = feedInput(session.input, machine);
    const spent = machine.run(FUEL_BATCH);
    session.fuel += BigInt(spent);
    const bytes = machine.receive(SERIAL_BATCH);
    const outputLength = bytes.length;
    if (outputLength !== 0) post({ type: "stdout", sessionId: id, bytes });

    if (machine.stalledTraps >= STALLED_TRAP_LIMIT) {
      throw new Error(`Guest stalled in a trap loop at PC 0x${machine.pc.toString(16)}.`);
    }
    const diskFault = machine.diskFault;
    if (diskFault !== undefined) throw new Error(`Guest data disk fault: ${diskFault}`);
    if (spent === 0 && outputLength === 0) {
      if (!session.input?.length) {
        setState(session, "waiting");
        return;
      }
      if (accepted === 0) throw new Error("Guest is idle with serial input still pending.");
    }
    if (performance.now() >= session.nextReport) report(session);
    schedule(session);
  } catch (error) {
    finish(session, `Linux execution failed: ${detail(error)}`);
  }
};

self.onmessage = ({ data: command }: MessageEvent<LinuxCommand>) => {
  if (command.type === "boot") {
    if (usedIds.has(command.sessionId)) return;
    usedIds.add(command.sessionId);
    if (active) finish(active);
    const session: Session = {
      id: command.sessionId,
      abort: new AbortController(),
      state: "loading",
      pauseRequested: false,
      exportPending: false,
      fuel: 0n,
      batchPending: false,
      nextReport: 0,
    };
    active = session;
    report(session);
    void boot(session, command);
    return;
  }

  const session = active;
  if (!session || command.sessionId !== session.id) return;
  switch (command.type) {
    case "stdin":
      try {
        enqueue(session, command.bytes);
      } catch (error) {
        finish(session, `Cannot queue serial input: ${detail(error)}`);
      }
      break;
    case "pause":
      session.pauseRequested = true;
      if (session.state === "running" || session.state === "waiting") setState(session, "paused");
      break;
    case "resume":
      if (session.exportPending) break;
      session.pauseRequested = false;
      if (session.state === "paused") setState(session, "running");
      break;
    case "export":
      if (session.state === "paused" && !session.exportPending && session.machine) {
        try {
          const bytes = session.machine.exportDisk();
          if (bytes) {
            session.exportPending = true;
            post({ type: "disk", sessionId: session.id, bytes: bytes as Uint8Array<ArrayBuffer> });
          }
        } catch (error) {
          finish(session, `Cannot export data disk: ${detail(error)}`);
        }
      }
      break;
    case "exported":
      if (session.exportPending) {
        session.machine?.markDiskExported();
        session.exportPending = false;
      }
      break;
    case "stop":
      session.exportPending = false;
      if (!command.discard && session.machine?.diskDirty) {
        setState(session, "paused");
        post({ type: "discard-required", sessionId: session.id });
      } else {
        finish(session);
      }
      break;
  }
};
