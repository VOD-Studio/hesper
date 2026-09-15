// The same public-interface checks run in Bun/Node.js and in a real browser.
function normalized(value) {
  return JSON.stringify(value, (_, item) => {
    if (typeof item === "bigint") return item.toString();
    if (item && typeof item === "object" && !Array.isArray(item)) {
      return Object.fromEntries(Object.keys(item).sort().map(key => [key, item[key]]));
    }
    return item;
  });
}
function equal(actual, expected, label = "value") {
  if (normalized(actual) !== normalized(expected)) {
    throw new Error(`${label}: expected ${normalized(expected)?.slice(0, 500)}, got ${normalized(actual)?.slice(0, 500)}`);
  }
}
function check(condition, label) {
  if (!condition) throw new Error(label);
}
function fails(call, code) {
  let error;
  try { call(); } catch (caught) { error = caught; }
  check(error instanceof Error, `expected a JS Error (${code})`);
  equal(error.name, "HesperError");
  equal(error.code, code);
  return error;
}

export function cases(Cpu6502Ram, Apple1, reference) {
  function cpuWith(program = [0xea, 0x4c, 0, 0x80]) {
    const cpu = new Cpu6502Ram();
    cpu.load(0x8000, Uint8Array.from(program));
    cpu.load(0xfffc, Uint8Array.of(0, 0x80));
    return cpu;
  }
  function echo() {
    const machine = new Apple1(Uint8Array.from(reference.apple1.rom), false);
    machine.loadRam(0, Uint8Array.from(reference.apple1.program));
    machine.reset();
    return machine;
  }
  function machineState(machine) {
    return {
      screen: Array.from(machine.screen()), cursor: machine.cursor(), registers: machine.registers(),
      masterTicks: machine.masterTicks(), cpuCycles: machine.cpuCycles(),
      videoFrames: machine.videoFrames(), ioPending: machine.ioPending(),
    };
  }

  return [
    ["CPU: native bus trace including RMW, RDY, SO, NMI and physical RESET", () => {
      const cpu = new Cpu6502Ram();
      try {
        const r = reference.cpu;
        cpu.load(0x8000, Uint8Array.from(r.program));
        cpu.load(0xfffa, Uint8Array.from(r.vectors));
        cpu.beginReset();
        const trace = [];
        for (let at = 0; at < r.trace.length; at++) {
          for (const [index, pin, value] of r.events) if (at === index) cpu[pin](value);
          trace.push(cpu.cycle());
        }
        equal(trace, r.trace, "bus trace");
        equal(cpu.registers(), r.registers);
        equal(cpu.readMemory(0x200, 1)[0], r.memory);
        check(trace.some(c => c.stalled), "reference exercises RDY");
        check(trace.some(c => c.bus.direction === "write"), "reference exercises writes");
      } finally { cpu.free(); }
    }],
    ["CPU: half cycles, bounded step resume, snapshots and batch equivalence", () => {
      const a = cpuWith(), b = cpuWith();
      try {
        equal(a.reset().cycles, 7n);
        b.reset();
        const snapshot = a.debugState();
        equal(snapshot.nextClockPhase, "Phi1");
        equal(snapshot.pins, { irq: false, nmi: false, reset: false, ready: true, so: false });
        equal(a.halfCycle(), undefined);
        equal(a.debugState().nextClockPhase, "Phi2");
        equal(a.halfCycle(), b.cycle());
        a.setReady(false);
        const error = fails(() => a.step(3), "CycleBudgetExceeded");
        equal(error.budget, 3n);
        a.setReady(true);
        a.step(7);
        b.step(7);
        equal(a.registers(), b.registers());
        check(snapshot.registers.pc === 0x8000, "snapshot stays detached");
        // Both now have the same architectural state at an instruction boundary.
        const batch = a.runCycles(25);
        for (let i = 0; i < 25; i++) b.cycle();
        equal(batch.cycles, 25);
        check(batch.completedSteps > 0, "batch counts completed instructions");
        equal(a.registers(), b.registers());
        equal(a.debugState().execution, b.debugState().execution);
      } finally { a.free(); b.free(); }
    }],
    ["CPU: invalid inputs do not wrap or mutate RAM; copies and instances are isolated", () => {
      const a = cpuWith(), b = cpuWith();
      try {
        a.load(0xffff, Uint8Array.of(0x5a));
        fails(() => a.load(0xffff, Uint8Array.of(1, 2)), "InvalidLoad");
        equal(a.readMemory(0xffff, 1)[0], 0x5a);
        for (const value of [-1, 65536, 0.5, NaN, Infinity, -Infinity]) {
          fails(() => a.load(value, Uint8Array.of(1)), "InvalidArgument");
        }
        for (const value of [-1, 0, 1.5, NaN, Infinity, 1_000_001]) {
          fails(() => a.runCycles(value), "InvalidArgument");
          fails(() => a.step(value), "InvalidArgument");
        }
        fails(() => a.readMemory(65535, 2), "InvalidRange");
        fails(() => a.readMemory(0, -1), "InvalidArgument");
        const memory = a.readMemory(0, 16);
        memory[0] = 33;
        equal(a.readMemory(0, 1)[0], 0);
        equal(b.readMemory(0xffff, 1)[0], 0);
      } finally { a.free(); b.free(); }
    }],
    ["CPU: unsupported opcode is a structured JS exception", () => {
      const cpu = cpuWith([0x02]);
      try {
        cpu.reset();
        const error = fails(() => cpu.cycle(), "UnsupportedOpcode");
        equal(error.address, 0x8000);
        equal(error.opcode, 0x02);
      } finally { cpu.free(); }
    }],
    ["Apple I: native screen, board clocks, video samples and batch equivalence", () => {
      const a = echo(), b = echo();
      try {
        const r = reference.apple1;
        a.typeText("a\r\nB"); // normalize to the reference's a\rB
        b.typeText(r.text);
        let hash = 2166136261;
        let refreshSeen = false;
        for (let i = 0; i < r.videoPrefix; i++) {
          const tick = a.tick(), v = tick.video;
          const bits = +v.luminance | (+v.sync << 1) | (+v.hsync << 2) | (+v.vsync << 3) | (+v.dotEdge << 4);
          hash = Math.imul(hash ^ bits, 16777619) >>> 0;
          if (tick.refresh) { equal(tick.cpu, null); refreshSeen = true; }
        }
        equal(hash, r.videoHash, "digital video prefix");
        check(refreshSeen, "refresh suppresses CPU bus cycles");
        equal(Array.from(a.runTicks(r.ticks - r.videoPrefix)), r.output);
        const output = [];
        for (const ticks of [1, 13, 997, 100001, 898988]) output.push(...b.runTicks(ticks));
        equal(output, r.output);
        const expected = Object.fromEntries(Object.keys(machineState(a)).map(key => [key, r[key]]));
        equal(machineState(a), expected, "native machine state");
        equal(machineState(a), machineState(b), "batch independence");
        check(typeof a.masterTicks() === "bigint", "clock uses bigint");
        equal(a.screen().length, 960);
        equal(a.drainOutput().length, 0);
      } finally { a.free(); b.free(); }
    }],
    ["Apple I: RESET and CLEAR SCREEN remain distinct; screen copies are detached", () => {
      const a = echo(), b = echo();
      try {
        a.typeText("A");
        for (let n = 0; n < 2; n++) a.runTicks(1_000_000);
        const screen = a.screen();
        check(screen.includes(65), "echo reached screen");
        a.typeChar(66);
        a.reset();
        equal(Array.from(a.screen()), Array.from(screen), "reset preserves screen");
        check(a.ioPending(), "reset preserves pending input");
        const ticks = a.masterTicks(), cycles = a.cpuCycles(), registers = a.registers();
        a.clearScreen();
        equal(a.masterTicks(), ticks);
        equal(a.cpuCycles(), cycles);
        equal(a.registers(), registers);
        check(a.screen().every(byte => byte === 32), "clear makes blank screen");
        check(screen.includes(65), "old screen snapshot preserved");
        check(b.screen().every(byte => byte === 32), "other instance unaffected");
        a.setResetLine(true);
        a.runTicks(100);
        a.setResetLine(false);
        a.runTicks(1000);
      } finally { a.free(); b.free(); }
    }],
    ["Apple I: ROM, mapped RAM, text and cumulative queue limits", () => {
      fails(() => new Apple1(new Uint8Array(255), false), "InvalidRom");
      const a = echo(), expanded = new Apple1(new Uint8Array(256), true);
      try {
        fails(() => a.loadRam(0x1000, Uint8Array.of(1)), "InvalidLoad");
        fails(() => a.loadRam(0x0fff, Uint8Array.of(1, 2)), "InvalidLoad");
        fails(() => a.loadRam(0xd010, Uint8Array.of(1)), "InvalidLoad");
        fails(() => a.loadRam(0xffff, Uint8Array.of(1)), "InvalidLoad");
        a.loadRam(0xe000, Uint8Array.of(1, 2));
        expanded.loadRam(0x1000, Uint8Array.of(1, 2));
        fails(() => a.typeText("A中文"), "InvalidText");
        check(!a.ioPending(), "invalid text queues no prefix");
        for (const value of [-1, 256, 0.5, NaN, Infinity]) fails(() => a.typeChar(value), "InvalidArgument");
        for (const value of [0, -1, 0.5, NaN, Infinity, 1_000_001]) fails(() => a.runTicks(value), "InvalidArgument");
        a.typeText("A".repeat(4096));
        fails(() => a.typeChar(66), "InputLimit");
        fails(() => a.typeText("B"), "InputLimit");
        a.reset();
        fails(() => a.typeChar(66), "InputLimit");
        expanded.typeText("B"); // independent queue
      } finally { a.free(); expanded.free(); }
    }],
    ["Apple I: output preceding a CPU error can be drained", () => {
      const a = echo();
      try {
        // Real PIA output handshake, followed by an unsupported opcode.
        a.loadRam(0, Uint8Array.from([
          0xa9,0x7f,0x8d,0x12,0xd0,0xa9,0x27,0x8d,0x13,0xd0,
          0xa9,0x41,0x8d,0x12,0xd0,0x2c,0x12,0xd0,0x30,0xfb,0x02,
        ]));
        a.reset();
        const error = fails(() => a.runTicks(1_000_000), "UnsupportedOpcode");
        equal(error.address, 20);
        equal(Array.from(a.drainOutput()), [65]);
        equal(a.drainOutput().length, 0);
      } finally { a.free(); }
    }],
  ];
}
