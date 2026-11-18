import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import {
  Keyboard,
  Pause,
  Play,
  Square,
  Terminal as TerminalIcon,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "./components/ui/button";
import type { LinuxCommand, LinuxReply, LinuxState } from "./linux-messages";
import "@xterm/xterm/css/xterm.css";
import "./LinuxWorkspace.css";

const imageNames = ["fw_jump.bin", "Image", "initramfs.cpio", "board.dtb"];
const inputChunkSize = 4096;
const encoder = new TextEncoder();

const labels: Record<LinuxState, string> = {
  loading: "校验镜像 / 装载中",
  running: "运行中",
  paused: "已暂停",
  waiting: "等待输入",
  failed: "运行失败",
  ended: "未运行",
};

type Status = { state: LinuxState; time?: bigint; retired?: bigint };
type Session = {
  id: number;
  worker: Worker;
  ready: boolean;
  stopping: boolean;
};
type Serial = { terminal: Terminal; dispose: () => void };

function openSerial(
  host: HTMLDivElement,
  send: (bytes: Uint8Array<ArrayBuffer>) => void,
): Serial {
  const terminal = new Terminal({
    disableStdin: true,
    cursorBlink: true,
    fontFamily: '"Geist Mono Variable", monospace',
    fontSize: 13,
    scrollback: 4000,
    theme: {
      background: "#101610",
      foreground: "#d6e6d1",
      cursor: "#b5db91",
      selectionBackground: "#435540",
    },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(host);
  terminal.onData((text) => send(encoder.encode(text)));
  terminal.onBinary((data) => {
    const bytes = new Uint8Array(data.length);
    for (let index = 0; index < bytes.length; index++) {
      bytes[index] = data.charCodeAt(index);
    }
    send(bytes);
  });
  const observer = new ResizeObserver(() => {
    if (host.clientWidth && host.clientHeight) fit.fit();
  });
  observer.observe(host);
  return {
    terminal,
    dispose() {
      observer.disconnect();
      terminal.dispose();
    },
  };
}

function postInput({ id, worker }: Session, bytes: Uint8Array<ArrayBuffer>) {
  const length = bytes.length;
  for (let offset = 0; offset < length; offset += inputChunkSize) {
    const chunk = length <= inputChunkSize
      ? bytes
      : bytes.slice(offset, offset + inputChunkSize);
    const command: LinuxCommand = { type: "stdin", sessionId: id, bytes: chunk };
    worker.postMessage(command, [chunk.buffer]);
  }
}

export default function LinuxWorkspace({ hidden }: { hidden: boolean }) {
  const [files, setFiles] = useState<File[]>([]);
  const [status, setStatus] = useState<Status>({ state: "ended" });
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const [hasTerminal, setHasTerminal] = useState(false);
  const host = useRef<HTMLDivElement>(null);
  const serial = useRef<Serial | null>(null);
  const session = useRef<Session | null>(null);
  const nextId = useRef(0);

  const releaseSession = useCallback(() => {
    const current = session.current;
    session.current = null;
    current?.worker.terminate();
    if (serial.current) serial.current.terminal.options.disableStdin = true;
  }, []);

  useEffect(() => () => {
    releaseSession();
    serial.current?.dispose();
    serial.current = null;
  }, [releaseSession]);

  function fail(message: string) {
    releaseSession();
    setError(message);
    setStopping(false);
    setStatus((previous) => ({ ...previous, state: "failed" }));
  }

  function start() {
    if (!host.current || session.current || files.length !== imageNames.length)
      return;
    const id = ++nextId.current;
    serial.current?.dispose();
    serial.current = null;
    setHasTerminal(false);
    setError("");
    setStatus({ state: "loading" });
    try {
      // Keep xterm's queued writes and decoder state isolated from the next boot.
      const tty = openSerial(host.current, (bytes) => {
        const current = session.current;
        if (current?.id === id && current.ready && !current.stopping) {
          postInput(current, bytes);
        }
      });
      serial.current = tty;
      const { terminal } = tty;
      setHasTerminal(true);
      const worker = new Worker(new URL("./linux.worker.ts", import.meta.url), {
        type: "module",
      });
      const current: Session = { id, worker, ready: false, stopping: false };
      session.current = current;
      worker.onmessage = ({ data }: MessageEvent<LinuxReply>) => {
        if (session.current !== current || data.sessionId !== id) return;
        switch (data.type) {
          case "ready":
            current.ready = true;
            terminal.options.disableStdin = false;
            break;
          case "stdout":
            terminal.write(data.bytes);
            break;
          case "state":
            setStatus({
              state: data.state,
              time: data.time,
              retired: data.retired,
            });
            if (data.state === "ended" || data.state === "failed") {
              releaseSession();
              setStopping(false);
            }
            break;
          case "error":
            current.ready = false;
            terminal.options.disableStdin = true;
            setError(data.message);
            break;
        }
      };
      worker.onerror = (event) => {
        if (session.current === current) {
          fail(event.message || "无法加载 Linux Worker。");
        }
      };
      worker.onmessageerror = () => {
        if (session.current === current) fail("无法读取 Linux Worker 消息。");
      };
      const command: LinuxCommand = { type: "boot", sessionId: id, files };
      worker.postMessage(command);
    } catch (reason) {
      fail(reason instanceof Error ? reason.message : String(reason));
    }
  }

  function control(type: "pause" | "resume" | "stop") {
    const current = session.current;
    if (!current || current.stopping) return;
    if (!current.ready) {
      // Termination cancels image reads and integrity checks without waiting for a reply.
      if (type === "stop") {
        releaseSession();
        setStatus({ state: "ended" });
      }
      return;
    }
    if (type === "stop") {
      current.stopping = true;
      if (serial.current) serial.current.terminal.options.disableStdin = true;
      setStopping(true);
    }
    const command: LinuxCommand = { type, sessionId: current.id };
    current.worker.postMessage(command);
  }

  const active = status.state !== "ended" && status.state !== "failed";
  const advancing = status.state === "running" || status.state === "waiting";
  const wrongCount = files.length !== 0 && files.length !== imageNames.length;

  return (
    <main className="linux-workspace" hidden={hidden}>
      <div className="page-heading">
        <div>
          <div className="eyebrow">
            <span>HESPER / MACHINES</span>
            <span className="heading-divider" />
            RV64 · UART
          </div>
          <h1>
            RISC-V Linux <span>用本地镜像启动串口会话。</span>
          </h1>
        </div>
      </div>
      <div className="toolbar linux-toolbar">
        <div className="toolbar-status">
          <span className={`status-dot ${advancing ? "live" : ""}`} />
          <span role="status" data-linux-state={status.state}>
            {stopping ? "正在停止" : labels[status.state]}
          </span>
        </div>
        <div className="run-controls">
          <Button
            data-linux-start
            size="sm"
            disabled={active || files.length !== imageNames.length}
            onClick={start}
          >
            <Play /> 启动
          </Button>
          <Button
            data-linux-pause
            variant="ghost"
            size="sm"
            disabled={!advancing || stopping}
            onClick={() => control("pause")}
          >
            <Pause /> 暂停
          </Button>
          <Button
            data-linux-resume
            variant="ghost"
            size="sm"
            disabled={status.state !== "paused" || stopping}
            onClick={() => control("resume")}
          >
            <Play /> 继续
          </Button>
          <Button
            data-linux-stop
            variant="ghost"
            size="sm"
            disabled={!active || stopping}
            onClick={() => control("stop")}
          >
            <Square /> {status.state === "loading" ? "取消启动" : "停止"}
          </Button>
        </div>
      </div>
      {error && (
        <p className="linux-error" data-linux-error role="alert">
          {error}
        </p>
      )}
      <div className="linux-workbench">
        <aside className="linux-details">
          <section aria-labelledby="linux-images-label">
            <h2 className="section-label" id="linux-images-label">
              本地启动镜像
            </h2>
            <p id="linux-files-help">
              请一次选择锁定版本的四个文件。浏览器只在本地读取并校验，不会上传。
            </p>
            <ul className="linux-images">
              {imageNames.map((name) => (
                <li key={name}>
                  <code>{name}</code>
                </li>
              ))}
            </ul>
            <input
              data-linux-files
              type="file"
              multiple
              aria-label="选择 Linux 启动镜像"
              aria-describedby="linux-files-help"
              aria-invalid={wrongCount}
              disabled={active}
              onChange={(event) =>
                setFiles(Array.from(event.currentTarget.files ?? []))
              }
            />
            {wrongCount && (
              <p role="alert">需要四个文件，已选 {files.length} 个。</p>
            )}
          </section>
          <section aria-labelledby="linux-metrics-label">
            <h2 className="section-label" id="linux-metrics-label">
              客体状态
            </h2>
            <dl className="linux-metrics">
              <div>
                <dt>虚拟时钟</dt>
                <dd data-linux-time={status.time?.toString()}>
                  {status.time?.toLocaleString() ?? "—"}
                </dd>
              </div>
              <div>
                <dt>已退役指令</dt>
                <dd data-linux-retired={status.retired?.toString()}>
                  {status.retired?.toLocaleString() ?? "—"}
                </dd>
              </div>
            </dl>
          </section>
        </aside>
        <section className="linux-console" aria-label="Linux 串口终端">
          <div className="linux-console-top">
            <span className="section-label">
              <TerminalIcon size={14} /> 串口终端 / ttyS0
            </span>
            <Button
              data-linux-focus
              variant="outline"
              size="sm"
              disabled={!hasTerminal}
              onClick={() => serial.current?.terminal.focus()}
            >
              <Keyboard /> 聚焦终端
            </Button>
          </div>
          <div className="linux-terminal-frame">
            <div className="linux-terminal" data-linux-terminal ref={host} />
          </div>
          <p className="linux-terminal-hint">
            键盘和粘贴直接送往串口；暂停时输入将保留至继续运行。
          </p>
        </section>
      </div>
    </main>
  );
}
