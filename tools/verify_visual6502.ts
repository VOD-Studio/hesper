// Host-only reference driver. Loads unmodified upstream code from a hashed cache.
// No reference simulator/netlist is linked into or copied into the Rust CPU.
// Run with Bun: `bun tools/verify_visual6502.ts`. Bun executes TypeScript directly
// (types are stripped, not checked); no package.json/tsconfig.json is required.
import path from "node:path";
import vm from "node:vm";

const root = path.resolve(import.meta.dirname, "..");
const fixtures = path.join(root, "crates/cpu6502/tests/data/visual6502");

interface Manifest {
  revision: string;
  model: string;
  fixture_sha256: string;
  reset_fixture_sha256: string;
  reset_cases: number;
  reset_cycles: number;
  files: Record<string, string>;
}

const manifest: Manifest = JSON.parse(
  await Bun.file(path.join(fixtures, "manifest.json")).text(),
);
const cache = path.join(root, ".cache/cpu6502/visual6502", manifest.revision);
const sources: Array<[string, string]> = await Promise.all(
  Object.entries(manifest.files).map(
    async ([name, hash]): Promise<[string, string]> => {
      const bytes = await Bun.file(path.join(cache, name)).bytes();
      if (new Bun.CryptoHasher("sha256").update(bytes).digest("hex") !== hash)
        throw new Error(`${name}: hash mismatch`);
      return [name, new TextDecoder().decode(bytes)];
    },
  ),
);

// Pins the reference driver asserts/releases via scenario events. The upstream
// chip model also exposes many other node names (e.g. "clk0", "sync", "rw",
// "p0".."p7") that this driver only ever reads, never sets through events.
type Pin = "irq" | "nmi" | "rdy" | "so" | "res";
// [half-cycle index, pin, asserted]; asserted=true means the low-active pin is held low.
type Event = readonly [half: number, pin: Pin, asserted: boolean];

interface Spec {
  name: string;
  program: number[];
  pc?: number;
  p?: number;
  a?: number;
  x?: number;
  y?: number;
  s?: number;
  ram?: ReadonlyArray<readonly [number, number]>;
  events: Event[];
  cycles?: number;
}

interface Registers {
  pc: number;
  a: number;
  x: number;
  y: number;
  s: number;
  p: number;
  ram: Array<[number, number]>;
}

type CycleRecord = [
  address: number,
  data: number,
  direction: "read" | "write",
  sync: boolean,
];

interface ReferenceResult {
  name: string;
  initial: Registers;
  events: Event[];
  cycles: CycleRecord[];
}

// The upstream chipsim/macros files splice a large set of global functions and
// state (setupNodes, initChip, halfStep, readBit, memory, ...) into the vm
// context at runtime; there is no ambient declaration for them to type against.
type ChipContext = Record<string, any>;

