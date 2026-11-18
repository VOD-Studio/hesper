import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import {
  Download,
  Keyboard,
  Pause,
  Play,
  Square,
  Terminal as TerminalIcon,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "./components/ui/button";
import { validDiskSize } from "./linux-disk";
import { DEFAULT_INITRAMFS_NAME, validInitramfsSize } from "./linux-initramfs";
import type { LinuxCommand, LinuxProfile, LinuxReply, LinuxState } from "./linux-messages";
import "@xterm/xterm/css/xterm.css";
import "./LinuxWorkspace.css";

const baseImageNames = ["fw_jump.bin", "Image", "board.dtb"];
const defaultImageNames = [...baseImageNames, DEFAULT_INITRAMFS_NAME];
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
type Pending = "resuming" | "exporting" | "stopping" | null;
type DiskSelection = "empty" | "selected" | "invalid";
type Session = {
  id: number;
  worker: Worker;
  ready: boolean;
  state: LinuxState;
  hasDisk: boolean;
  pending: Pending;
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

function downloadDisk(bytes: Uint8Array<ArrayBuffer>) {
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
  try {
    const link = document.createElement("a");
    link.href = url;
    link.download = "hesper-data.raw";
    link.click();
  } finally {
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
}

export default function LinuxWorkspace({ hidden }: { hidden: boolean }) {
  const [profile, setProfile] = useState<LinuxProfile>("standard");
  const [files, setFiles] = useState<File[]>([]);
  const [initramfs, setInitramfs] = useState<File>();
  const [initramfsError, setInitramfsError] = useState("");
  const [diskSelection, setDiskSelection] = useState<DiskSelection>("empty");
  const [status, setStatus] = useState<Status>({ state: "ended" });
  const [pending, setPending] = useState<Pending>(null);
  const [error, setError] = useState("");
  const [hasTerminal, setHasTerminal] = useState(false);
  const filesInput = useRef<HTMLInputElement>(null);
  const initramfsInput = useRef<HTMLInputElement>(null);
  const diskInput = useRef<HTMLInputElement>(null);
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

  function updatePending(current: Session, action: Pending) {
    current.pending = action;
    setPending(action);
  }

  function selectInitramfs(input: HTMLInputElement) {
    const file = input.files?.[0];
    if (file && !validInitramfsSize(file.size)) {
      input.value = "";
      setInitramfs(undefined);
      setInitramfsError("自定义 initramfs 必须非空，且不得超过 16 MiB。");
    } else {
      setInitramfs(file);
      setInitramfsError("");
    }
  }

  function clearInitramfs() {
    if (initramfsInput.current) initramfsInput.current.value = "";
    setInitramfs(undefined);
    setInitramfsError("");
  }

  function selectDisk(input: HTMLInputElement) {
    const disk = input.files?.[0];
    if (disk && !validDiskSize(disk.size)) {
      input.value = "";
      setDiskSelection("invalid");
    } else {
      setDiskSelection(disk ? "selected" : "empty");
    }
  }

  function clearDisk() {
    if (diskInput.current) diskInput.current.value = "";
    setDiskSelection("empty");
  }

  function selectProfile(value: string) {
    if (session.current || value === profile || (value !== "standard" && value !== "alpine")) return;
    if (filesInput.current) filesInput.current.value = "";
    setFiles([]);
    clearInitramfs();
    clearDisk();
    setError("");
    if (status.state === "failed") setStatus({ state: "ended" });
    setProfile(value);
  }

  function fail(message: string) {
    releaseSession();
    setError(message);
    setPending(null);
    setStatus((previous) => ({ ...previous, state: "failed" }));
  }

  function start() {
    if (!host.current || session.current || !canStart) return;
    const disk = diskInput.current?.files?.[0];
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
        if (current?.id === id && current.ready && current.pending !== "stopping") {
          postInput(current, bytes);
        }
      });
      serial.current = tty;
      const { terminal } = tty;
      setHasTerminal(true);
      const worker = new Worker(new URL("./linux.worker.ts", import.meta.url), {
        type: "module",
      });
      const current: Session = {
        id,
        worker,
        ready: false,
        state: "loading",
        hasDisk: disk !== undefined,
        pending: null,
      };
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
            current.state = data.state;
            setStatus({
              state: data.state,
              time: data.time,
              retired: data.retired,
            });
            if (data.state === "ended" || data.state === "failed") {
              releaseSession();
              setPending(null);
            } else if (current.pending === "resuming" && data.state !== "paused") {
              updatePending(current, null);
            }
            break;
          case "disk": {
            // Stop invalidates the in-flight export even if its reply is queued.
            if (current.pending !== "exporting" || current.state !== "paused") break;
            downloadDisk(data.bytes);
            const command: LinuxCommand = { type: "exported", sessionId: id };
            worker.postMessage(command);
            updatePending(current, null);
            break;
          }
          case "discard-required": {
            if (current.pending !== "stopping") break;
            if (window.confirm("数据盘有未导出的修改。要丢弃修改并停止会话吗？")) {
              const command: LinuxCommand = { type: "stop", sessionId: id, discard: true };
              worker.postMessage(command);
            } else {
              terminal.options.disableStdin = false;
              updatePending(current, null);
            }
            break;
          }
          case "error":
            current.ready = false;
            terminal.options.disableStdin = true;
            if (data.invalidInitramfs) setInitramfsError(data.message);
            else setError(data.message);
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
      const command: LinuxCommand = { type: "boot", sessionId: id, profile, files, initramfs, disk };
      worker.postMessage(command);
    } catch (reason) {
      fail(reason instanceof Error ? reason.message : String(reason));
    }
  }

  function control(type: "pause" | "resume" | "stop" | "export") {
    const current = session.current;
    if (!current || current.pending === "stopping") return;
    if (!current.ready) {
      // Termination cancels image reads and integrity checks without waiting for a reply.
      if (type === "stop") {
        releaseSession();
        setStatus({ state: "ended" });
      }
      return;
    }
    if (type === "stop") {
      if (serial.current) serial.current.terminal.options.disableStdin = true;
      updatePending(current, "stopping");
    } else if (type === "resume" || type === "export") {
      if (current.state !== "paused" || current.pending || (type === "export" && !current.hasDisk)) {
        return;
      }
      updatePending(current, type === "export" ? "exporting" : "resuming");
    }
    const command: LinuxCommand = { type, sessionId: current.id };
    current.worker.postMessage(command);
  }

  const active = status.state !== "ended" && status.state !== "failed";
  const stopping = pending === "stopping";
  const advancing = status.state === "running" || status.state === "waiting";
  const requiredImages = profile === "standard" && initramfs ? baseImageNames : defaultImageNames;
  const selectedNames = new Set(files.map((file) => file.name));
  const missingImages = requiredImages.filter((name) => !selectedNames.has(name));
  const unexpectedImages = selectedNames.size !== files.length ||
    files.some((file) => !defaultImageNames.includes(file.name));
  const canStart = missingImages.length === 0 && !unexpectedImages &&
    !initramfsError && diskSelection !== "invalid";
  const canExport = status.state === "paused" && pending === null && diskSelection === "selected";

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
            disabled={active || !canStart}
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
            disabled={status.state !== "paused" || pending !== null}
            onClick={() => control("resume")}
          >
            <Play /> 继续
          </Button>
          <Button
            data-linux-export
            variant="ghost"
            size="sm"
            disabled={!canExport}
            onClick={() => control("export")}
          >
            <Download /> 导出数据盘
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
              基础启动文件
            </h2>
            <label className="linux-profile">
              启动配置
              <select
                data-linux-profile
                aria-label="启动配置"
                value={profile}
                disabled={active}
                onChange={(event) => selectProfile(event.currentTarget.value)}
              >
                <option value="standard">标准</option>
                <option value="alpine">Alpine</option>
              </select>
            </label>
            <p id="linux-files-help">
              请一次选择此配置的锁定文件。浏览器只在本地读取并校验，不会上传。
              {profile === "alpine" && " Alpine 必须使用锁定的 initramfs，不支持自定义根或数据盘。"}
            </p>
            <ul className="linux-images">
              {requiredImages.map((name) => (
                <li key={name}>
                  <code>{name}</code>
                </li>
              ))}
            </ul>
            {initramfs && (
              <p data-linux-ignored-initramfs>
                如果同时选择了默认的 <code>{DEFAULT_INITRAMFS_NAME}</code>，本次启动不会读取它。
              </p>
            )}
            <input
              data-linux-files
              ref={filesInput}
              type="file"
              multiple
              aria-label="选择 Linux 启动镜像"
              aria-describedby="linux-files-help"
              aria-invalid={unexpectedImages || (files.length > 0 && missingImages.length > 0)}
              disabled={active}
              onChange={(event) =>
                setFiles(Array.from(event.currentTarget.files ?? []))
              }
            />
            {unexpectedImages ? (
              <p role="alert">选择中存在重复或未知的文件名。</p>
            ) : files.length > 0 && missingImages.length > 0 && (
              <p data-linux-files-error role="alert">缺少启动文件：{missingImages.join(", ")}。</p>
            )}
          </section>
          <section
            className="linux-local-input"
            aria-labelledby="linux-initramfs-label"
            hidden={profile === "alpine"}
          >
            <h2 className="section-label" id="linux-initramfs-label">
              自定义 initramfs（可选）
            </h2>
            <p id="linux-initramfs-help">
              选择未压缩的 newc 归档（至多 16 MiB），从中运行客体的 /init。归档只在本地读取，不会上传；清除后恢复默认启动根。
            </p>
            <input
              data-linux-initramfs
              ref={initramfsInput}
              type="file"
              aria-label="选择自定义 initramfs"
              aria-describedby="linux-initramfs-help"
              aria-invalid={Boolean(initramfsError)}
              disabled={active || profile === "alpine"}
              onChange={(event) => selectInitramfs(event.currentTarget)}
            />
            {initramfsError && (
              <p data-linux-initramfs-error role="alert">{initramfsError}</p>
            )}
            <Button
              data-linux-clear-initramfs
              variant="outline"
              size="sm"
              disabled={active || profile === "alpine" || (!initramfs && !initramfsError)}
              onClick={clearInitramfs}
            >
              清除自定义 initramfs
            </Button>
          </section>
          <section
            className="linux-local-input"
            aria-labelledby="linux-disk-label"
            hidden={profile === "alpine"}
          >
            <h2 className="section-label" id="linux-disk-label">
              本地数据盘（可选）
            </h2>
            <p id="linux-disk-help">
              选用数据盘时，基础文件须来自块设备配置。先在客体卸载数据盘，再暂停并导出。数据盘不会上传或覆盖原文件。
            </p>
            <input
              data-linux-disk
              ref={diskInput}
              type="file"
              accept=".raw"
              aria-label="选择 Linux 原始数据盘"
              aria-describedby="linux-disk-help"
              aria-invalid={diskSelection === "invalid"}
              disabled={active || profile === "alpine"}
              onChange={(event) => selectDisk(event.currentTarget)}
            />
            {diskSelection === "invalid" && (
              <p data-linux-disk-error role="alert">
                数据盘大小必须是 512 字节的正整数倍，且不得超过 16 MiB。
              </p>
            )}
            <Button
              data-linux-clear-disk
              variant="outline"
              size="sm"
              disabled={active || profile === "alpine" || diskSelection === "empty"}
              onClick={clearDisk}
            >
              清除数据盘
            </Button>
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
