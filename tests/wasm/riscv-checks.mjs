const DRAM = 0x80000000n;

function program(words) {
  const bytes = new Uint8Array(words.length * 4);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < words.length; i++) view.setUint32(i * 4, words[i], true);
  return bytes;
}

function equal(actual, expected, field) {
  if (actual !== expected) throw new Error(`${field}: got ${actual}, expected ${expected}`);
}

function checkState(machine, expected) {
  for (const key of ["pc", "time", "retired"]) equal(machine[key], BigInt(expected[key]), key);
  equal(machine.satp, 0n, "Bare satp");
}

export function riscvCases(RiscvLinux, { integer, uart }) {
  return [
    ["RV64 unsigned addresses, retirement, x0 and DRAM match the native four instructions", () => {
      const bytes = program(integer.program);
      for (const batches of [[0, 4], [1, 3], [1, 1, 1, 1]]) {
        const machine = RiscvLinux.bare(DRAM, bytes);
        try {
          for (const budget of batches) equal(machine.run(budget), budget, "spent");
          checkState(machine, integer);
          for (let reg = 0; reg < 3; reg++) equal(machine.register(reg), BigInt(integer[`x${reg}`]), `x${reg}`);
          equal(machine.word(DRAM + 32n), integer.signature, "memory signature");
        } finally { machine.free(); }
      }
      const wide = RiscvLinux.bare(DRAM, program([0xfff00193])); // addi x3,x0,-1
      try {
        wide.run(1);
        equal(wide.register(3), 0xffffffffffffffffn, "full-width x3");
        equal(wide.exportDisk(), undefined, "bare disk export");
        equal(wide.diskFault, undefined, "bare disk fault");
        equal(wide.diskDirty, false, "bare disk dirty flag");
        wide.markDiskExported();
        equal(wide.diskDirty, false, "marking a bare export");
      } finally { wide.free(); }
      let rejected = false;
      try { RiscvLinux.bare(DRAM + 0x100000000n, bytes).free(); }
      catch { rejected = true; }
      equal(rejected, true, "out-of-RAM address must not truncate to Wasm32");
    }],
    ["disk boot rejects invalid raw lengths before validating boot image content", () => {
      // Pass the raw image length bounds but not the boot image content checks.
      const firmware = Uint8Array.of(0), kernel = new Uint8Array(64);
      const initramfs = new Uint8Array(6), fdt = new Uint8Array(40);
      const invalidLength = new Uint8Array(1);
      Object.defineProperty(invalidLength, "length", { value: 512 });
      for (const disk of [new Uint8Array(0), invalidLength, new Uint8Array(16 * 1024 * 1024 + 512)]) {
        let code;
        try { RiscvLinux.bootWithDisk(firmware, DRAM + 1n, kernel, initramfs, fdt, disk).free(); }
        catch (error) { code = error.code; }
        equal(code, "InvalidDisk", "invalid raw disk size");
      }
    }],
    ["queued UART bytes at the same virtual tick are invariant across Wasm batch sizes", () => {
      for (const batch of [1, 37, 4096]) {
        const machine = RiscvLinux.bare(DRAM, program(uart.program));
        try {
          const at = Number(uart.at);
          for (let left = at; left > 0; left -= batch) {
            const count = Math.min(left, batch);
            equal(machine.run(count), count, "advance to the fixed injection tick");
          }
          equal(machine.time, BigInt(uart.at), "injection tick");
          equal(machine.send(Uint8Array.from(uart.input)), uart.input.length, "accepted input bytes");
          equal(machine.time, BigInt(uart.at), "queueing input cannot advance time");
          equal(machine.receive(16).length, 0, "no host echo");
          let spent = 0;
          while (spent < 4096) {
            const count = machine.run(Math.min(batch, 4096 - spent));
            spent += count;
            if (count === 0) break;
          }
          equal(spent, uart.spent, "post-input fuel");
          checkState(machine, uart);
          equal([...machine.receive(16)].join(), uart.output.join(), "guest UART reply");
          equal(machine.receive(16).length, 0, "each UART byte drains only once");
        } finally { machine.free(); }
      }
    }],
  ];
}
