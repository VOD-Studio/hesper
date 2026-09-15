import { useState } from "react";
import {
  ArrowRight,
  Check,
  ChevronRight,
  Cpu,
  RotateCcw,
  Search,
  StepForward,
} from "lucide-react";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "./components/ui/tabs";
import {
  hex,
  parseAddress,
  type Command,
  type Registers,
  type Snapshot,
} from "./machine";

export function RegisterPanel({ registers }: { registers: Registers | null }) {
  return (
    <div className="register-panel">
      <div className="section-label">
        <Cpu size={14} /> CPU REGISTERS <span>NMOS 6502</span>
      </div>
      <div className="registers">
        {(["pc", "a", "x", "y", "sp", "status"] as const).map((key) => (
          <div key={key}>
            <span>{key === "status" ? "P" : key.toUpperCase()}</span>
            <strong>
              {registers ? hex(registers[key], key === "pc" ? 4 : 2) : "—"}
            </strong>
          </div>
        ))}
      </div>
      <div className="flags">
        {[
          ["N", 7],
          ["V", 6],
          ["D", 3],
          ["I", 2],
          ["Z", 1],
          ["C", 0],
        ].map(([name, bit]) => (
          <span
            key={name}
            className={
              registers && registers.status & (1 << Number(bit))
                ? "flag active"
                : "flag"
            }
          >
            {name}
            <i />
          </span>
        ))}
      </div>
    </div>
  );
}

export function CpuPanel({
  state,
  send,
  notify,
}: {
  state: Snapshot;
  send: (c: Command) => void;
  notify: (text: string, error?: boolean) => void;
}) {
  const [address, setAddress] = useState("$0200");
  const [traceTab, setTraceTab] = useState("instructions");
  const instructions = state.trace?.filter((row) => row.completed) ?? [];
  return (
    <div className="cpu-workspace">
      <section className="cpu-demo">
        <div className="demo-heading">
          <span className="eyebrow">BUILT-IN EXPERIMENT / 01</span>
          <span className={`result-badge ${state.done ? "passed" : ""}`}>
            {state.done ? (
              <>
                <Check size={13} />
                已完成
              </>
            ) : (
              "计数演示"
            )}
          </span>
        </div>
        <h2>
          Count to ten<span>.</span>
        </h2>
        <p>从第一条指令开始，看 6502 将 00–09 写入内存。</p>
        <div className="demo-flow">
          <span>
            $8000 <small>程序入口</small>
          </span>
          <ArrowRight size={20} />
          <span>
            $0200–$0209 <small>结果内存</small>
          </span>
          <ArrowRight size={20} />
          <span>
            $800F <small>停止地址</small>
          </span>
        </div>
        <div className="demo-controls">
          <Button
            onClick={() => send({ type: "cpu", action: "restart" })}
            disabled={!state.loaded}
          >
            <RotateCcw />
            {state.done ? "重新运行" : "运行演示"}
          </Button>
          <Button
            variant="outline"
            onClick={() => send({ type: "cpu", action: "step" })}
            disabled={!state.loaded || state.done}
          >
            <StepForward />
            指令单步
          </Button>
          <Button
            variant="ghost"
            onClick={() => send({ type: "cpu", action: "cycle" })}
            disabled={!state.loaded || state.done}
          >
            <ChevronRight />
            周期单步
          </Button>
        </div>
        <div className="result-bytes" aria-label="计数结果">
          {Array.from({ length: 10 }, (_, i) => (
            <div key={i}>
              <span>{hex(0x200 + i, 4)}</span>
              <b>{state.result ? hex(state.result[i]) : "··"}</b>
            </div>
          ))}
        </div>
      </section>
      <section className="inspector">
        <Tabs defaultValue="memory">
          <div className="inspector-top">
            <TabsList variant="line">
              <TabsTrigger value="memory">内存</TabsTrigger>
              <TabsTrigger value="trace">执行记录</TabsTrigger>
            </TabsList>
            <span className="micro-label">
              {state.steps ?? 0} INSTRUCTIONS · {state.cycles.toString()} CYCLES
            </span>
          </div>
          <TabsContent value="memory">
            <form
              className="memory-search"
              onSubmit={(event) => {
                event.preventDefault();
                try {
                  send({ type: "memory", address: parseAddress(address) });
                } catch (error) {
                  notify(String(error), true);
                }
              }}
            >
              <label htmlFor="memory-address">起始地址</label>
              <Input
                id="memory-address"
                value={address}
                onChange={(event) => setAddress(event.target.value)}
              />
              <Button
                variant="secondary"
                type="submit"
                aria-label="查看内存地址"
              >
                <Search size={14} />
              </Button>
              <span>256 bytes / 64 KiB RAM</span>
            </form>
            <div className="memory-scroll">
              <table className="memory-table">
                <thead>
                  <tr>
                    <th>ADDR</th>
                    {Array.from({ length: 16 }, (_, i) => (
                      <th key={i}>{hex(i, 1)}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {Array.from(
                    { length: Math.ceil((state.memory?.length ?? 0) / 16) },
                    (_, row) => (
                      <tr key={row}>
                        <th>{hex((state.memoryAddress ?? 0) + row * 16, 4)}</th>
                        {Array.from({ length: 16 }, (_, col) => {
                          const byte = state.memory?.[row * 16 + col];
                          return (
                            <td key={col} className={byte ? "nonzero" : ""}>
                              {byte === undefined ? "" : hex(byte)}
                            </td>
                          );
                        })}
                      </tr>
                    ),
                  )}
                </tbody>
              </table>
            </div>
          </TabsContent>
          <TabsContent value="trace">
            <div className="trace-toolbar">
              <Button
                size="sm"
                variant={traceTab === "instructions" ? "secondary" : "ghost"}
                onClick={() => setTraceTab("instructions")}
              >
                指令
              </Button>
              <Button
                size="sm"
                variant={traceTab === "bus" ? "secondary" : "ghost"}
                onClick={() => setTraceTab("bus")}
              >
                总线周期
              </Button>
              <span>保留最近 256 个周期</span>
            </div>
            <div className="trace-scroll">
              <table className="trace-table">
                <thead>
                  <tr>
                    {(traceTab === "bus"
                      ? ["CYCLE", "ADDRESS", "R/W", "DATA", "SYNC", "RDY"]
                      : ["CYCLE", "PC", "OPCODE", "A", "X", "Y", "SP"]
                    ).map((h) => (
                      <th key={h}>{h}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {(traceTab === "bus" ? (state.trace ?? []) : instructions)
                    .slice()
                    .reverse()
                    .map((row) => (
                      <tr key={row.cycle}>
                        {(traceTab === "bus"
                          ? [
                              row.cycle,
                              "$" + hex(row.bus.address, 4),
                              row.bus.direction === "read" ? "R" : "W",
                              hex(row.bus.data),
                              row.bus.sync ? "1" : "0",
                              row.stalled ? "STALL" : "—",
                            ]
                          : [
                              row.cycle,
                              "$" + hex(row.completed!.address, 4),
                              row.completed!.opcode === null
                                ? row.completed!.kind.toUpperCase()
                                : hex(row.completed!.opcode),
                              hex(row.completed!.after.a),
                              hex(row.completed!.after.x),
                              hex(row.completed!.after.y),
                              hex(row.completed!.after.sp),
                            ]
                        ).map((cell, i) => (
                          <td key={i}>{cell}</td>
                        ))}
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
          </TabsContent>
        </Tabs>
      </section>
    </div>
  );
}