function reference(spec: Spec): ReferenceResult {
  const c: ChipContext = vm.createContext({ console });
  for (const name of [
    "nodenames.js",
    "segdefs.js",
    "transdefs.js",
    "wires.js",
    "chipsim.js",
    "macros.js",
  ]) {
    const source = sources.find(([file]) => file === name);
    if (!source) throw new Error(`${name}: source missing from cache`);
    vm.runInContext(source[1], c, { filename: name });
  }
  // Disable presentation only, then call the upstream circuit setup and reset.
  vm.runInContext(
    "refresh=function(){};chipStatus=function(){};setCellValue=function(){};setupNodes();setupTransistors();",
    c,
  );
  // Bun's node:vm shim does not fully "contextify" the sandbox: a host-side
  // reassignment of a var-declared sandbox global (e.g. `c.memory = [...]`)
  // creates a value visible only to the host, decoupled from the internal
  // binding upstream functions (mRead/mWrite/readBits) actually close over.
  // Reassigning through a script keeps both sides pointing at the same array;
  // element-level reads/writes on that array (used everywhere below) then
  // propagate correctly in both directions under Bun and Node alike.
  vm.runInContext("memory = new Array(65536).fill(0xea);", c);
  const start = spec.pc ?? 0x8000;
  const p = spec.p ?? 0x20;
  const a = spec.a ?? 0,
    x = spec.x ?? 0,
    y = spec.y ?? 0,
    s = spec.s ?? 0xfd;
  const boot: number[] = [
    0xa2,
    s,
    0x9a,
    0xa2,
    x,
    0xa0,
    y,
    0xa9,
    p,
    0x48,
    0xa9,
    a,
    0x28,
    0x4c,
    start & 255,
    start >> 8,
  ];
  boot.forEach((value, i) => (c.memory[0x200 + i] = value));
  spec.program.forEach((value, i) => (c.memory[start + i] = value));
  [0, 0xa0, 0, 2, 0, 0x90].forEach(
    (value, i) => (c.memory[0xfffa + i] = value),
  );
  c.memory[0x9000] = c.memory[0xa000] = 0x40;
  c.initChip();
  c.setHigh("so");
  let ready = false;
  for (let half = 0; half < 200; half++) {
    if (
      !c.readBit("clk0") &&
      c.readBit("sync") &&
      c.readAddressBus() === start
    ) {
      ready = true;
      break;
    }
    c.halfStep();
  }
  if (!ready) throw new Error(`${spec.name}: bootstrap cycle budget exceeded`);
  for (const [address, value] of spec.ram ?? []) c.memory[address] = value;
  const initial: Registers = {
    pc: start,
    a: c.readA(),
    x: c.readX(),
    y: c.readY(),
    s: c.readSP(),
    p: [0, 1, 2, 3, 6, 7].reduce(
      (bits: number, i: number) => bits | (c.readBit(`p${i}`) << i),
      0x20,
    ),
    ram: c.memory.flatMap((value: number, address: number) =>
      value === 0xea ? [] : [[address, value]],
    ),
  };
  if (
    initial.pc !== c.readPC() ||
    initial.a !== a ||
    initial.x !== x ||
    initial.y !== y ||
    initial.s !== s ||
    initial.p !== p
  ) {
    throw new Error(`${spec.name}: unexpected bootstrap registers`);
  }
  const cycles: CycleRecord[] = [];
  for (let cycle = 0; cycle < (spec.cycles ?? 24); cycle++) {
    for (const [half, pin, asserted] of spec.events)
      if (half === cycle * 2) c[asserted ? "setLow" : "setHigh"](pin);
    c.halfStep(); // Low -> high: upstream bus writes occur here.
    cycles.push([
      c.readAddressBus(),
      c.readDataBus(),
      c.readBit("rw") ? "read" : "write",
      !!c.readBit("sync"),
    ]);
    for (const [half, pin, asserted] of spec.events)
      if (half === cycle * 2 + 1) c[asserted ? "setLow" : "setHigh"](pin);
    c.halfStep(); // High -> low: establish next address/read data.
  }
  return { name: spec.name, initial, events: spec.events, cycles };
}

const scenarios: Spec[] = [];
for (const pin of ["irq", "nmi"] as const) {
  for (const [family, program, pc] of [
    ["nop", [0xea, 0xea, 0xea], 0x8000],
    ["read", [0xad, 0, 0x20, 0xea], 0x8000],
    ["rmw", [0xee, 0, 0x20, 0xea], 0x8000],
    ["branch", [0xd0, 0, 0xea], 0x8000],
    ["branch-cross", [0xd0, 1, 0xea, 0xea], 0x80fd],
  ] as const)
    for (let at = 0; at < 7; at++) {
      scenarios.push({
        name: `${family}-${pin}-${at}`,
        program: [...program],
        pc,
        events: [[at * 2, pin, true]],
      });
    }
}
for (let at = 0; at < 7; at++) {
  scenarios.push({
    name: `brk-nmi-${at}`,
    program: [0, 0xea, 0xea],
    events: [[at * 2, "nmi", true]],
  });
  scenarios.push({
    name: `irq-nmi-${at}`,
    program: [0xea, 0xea],
    events: [
      [0, "irq", true],
      [(at + 2) * 2, "nmi", true],
    ],
  });
}
for (const first of [0x58, 0x28])
  for (const next of [0x78, 0x28, 0x40]) {
    scenarios.push({
      name: `i-combination-${first}-${next}`,
      p: 0x24,
      program: [first, next, 0xea],
      ram: [
        [0x1fe, 0x20],
        [0x1ff, 0x24],
        [0x100, 0x80],
        [0x101, 0x80],
      ],
      events: [[0, "irq", true]],
    });
  }
