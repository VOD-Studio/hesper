import { Apple1 } from "./generated/apple1/hesper_apple1";
import { Cpu6502Ram } from "./generated/cpu6502/hesper_cpu6502";
import {
  blankSnapshot,
  demo,
  type Boot,
  type Command,
  type Mode,
  type Reply,
  type Trace,
} from "./machine";

// The browser worker and regression tests drive the same session controller.
export function createRuntime(send: (reply: Reply) => void) {
  let apple: Apple1 | undefined;
  let boot: Boot | undefined;
  let cpu: Cpu6502Ram | undefined;
  let active: Mode = "apple1";
  let blocked = false;
  const paused = { apple1: false, cpu: true };
  let cpuCycles = 0,
    cpuSteps = 0,
    done = false,
    memoryAddress = 0x200;
  const trace: Trace[] = [];
  const notice = (text: string, error = false) =>
    send({ type: "notice", text, error });
  function snapshot(mode: Mode) {
    const base = {
      ...blankSnapshot(mode),
      paused: paused[mode],
      running: !paused[mode] && !blocked && active === mode,
    };
    if (mode === "apple1" && apple) {
      send({
        type: "snapshot",
        snapshot: {
          ...base,
          loaded: true,
          registers: apple.registers(),
          cycles: apple.cpuCycles(),
          ticks: apple.masterTicks(),
          frames: apple.videoFrames(),
          screen: apple.screen(),
          cursor: apple.cursor(),
          pending: apple.ioPending(),
          name: boot!.name,
          startup: boot!.startup,
          presetId: boot!.presetId,
          expansion: boot!.expansion,
        },
      });
    } else if (mode === "cpu" && cpu) {
      send({
        type: "snapshot",
        snapshot: {
          ...base,
          loaded: true,
          running: base.running && !done,
          done,
          registers: cpu.registers(),
          cycles: BigInt(cpuCycles),
          steps: cpuSteps,
          result: cpu.readMemory(0x200, 10),
          memory: cpu.readMemory(
            memoryAddress,
            Math.min(256, 65536 - memoryAddress),
          ),
          memoryAddress,
          trace: trace.slice(),
          name: "Count to ten",
          startup: "",
        },
      });
    } else send({ type: "snapshot", snapshot: { ...base, running: false } });
  }
  function cpuCycle() {
    if (!cpu || done) return;
    if (cpuCycles >= 10000)
      throw new Error("演示达到 10,000 周期上限。请 RESET 或重新运行。");
    const record = cpu.cycle();
    cpuCycles++;
    trace.push({ cycle: cpuCycles, ...record });
    if (trace.length > 256) trace.shift();
    if (record.completed) {
      if (record.completed.kind === "instruction") cpuSteps++;
      if (cpu.registers().pc === demo.demo_done) {
        done = true;
        paused.cpu = true;
        notice("计数演示完成：$0200–$0209 已写入 00–09。");
      }
    }
    return record;
  }
  function resetCpu(recreate: boolean) {
    if (recreate || !cpu) {
      cpu?.free();
      cpu = new Cpu6502Ram();
      cpu.load(demo.demo_start, Uint8Array.from(demo.demo));
      cpu.load(
        0xfffc,
        Uint8Array.of(demo.demo_start & 255, demo.demo_start >> 8),
      );
    }
    done = false;
    cpuCycles = 0;
    cpuSteps = 0;
    trace.length = 0;
    paused.cpu = true;
    cpu.beginReset();
    for (let i = 0; i < 7; i++) cpuCycle();
  }
  function startApple(config: Boot) {
    const next = new Apple1(config.rom, config.expansion);
    try {
      for (const block of config.blocks)
        next.loadRam(block.address, block.bytes);
      next.reset();
    } catch (error) {
      next.free();
      throw error;
    }
    apple?.free();
    apple = next;
    boot = config;
    paused.apple1 = false;
    notice(`已启动 ${config.name}。${config.startup}`);
  }
  function handle(command: Command) {
    try {
      switch (command.type) {
        case "boot":
          startApple(command.boot);
          break;
        case "gate":
          active = command.mode;
          blocked = command.blocked;
          break;
        case "toggle":
          if (command.mode === "apple1" && !apple) break;
          if (command.mode === "cpu" && done) break;
          paused[command.mode] = !paused[command.mode];
          break;
        case "reset":
          if (command.mode === "cpu") resetCpu(false);
          else if (apple) {
            apple.drainOutput();
            apple.reset();
          }
          notice(
            command.mode === "apple1"
              ? "物理 RESET 完成，RAM 与屏幕保留。"
              : "CPU RESET 完成，RAM 保留。",
          );
          break;
        case "reboot":
          if (boot) startApple(boot);
          break;
        case "clear":
          apple?.clearScreen();
          notice("屏幕已清空，CPU 状态保留。");
          break;
        case "stop":
          apple?.free();
          apple = undefined;
          boot = undefined;
          notice("已结束 Apple-1 会话。");
          break;
        case "text":
          if (!apple) throw new Error("请先启动 Apple-1。");
          apple.typeText(command.text);
          break;
        case "key":
          if (!apple) throw new Error("请先启动 Apple-1。");
          apple.typeChar(command.byte);
          break;
        case "memory":
          if (
            !Number.isInteger(command.address) ||
            command.address < 0 ||
            command.address > 65535
          )
            throw new Error("内存地址超出范围。");
          memoryAddress = command.address;
          break;
        case "cpu":
          paused.cpu = true;
          if (command.action === "restart") {
            resetCpu(true);
            paused.cpu = false;
          }
          if (command.action === "cycle") cpuCycle();
          if (command.action === "step") {
            for (let i = 0; i < 32 && !done; i++) {
              if (cpuCycle()?.completed) break;
            }
          }
          break;
      }
    } catch (error) {
      // Input/configuration errors do not destroy or pause an existing valid machine.
      notice(error instanceof Error ? error.message : String(error), true);
    }
    snapshot("apple1");
    snapshot("cpu");
  }

  function advance() {
    if (blocked || paused[active]) return;
    try {
      if (active === "apple1" && apple) {
        // At most ~8 ms of work before accepting the next input/control message.
        const start = performance.now();
        let ticks = 0;
        while (ticks < 240000 && performance.now() - start < 8) {
          apple.runTicks(20000);
          ticks += 20000;
        }
      } else if (active === "cpu" && cpu && !done) {
        for (let i = 0; i < 200 && !done; i++) cpuCycle();
      } else return;
      snapshot(active);
    } catch (error) {
      paused[active] = true;
      if (active === "apple1") apple?.drainOutput();
      notice(error instanceof Error ? error.message : String(error), true);
      snapshot(active);
    }
  }
  resetCpu(true);
  snapshot("cpu");
  snapshot("apple1");
  return {
    handle,
    advance,
    dispose() {
      apple?.free();
      cpu?.free();
    },
  };
}
