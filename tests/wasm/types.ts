// Compile against the shipped declarations, including ordinary JS snapshots.
import init, { Cpu6502Ram, type RegistersSnapshot, type CycleSnapshot } from "../../target/wasm-packages/cpu6502/web/hesper_cpu6502.js";
import initApple, { Apple1, type TickSnapshot } from "../../target/wasm-packages/apple1/web/hesper_apple1.js";
import { Cpu6502Ram as NodeCpu } from "../../target/wasm-packages/cpu6502/nodejs/hesper_cpu6502.js";
import { Apple1 as NodeApple } from "../../target/wasm-packages/apple1/nodejs/hesper_apple1.js";
import initRiscv, { RiscvLinux } from "../../target/wasm-packages/riscv-linux/web/hesper_riscv_linux.js";
import { RiscvLinux as NodeRiscv } from "../../target/wasm-packages/riscv-linux/nodejs/hesper_riscv_linux.js";

async function example(rom: Uint8Array) {
  await Promise.all([init(), initApple(), initRiscv()]);
  const cpu = new Cpu6502Ram(), apple = new Apple1(rom, false);
  const nodeCpu = new NodeCpu(), nodeApple = new NodeApple(rom, true);
  const registers: RegistersSnapshot = cpu.registers();
  const cycle: CycleSnapshot | undefined = cpu.halfCycle();
  const tick: TickSnapshot = apple.tick();
  const stepCycles: bigint = cpu.step(7).cycles;
  const ticks: bigint = apple.masterTicks();
  const screen: Uint8Array = apple.screen();
  const visible: boolean = apple.cursor().visible;
  const completed: number = cpu.runCycles(50).completedSteps;
  const pending: boolean | null = cpu.debugState().pendingVWrites[0];
  // @ts-expect-error snapshots have numeric registers, not strings
  const invalid: string = registers.a;
  // @ts-expect-error execution budgets are validated JS numbers, not bigint
  cpu.runCycles(10n);
  void [cycle, tick, stepCycles, ticks, screen, visible, completed, pending, invalid];
  cpu.free(); apple.free(); nodeCpu.free(); nodeApple.free();
  const linux = RiscvLinux.bare(0x80000000n, Uint8Array.of(0x93, 0, 0x10, 0));
  const spent: number = linux.run(1);
  const reg: bigint = linux.register(1), pc: bigint = linux.pc, time: bigint = linux.time;
  const uart: Uint8Array = linux.receive(4096);
  // @ts-expect-error RV64 addresses must not pass through a JS number
  linux.word(0x80000000);
  void [spent, reg, pc, time, uart, NodeRiscv];
  linux.free();
}
void example;