for (const pin of ["irq", "nmi"] as const)
  for (const width of [1, 2, 3]) {
    scenarios.push({
      name: `${pin}-pulse-${width}`,
      program: [0xea, 0xea, 0xea],
      events: [
        [0, pin, true],
        [width * 2, pin, false],
      ],
    });
  }

for (const [family, program] of [
  ["nop", [0xea, 0xea, 0xea]],
  ["rmw", [0xee, 0, 0x20, 0xea]],
  ["store", [0x8d, 0, 0x20, 0xea]],
] as const) {
  for (let half = 0; half < 12; half++) {
    scenarios.push({
      name: `rdy-${family}-${half}`,
      program: [...program],
      events: [
        [half, "rdy", true],
        [half + 8, "rdy", false],
      ],
    });
  }
}
for (const pin of ["irq", "nmi"] as const)
  for (const stall of [0, 1, 2])
    for (const at of [0, 2, 4]) {
      scenarios.push({
        name: `rdy-${pin}-${stall}-${at}`,
        program: [0xea, 0xea, 0xea],
        events: [
          [stall * 2, "rdy", true],
          [(stall + 4) * 2, "rdy", false],
          [at * 2, pin, true],
        ],
      });
    }
for (const [family, program] of [
  ["branch", [0xea, 0x50, 1, 0xea, 0xea]],
  ["clv", [0xb8, 0x50, 1, 0xea, 0xea]],
  ["adc", [0x69, 0, 0x50, 1, 0xea, 0xea]],
  ["bit", [0x24, 0x40, 0x50, 1, 0xea, 0xea]],
] as const) {
  for (let half = 0; half < 12; half++)
    scenarios.push({
      name: `so-${family}-${half}`,
      program: [...program],
      ram: [[0x40, 0]],
      events: [[half, "so", true]],
    });
}
for (const [family, program, ram] of [
  ["sbc", [0xe9, 0, 0x50, 1, 0xea, 0xea], []],
  ["plp", [0x28, 0x50, 1, 0xea, 0xea], [[0x1fe, 0x20]]],
  [
    "rti",
    [0x40, 0xea, 0x50, 1, 0xea, 0xea],
    [
      [0x1fe, 0x20],
      [0x1ff, 2],
      [0x100, 0x80],
    ],
  ],
] as const)
  for (let half = 0; half < 16; half++) {
    scenarios.push({
      name: `so-${family}-${half}`,
      program: [...program],
      ram: [...ram],
      events: [[half, "so", true]],
    });
  }

// Exercise RESET only after the bootstrap has reached the original test program.
// The handler records A/X/Y and pushes P before TSX/INX compute the reset SP.
const resetScenarios: Spec[] = [];
const resetHandler = [
  0x8d, 0x10, 0x20, 0x8e, 0x13, 0x20, 0x8c, 0x14, 0x20, 0x08, 0xba, 0xe8, 0x8e,
  0x11, 0x20, 0x68, 0x8d, 0x12, 0x20, 0x4c, 0x47, 0xb1,
];
const resetRam: Array<[number, number]> = [
  [0xfffc, 0x34],
  [0xfffd, 0xb1],
  [0x2000, 0x63],
  [0x2010, 0xcc],
  [0x2011, 0xcc],
  [0x2012, 0xcc],
  [0x2013, 0xcc],
  [0x2014, 0xcc],
  ...resetHandler.map((value, i): [number, number] => [0xb134 + i, value]),
];
const resetFamilies: Array<
  [string, number[], number, Array<[number, number]>, Event[]]
> = [
  ["nop", [0xea, 0xea], 2, [], []],
  ["read", [0xad, 0, 0x20, 0xea], 4, [], []],
  ["store", [0x8d, 0, 0x20, 0xea], 4, [], []],
  ["rmw", [0xee, 0, 0x20, 0xea], 6, [], []],
  ["pha", [0x48, 0xea], 3, [], []],
  ["jsr", [0x20, 0, 0x81, 0xea], 6, [[0x8100, 0x60]], []],
  ["pla", [0x68, 0xea], 4, [[0x1fe, 0x5a]], []],
  ["plp", [0x28, 0xea], 4, [[0x1fe, 0xc9]], []],
  [
    "rts",
    [0x60, 0xea],
    6,
    [
      [0x1fe, 0xff],
      [0x1ff, 0x80],
    ],
    [],
  ],
  [
    "rti",
    [0x40, 0xea],
    6,
    [
      [0x1fe, 0xc9],
      [0x1ff, 0],
      [0x100, 0x81],
    ],
    [],
  ],
  ["brk", [0, 0xea], 7, [], []],
  ["irq", [0xea, 0xea], 9, [], [[0, "irq", true]]],
  ["nmi", [0xea, 0xea], 9, [], [[0, "nmi", true]]],
];
function resetScenario(
  name: string,
  program: number[],
  events: Event[],
  ram: Array<[number, number]> = [],
  p = 0x29,
): void {
  resetScenarios.push({
    name: `reset-${name}`,
    program,
    ram: [...resetRam, ...ram],
    p,
    events,
    cycles: 64,
  });
}
resetScenarios.push({
  name: "reset-registers-stack-wrap",
  program: [0xea, 0xea],
  ram: resetRam,
  a: 0x12,
  x: 0x34,
  y: 0x56,
  s: 0x01,
  p: 0x29,
  events: [
    [0, "res", true],
    [8, "res", false],
  ],
  cycles: 64,
});
for (const [family, program, cycles, ram, interrupt] of resetFamilies) {
  for (let half = 0; half < cycles * 2; half++)
    for (const width of [8, 9]) {
      resetScenario(
        `${family}-${half}-${width}`,
        program,
        [...interrupt, [half, "res", true], [half + width, "res", false]],
        ram,
      );
    }
}
for (const half of [0, 1]) {
  resetScenario(`held-${half}`, [0xea, 0xea], [[half, "res", true]]);
  for (const release of [30, 31]) {
    resetScenario(
      `long-${half}-${release}`,
      [0xea, 0xea],
      [
        [half, "res", true],
        [release, "res", false],
      ],
    );
  }
}
for (const [family, program, cycles, ram] of resetFamilies.filter(
  ([family]) => ["nop", "store", "rmw", "brk"].includes(family),
)) {
  for (const half of [0, 1, cycles * 2 - 2, cycles * 2 - 1])
    for (const width of [1, 2, 3]) {
      resetScenario(
        `pulse-${family}-${half}-${width}`,
        program,
        [
          [half, "res", true],
          [half + width, "res", false],
        ],
        ram,
      );
    }
}
// Reassert from release synchronization through stack reads, vector reads and fetch.
for (let half = 6; half < 26; half++) {
  resetScenario(
    `retrigger-${half}`,
    [0xea, 0xea],
    [
      [0, "res", true],
      [4, "res", false],
      [half, "res", true],
      [half + 8, "res", false],
    ],
  );
}
for (const [family, program] of resetFamilies.filter(([family]) =>
  ["nop", "store", "rmw"].includes(family),
)) {
  for (const half of [0, 1, 4, 5, 12, 13, 20, 21, 22, 23, 24, 25]) {
    resetScenario(`rdy-${family}-${half}`, program, [
      [0, "res", true],
      [8, "res", false],
      [half, "rdy", true],
      [half + 8, "rdy", false],
    ]);
  }
}
for (const pin of ["irq", "nmi"] as const)
  for (const half of [0, 1, 6, 7, 8, 9, 14, 15, 20, 21, 22, 23, 24, 25]) {
    resetScenario(
      `${pin}-overlap-${half}`,
      [0xea, 0xea],
      [
        [0, "res", true],
        [8, "res", false],
        [half, pin, true],
        [half + 4, pin, false],
      ],
    );
  }

const usage =
  "Usage: bun tools/verify_visual6502.ts [--suite pins|reset|all] [--case NAME] [--record OUTPUT (requires --suite pins|reset, no --case)]";
type OptionFlag = "--suite" | "--case" | "--record";
const optionFlags: readonly OptionFlag[] = ["--suite", "--case", "--record"];
const options: Partial<Record<OptionFlag, string>> = {};
const args = Bun.argv.slice(2);
for (let i = 0; i < args.length; i += 2) {
  const flag = args[i] as OptionFlag;
  if (
    !optionFlags.includes(flag) ||
    !args[i + 1] ||
    args[i + 1].startsWith("--") ||
    options[flag]
  ) {
    throw new Error(usage);
  }
  options[flag] = args[i + 1];
}
const suite = options["--suite"] ?? "all";
if (
  !["pins", "reset", "all"].includes(suite) ||
  (options["--record"] && (suite === "all" || options["--case"]))
)
  throw new Error(usage);

interface Suite {
  name: "pins" | "reset";
  specs: Spec[];
  file: string;
  hash: string;
}

const suites: Suite[] = [
  {
    name: "pins",
    specs: scenarios,
    file: "pins.json",
    hash: manifest.fixture_sha256,
  },
  {
    name: "reset",
    specs: resetScenarios,
    file: "reset.json",
    hash: manifest.reset_fixture_sha256,
  },
].filter((s) => suite === "all" || suite === s.name);
const caseName = options["--case"];
if (
  caseName &&
  !suites.some((s) => s.specs.some((spec) => spec.name === caseName))
)
  throw new Error(
    `No reference case matches ${JSON.stringify(caseName)} in suite ${suite}`,
  );
for (const selected of suites) {
  const specs = selected.specs.filter(
    (spec) => !caseName || spec.name === caseName,
  );
  if (!specs.length) continue;
  let expected: string | undefined;
  if (!options["--record"]) {
    expected = await Bun.file(path.join(fixtures, selected.file)).text();
    if (
      new Bun.CryptoHasher("sha256").update(expected).digest("hex") !==
      selected.hash
    )
      throw new Error(`${selected.file}: fixture hash mismatch`);
  }
  const actual = specs.map(reference);
  const serialized =
    "[\n" + actual.map((c) => JSON.stringify(c)).join(",\n") + "\n]\n";
  if (options["--record"]) {
    await Bun.write(options["--record"], serialized);
    console.log(
      `Recorded ${actual.length} independent revD ${selected.name} traces to ${options["--record"]}`,
    );
  } else {
    const pinned: ReferenceResult[] = caseName
      ? JSON.parse(expected!).filter(
          (c: ReferenceResult) => c.name === caseName,
        )
      : JSON.parse(expected!);
    for (let i = 0; i < Math.max(actual.length, pinned.length); i++) {
      const observed = actual[i],
        baseline = pinned[i];
      if (JSON.stringify(observed) === JSON.stringify(baseline)) continue;
      const name = observed?.name ?? baseline.name;
      const replay = `bun tools/verify_visual6502.ts --suite ${selected.name} --case ${name}`;
      if (!observed || !baseline)
        throw new Error(
          `${selected.name}: case ${name} missing from ${observed ? "fixture" : "driver"}; replay: ${replay}`,
        );
      for (const field of ["name", "initial", "events"] as const) {
        if (
          JSON.stringify(observed[field]) !== JSON.stringify(baseline[field])
        ) {
          throw new Error(
            `${selected.name}: ${name} ${field}: expected ${JSON.stringify(baseline[field])}, actual ${JSON.stringify(observed[field])}; replay: ${replay}`,
          );
        }
      }
      for (
        let cycle = 0;
        cycle < Math.max(observed.cycles.length, baseline.cycles.length);
        cycle++
      ) {
        if (
          JSON.stringify(observed.cycles[cycle]) !==
          JSON.stringify(baseline.cycles[cycle])
        ) {
          throw new Error(
            `${selected.name}: ${name} cycle ${cycle}: expected ${JSON.stringify(baseline.cycles[cycle])}, actual ${JSON.stringify(observed.cycles[cycle])}; replay: ${replay}`,
          );
        }
      }
    }
    if (!caseName && serialized !== expected)
      throw new Error(
        `${selected.file}: fixture serialization differs despite identical observations`,
      );
    console.log(
      `Visual6502 revD @ ${manifest.revision}: ${actual.length} ${selected.name} traces reproduced${caseName ? ` (${caseName})` : ""}`,
    );
  }
}
