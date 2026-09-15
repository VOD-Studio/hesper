import { useEffect, useState } from "react";
import {
  Activity,
  ArrowRight,
  ArrowUpRight,
  BookOpen,
  Check,
  ChevronDown,
  CircleHelp,
  Cpu,
  FileCode2,
  FolderOpen,
  Keyboard,
  Layers,
  LoaderCircle,
  Monitor,
  PanelRightClose,
  PanelRightOpen,
  Pause,
  Play,
  Power,
  RotateCcw,
  Search,
  Settings2,
  Sparkles,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "./components/ui/button";
import { Badge } from "./components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Switch } from "./components/ui/switch";
import { ScrollArea } from "./components/ui/scroll-area";
import { Screen } from "./Screen";
import { CpuPanel, RegisterPanel } from "./Panels";
import { useEmulator } from "./use-emulator";
import {
  hex,
  normalizePaste,
  parseAddress,
  presets,
  readBytes,
  type Block,
  type Boot,
  type Mode,
} from "./machine";
import "./App.css";

type Modal =
  | "config"
  | "library"
  | "settings"
  | "help"
  | "input"
  | "status"
  | null;
type Preferences = {
  color: "green" | "amber" | "white";
  glow: boolean;
  sidebar: boolean;
  large: boolean;
};
function loadPreferences(): Preferences {
  const defaults: Preferences = {
    color: "green",
    glow: true,
    sidebar: true,
    large: false,
  };
  try {
    const value = JSON.parse(
      localStorage.getItem("hesper-web-display") ?? "{}",
    );
    return {
      color: ["green", "amber", "white"].includes(value.color)
        ? value.color
        : defaults.color,
      glow: typeof value.glow === "boolean" ? value.glow : true,
      sidebar: typeof value.sidebar === "boolean" ? value.sidebar : true,
      large: typeof value.large === "boolean" ? value.large : false,
    };
  } catch {
    return defaults;
  }
}
const colors = { green: "#a4e6a1", amber: "#edc17b", white: "#e5e9df" };

export default function App() {
  const emu = useEmulator();
  const { send, notify } = emu;
  const [mode, setMode] = useState<Mode>("apple1");
  const [modal, setModal] = useState<Modal>(null);
  const [confirm, setConfirm] = useState<{
    title: string;
    description: string;
    action: () => void;
  } | null>(null);
  const [hidden, setHidden] = useState(document.hidden);
  const [preferences, setPreferences] = useState(loadPreferences);
  const [presetId, setPresetId] = useState("basic-huston");
  const [rom, setRom] = useState<File | null>(null);
  const [program, setProgram] = useState<File | null>(null);
  const [address, setAddress] = useState("$0000");
  const [expansion, setExpansion] = useState(false);
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("全部");
  const [inputText, setInputText] = useState("");
  const [formError, setFormError] = useState("");
  const selected = presets.find((preset) => preset.id === presetId);
  const activePreset = presets.find(
    (preset) => preset.id === emu.apple.presetId,
  );
  const activeExpansion = !!emu.apple.expansion;
  const state = mode === "apple1" ? emu.apple : emu.cpu;
  const blocked = !!modal || !!confirm || hidden || busy;

  useEffect(() => {
    send({ type: "gate", mode, blocked });
  }, [mode, blocked, send]);
  useEffect(() => {
    const update = () => setHidden(document.hidden);
    document.addEventListener("visibilitychange", update);
    return () => document.removeEventListener("visibilitychange", update);
  }, []);
  useEffect(() => {
    try {
      localStorage.setItem("hesper-web-display", JSON.stringify(preferences));
    } catch {
      notify("浏览器无法保存显示设置；本次会话仍然有效。");
    }
  }, [preferences, notify]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.altKey && event.shiftKey && event.code === "KeyP" && !blocked) {
        event.preventDefault();
        send({ type: "toggle", mode });
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [mode, blocked, send]);

  function open(next: Modal) {
    setFormError("");
    setModal(next);
  }
  function launch() {
    if (emu.apple.loaded) {
      setConfirm({
        title: "替换当前 Apple-1 会话？",
        description:
          "当前 RAM、屏幕和未处理输入将被丢弃。新会话会使用当前启动配置。",
        action: () => {
          void bootMachine();
        },
      });
    } else void bootMachine();
  }
  async function bootMachine() {
    setConfirm(null);
    setBusy(true);
    setFormError("");
    try {
      if (rom && rom.size !== 256)
        throw new Error("Woz Monitor ROM 必须恰好为 256 bytes。");
      if (presetId === "local" && !program)
        throw new Error("请选择要加载的二进制程序。");
      if (
        program &&
        presetId === "local" &&
        (program.size < 1 || program.size > 65536)
      )
        throw new Error("程序大小必须为 1–65536 bytes。");
      const romBytes = rom
        ? new Uint8Array(await rom.arrayBuffer())
        : await readBytes("machine/wozmon.bin");
      const blocks: Block[] = selected
        ? await Promise.all(
            selected.blocks.map(async (b) => ({
              address: b.address,
              bytes: await readBytes(b.path),
            })),
          )
        : [];
      if (presetId === "local" && program)
        blocks.push({
          address: parseAddress(address),
          bytes: new Uint8Array(await program.arrayBuffer()),
        });
      // Validate all blocks before sending the replacement; the worker also loads transactionally.
      for (const b of blocks) {
        const end = b.address + b.bytes.length;
        const inLow = b.address >= 0 && end <= (expansion ? 0x2000 : 0x1000);
        const inHigh = b.address >= 0xe000 && end <= 0xf000;
        if (!b.bytes.length || (!inLow && !inHigh))
          throw new Error(
            `$${hex(b.address, 4)} 的程序块超出已安装 RAM。请检查加载地址或开启扩展 RAM。`,
          );
      }
      const config: Boot = {
        presetId: selected?.id,
        rom: romBytes,
        expansion,
        blocks,
        name:
          selected?.name ??
          (presetId === "local" ? program!.name : "Woz Monitor"),
        startup:
          selected?.startup ??
          (presetId === "local"
            ? `在 Monitor 输入 ${hex(parseAddress(address), 4)}R。`
            : "输入地址检查内存，或装载一个程序。"),
      };
      send({ type: "boot", boot: config });
      setMode("apple1");
      setModal(null);
    } catch (error) {
      const text = error instanceof Error ? error.message : String(error);
      setFormError(text);
      notify(text, true);
      setModal("config");
    } finally {
      setBusy(false);
    }
  }
  function choosePreset(id: string) {
    setPresetId(id);
    setModal("config");
    setFormError("");
  }
  function upload(file: File | undefined, kind: "rom" | "program") {
    if (!file) return;
    if (kind === "rom" && file.size !== 256) {
      setFormError("ROM 必须恰好为 256 bytes。");
      return;
    }
    if (kind === "program" && (file.size < 1 || file.size > 65536)) {
      setFormError("程序大小必须为 1–65536 bytes。");
      return;
    }
    if (kind === "rom") setRom(file);
    else {
      setProgram(file);
      setPresetId("local");
    }
    setFormError("");
  }
  function submitText() {
    try {
      const normalized = normalizePaste(inputText);
      if (!normalized.text) throw new Error("请输入可打印 ASCII 字符。");
      send({ type: "text", text: normalized.text });
      setInputText("");
      setModal(null);
      notify(
        normalized.skipped
          ? `文本已发送，跳过 ${normalized.skipped} 个不支持的字符。`
          : "文本已加入键盘队列。",
      );
    } catch (error) {
      setFormError(error instanceof Error ? error.message : String(error));
    }
  }
  const statusName = !state.loaded
    ? "等待启动"
    : state.done
      ? "已完成"
      : state.running
        ? "运行中"
        : "已暂停";
  const statusPanel = (
    <>
      <RegisterPanel registers={state.registers} />
      <section className="session-panel">
        <div className="section-label">
          <Activity size={14} /> SESSION
        </div>
        <dl>
          <div>
            <dt>CPU cycles</dt>
            <dd>{state.cycles.toLocaleString()}</dd>
          </div>
          {mode === "apple1" && (
            <>
              <div>
                <dt>Master ticks</dt>
                <dd>{state.ticks.toLocaleString()}</dd>
              </div>
              <div>
                <dt>Video frames</dt>
                <dd>{state.frames.toLocaleString()}</dd>
              </div>
              <div>
                <dt>输入 / 显示握手</dt>
                <dd>{state.pending ? "待处理" : "空闲"}</dd>
              </div>
            </>
          )}
        </dl>
      </section>
      {mode === "apple1" && (
        <section className="ram-panel">
          <div className="section-label">
            <Layers size={14} /> MEMORY MAP
          </div>
          <div className="ram-map">
            <span>
              RAM<small>4 KiB</small>
            </span>
            <span className={activeExpansion ? "" : "uninstalled"}>
              EXT<small>{activeExpansion ? "4 KiB" : "未安装"}</small>
            </span>
            <span>
              RAM<small>4 KiB</small>
            </span>
            <span className="rom-segment">
              ROM<small>256 B</small>
            </span>
          </div>
          <div className="ram-addresses">
            <span>0000</span>
            <span>1000</span>
            <span>E000</span>
            <span>FF00</span>
          </div>
        </section>
      )}
    </>
  );

  return (
    <div className="app-shell">
      <header className="topbar">
        <a
          className="brand"
          href="#"
          onClick={(event) => {
            event.preventDefault();
            setMode("apple1");
          }}
          aria-label="Hesper 首页"
        >
          <span className="brand-mark">
            <Terminal size={20} />
          </span>
          HESPER<span className="brand-label">WEB LAB</span>
        </a>
        <nav className="workspace-tabs" aria-label="工作区">
          <button
            className={mode === "apple1" ? "selected" : ""}
            onClick={() => setMode("apple1")}
          >
            <Monitor size={16} />
            Apple-1
          </button>
          <button
            className={mode === "cpu" ? "selected" : ""}
            onClick={() => setMode("cpu")}
          >
            <Cpu size={16} />
            CPU 6502
          </button>
        </nav>
        <div className="top-actions">
          <Button
            variant="ghost"
            size="icon"
            aria-label="帮助"
            onClick={() => open("help")}
          >
            <CircleHelp />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            aria-label="显示设置"
            onClick={() => open("settings")}
          >
            <Settings2 />
          </Button>
          <span className="local-label">
            <i />
            本地运行
          </span>
        </div>
      </header>
      <main>
        <div className="page-heading">
          <div>
            <div className="eyebrow">
              <span>HESPER / MACHINES</span>
              <span className="heading-divider" />
              {mode === "apple1" ? "EST. 1976" : "MOS TECHNOLOGY"}
            </div>
            <h1>
              {mode === "apple1" ? "Apple-1" : "CPU 6502"}
              <span>
                {mode === "apple1"
                  ? "一台计算机，无限可能。"
                  : "每一个周期，都清晰可见。"}
              </span>
            </h1>
          </div>
          <Button
            variant="outline"
            onClick={() => open(mode === "apple1" ? "library" : "help")}
          >
            {mode === "apple1" ? <BookOpen /> : <FileCode2 />}
            {mode === "apple1" ? "浏览程序库" : "了解演示"}
            <ArrowUpRight size={14} />
          </Button>
        </div>
        <div className="toolbar">
          <div className="toolbar-status">
            <span className={`status-dot ${state.running ? "live" : ""}`} />
            {statusName}
            <span className="toolbar-divider" />
            <span className="machine-spec">
              {mode === "apple1" ? "MOS 6502 · 40 × 24" : "NMOS · 64 KiB RAM"}
            </span>
          </div>
          <div className="run-controls">
            {mode === "apple1" && !state.loaded ? (
              <Button size="sm" onClick={launch} disabled={!emu.ready || busy}>
                <Play />
                启动
              </Button>
            ) : (
              <Button
                variant="ghost"
                size="sm"
                disabled={!state.loaded || state.done}
                onClick={() => send({ type: "toggle", mode })}
              >
                {state.paused || state.done ? <Play /> : <Pause />}
                {state.paused || state.done ? "继续" : "暂停"}
              </Button>
            )}
            <Button
              variant="ghost"
              size="sm"
              disabled={!state.loaded}
              onClick={() => send({ type: "reset", mode })}
            >
              <RotateCcw />
              RESET
            </Button>
            {mode === "apple1" && (
              <>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="清屏"
                  disabled={!state.loaded}
                  onClick={() => send({ type: "clear" })}
                >
                  <Trash2 />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="重新上电"
                  disabled={!state.loaded}
                  onClick={() =>
                    setConfirm({
                      title: "重新上电？",
                      description:
                        "当前 RAM、屏幕与输入将被清除，并重新装载本次会话的原始程序。",
                      action: () => {
                        send({ type: "reboot" });
                        setConfirm(null);
                      },
                    })
                  }
                >
                  <Power />
                </Button>
              </>
            )}
            <span className="toolbar-divider" />
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="切换状态侧栏"
              onClick={() => {
                if (window.innerWidth < 1000) open("status");
                else setPreferences((p) => ({ ...p, sidebar: !p.sidebar }));
              }}
            >
              {preferences.sidebar ? <PanelRightClose /> : <PanelRightOpen />}
            </Button>
          </div>
        </div>
        <div
          className={`workbench ${preferences.sidebar ? "" : "without-sidebar"}`}
        >
          <div className="main-surface">
            {mode === "apple1" ? (
              <>
                <Screen
                  state={emu.apple}
                  color={colors[preferences.color]}
                  glow={preferences.glow}
                  large={preferences.large}
                  send={send}
                  notify={notify}
                  start={launch}
                  openInput={() => open("input")}
                  disabled={!emu.ready || blocked}
                />
                <section className="program-strip">
                  <div className="program-icon">
                    <FileCode2 size={22} strokeWidth={1.4} />
                  </div>
                  <div className="program-strip-text">
                    <span className="eyebrow">
                      {emu.apple.loaded ? "LOADED PROGRAM" : "READY TO LOAD"}
                    </span>
                    <h3>
                      {emu.apple.loaded
                        ? emu.apple.name
                        : (selected?.name ??
                          (presetId === "local"
                            ? (program?.name ?? "本地程序")
                            : "Woz Monitor"))}
                    </h3>
                    <p>
                      {emu.apple.loaded
                        ? emu.apple.startup
                        : (selected?.startup ??
                          "通过启动配置选择 ROM 与程序。")}
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => open("config")}
                  >
                    {emu.apple.loaded ? "更换程序" : "启动配置"}
                    <ChevronDown />
                  </Button>
                </section>
                {emu.apple.loaded && activePreset && (
                  <button
                    className="startup-command"
                    onClick={() => {
                      send({
                        type: "text",
                        text: `${hex(activePreset.entry, 4)}R\r`,
                      });
                      notify("启动指令已加入键盘队列。");
                    }}
                  >
                    <Terminal size={14} />
                    <span>
                      发送启动指令 <code>{hex(activePreset.entry, 4)}R</code>
                    </span>
                    <ArrowRight size={14} />
                  </button>
                )}
              </>
            ) : (
              <CpuPanel state={emu.cpu} send={send} notify={notify} />
            )}
          </div>
          {preferences.sidebar && (
            <aside className="side-panel">
              {statusPanel}
              <div className="side-note">
                <Sparkles size={15} />
                <p>
                  {mode === "apple1"
                    ? "小小的 40 × 24，容得下整个新世界。"
                    : "从寄存器到总线，观察真实的执行过程。"}
                </p>
                <span>BUILT WITH HESPER</span>
              </div>
            </aside>
          )}
        </div>
        <footer className="status-footer">
          <div
            className={emu.notice.error ? "notice error" : "notice"}
            role={emu.notice.error ? "alert" : "status"}
          >
            {!emu.ready ? (
              <LoaderCircle className="spin" size={13} />
            ) : emu.notice.error ? (
              <CircleHelp size={13} />
            ) : (
              <Check size={13} />
            )}
            <span>{emu.notice.text}</span>
          </div>
          <span className="footer-key">
            <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd> 暂停 / 继续
          </span>
        </footer>
        <div className="page-footnote">
          <span>AN EXPLORATION IN COMPUTING</span>
          <button onClick={() => open("help")}>
            关于 Hesper <ArrowUpRight size={12} />
          </button>
        </div>
      </main>

      <Dialog
        open={modal !== null}
        onOpenChange={(value) => {
          if (!value && !busy) setModal(null);
        }}
      >
        <DialogContent
          className={modal === "library" ? "library-dialog" : "app-dialog"}
        >
          <DialogHeader>
            <DialogTitle>
              {
                (
                  {
                    config: "启动配置",
                    library: "程序库",
                    settings: "显示设置",
                    help: "关于这间计算机实验室",
                    input: "发送文本到 Apple-1",
                    status: "机器状态",
                  } as const
                )[modal ?? "config"]
              }
            </DialogTitle>
            <DialogDescription>
              {
                (
                  {
                    config: "选择固件与程序，然后开始新的 Apple-1 会话。",
                    library: `${presets.length} 个程序。把好奇心装进几千字节。`,
                    settings: "让这台终端看起来更像你的终端。",
                    help: "Hesper · Apple-1 与 NMOS 6502 交互工作台",
                    input: "支持 ASCII 文本；换行会作为 Enter 发送。",
                    status: "当前寄存器、计数和内存配置。",
                  } as const
                )[modal ?? "config"]
              }
            </DialogDescription>
          </DialogHeader>
          {modal === "config" && (
            <div className="config-form">
              <label className="field-label">
                01 <span>固件 ROM</span>
                <Badge variant="secondary">256 B</Badge>
              </label>
              <div
                className="file-drop"
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  event.preventDefault();
                  upload(event.dataTransfer.files[0], "rom");
                }}
              >
                <Monitor size={21} />
                <div>
                  <strong>{rom?.name ?? "Woz Monitor"}</strong>
                  <small>
                    {rom ? "自选 ROM · 当前会话有效" : "内置固件 · 开箱即用"}
                  </small>
                </div>
                <label className="file-button">
                  {rom ? "更换" : "选择文件"}
                  <input
                    aria-label="选择 ROM 文件"
                    type="file"
                    accept=".bin,.rom"
                    onChange={(event) => upload(event.target.files?.[0], "rom")}
                  />
                </label>
                {rom && (
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label="使用内置 ROM"
                    onClick={() => setRom(null)}
                  >
                    <X />
                  </Button>
                )}
              </div>
              <label className="field-label">
                02 <span>程序</span>
              </label>
              <div className="program-options">
                <Button
                  variant={selected ? "secondary" : "outline"}
                  onClick={() => open("library")}
                >
                  <BookOpen />
                  {selected?.name ?? "预置程序"}
                  <ChevronDown />
                </Button>
                <Button
                  variant={presetId === "none" ? "secondary" : "outline"}
                  onClick={() => setPresetId("none")}
                >
                  仅 Monitor
                </Button>
                <label
                  className={`file-button ${presetId === "local" ? "chosen" : ""}`}
                >
                  <FolderOpen size={14} />
                  本地程序
                  <input
                    aria-label="选择本地程序"
                    type="file"
                    accept=".bin"
                    onChange={(event) =>
                      upload(event.target.files?.[0], "program")
                    }
                  />
                </label>
              </div>
              {selected && (
                <div className="preset-summary">
                  <strong>{selected.name}</strong>
                  <p>{selected.startup}</p>
                  <div className="block-list">
                    {selected.blocks.map((block, index) => (
                      <code key={index}>
                        ${hex(block.address, 4)} · {block.size.toLocaleString()}{" "}
                        B
                      </code>
                    ))}
                  </div>
                  <small>
                    {expansion ? selected.expanded_note : selected.note}
                  </small>
                  <small>{selected.license}</small>
                  <a href={selected.source} target="_blank" rel="noreferrer">
                    {selected.author} · {selected.year}{" "}
                    <ArrowUpRight size={12} />
                  </a>
                </div>
              )}
              {presetId === "local" && (
                <div className="local-program">
                  <span>{program?.name ?? "尚未选择文件"}</span>
                  <label htmlFor="load-address">加载地址</label>
                  <Input
                    id="load-address"
                    value={address}
                    onChange={(event) => setAddress(event.target.value)}
                    placeholder="$0000"
                  />
                </div>
              )}
              <div className="setting-row">
                <label htmlFor="expansion">
                  <strong>扩展 RAM</strong>
                  <small>安装 $1000–$1FFF 的额外 4 KiB RAM</small>
                </label>
                <Switch
                  id="expansion"
                  checked={expansion}
                  onCheckedChange={setExpansion}
                />
              </div>
              {formError && (
                <p role="alert" className="form-error">
                  {formError}
                </p>
              )}
              <DialogFooter>
                <Button
                  variant="ghost"
                  disabled={busy}
                  onClick={() => setModal(null)}
                >
                  取消
                </Button>
                <Button onClick={launch} disabled={!emu.ready || busy}>
                  {busy ? <LoaderCircle className="spin" /> : <Power />}
                  校验并启动
                </Button>
              </DialogFooter>
            </div>
          )}
          {modal === "library" && (
            <>
              <div className="library-search">
                <Search size={16} />
                <Input
                  aria-label="搜索程序"
                  placeholder="搜索名称、作者或启动地址…"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                />
              </div>
              <div className="category-tabs">
                {["全部", ...new Set(presets.map((p) => p.category))].map(
                  (item) => (
                    <button
                      key={item}
                      className={category === item ? "selected" : ""}
                      onClick={() => setCategory(item)}
                    >
                      {item}
                    </button>
                  ),
                )}
              </div>
              <ScrollArea className="library-scroll">
                <div className="program-list">
                  {presets
                    .filter(
                      (p) =>
                        (category === "全部" || category === p.category) &&
                        `${p.name} ${p.id} ${p.author} ${hex(p.load, 4)}`
                          .toLowerCase()
                          .includes(query.toLowerCase()),
                    )
                    .map((p) => (
                      <button
                        className="program-row"
                        key={p.id}
                        onClick={() => choosePreset(p.id)}
                      >
                        <span className="library-program-icon">
                          <FileCode2 size={19} />
                        </span>
                        <span>
                          <strong>{p.name}</strong>
                          <small>
                            {p.category} · {p.author} · {p.year}
                          </small>
                          <em>
                            {p.note.includes("扩展") ? p.note : p.startup}
                          </em>
                        </span>
                        <span className="program-size">
                          ${hex(p.load, 4)}
                          <small>
                            {p.blocks
                              .reduce((n, b) => n + b.size, 0)
                              .toLocaleString()}{" "}
                            B
                          </small>
                        </span>
                        <ArrowRight size={15} />
                      </button>
                    ))}
                  {!presets.some(
                    (p) =>
                      (category === "全部" || category === p.category) &&
                      `${p.name} ${p.id} ${p.author} ${hex(p.load, 4)}`
                        .toLowerCase()
                        .includes(query.toLowerCase()),
                  ) && (
                    <p className="empty-results">
                      没有匹配的程序，试试其他关键词。
                    </p>
                  )}
                </div>
              </ScrollArea>
              <div className="library-bottom">
                <span>原始镜像 · 保留作者与来源信息</span>
                <Button variant="ghost" onClick={() => open("config")}>
                  返回配置
                </Button>
              </div>
            </>
          )}
          {modal === "settings" && (
            <div className="settings-content">
              <label className="field-label">屏幕荧光色</label>
              <div className="color-options">
                {(["green", "amber", "white"] as const).map((color) => (
                  <button
                    className={preferences.color === color ? "selected" : ""}
                    key={color}
                    onClick={() => setPreferences((p) => ({ ...p, color }))}
                  >
                    <i style={{ background: colors[color] }} />
                    {
                      { green: "经典绿", amber: "琥珀色", white: "柔白色" }[
                        color
                      ]
                    }
                    {preferences.color === color && <Check size={13} />}
                  </button>
                ))}
              </div>
              {(
                [
                  {
                    key: "glow",
                    title: "屏幕辉光",
                    text: "为字符添加轻微的荧光效果",
                  },
                  {
                    key: "large",
                    title: "放大屏幕",
                    text: "增加桌面显示尺寸，字符网格保持不变",
                  },
                  {
                    key: "sidebar",
                    title: "状态侧栏",
                    text: "显示寄存器与会话信息",
                  },
                ] as const
              ).map((setting) => (
                <div className="setting-row" key={setting.key}>
                  <label htmlFor={setting.key}>
                    <strong>{setting.title}</strong>
                    <small>{setting.text}</small>
                  </label>
                  <Switch
                    id={setting.key}
                    checked={preferences[setting.key]}
                    onCheckedChange={(value) =>
                      setPreferences((p) => ({ ...p, [setting.key]: value }))
                    }
                  />
                </div>
              ))}
              <p className="settings-hint">显示偏好会自动保存在当前浏览器。</p>
            </div>
          )}
          {modal === "input" && (
            <>
              <textarea
                className="text-entry"
                aria-label="发送的文本"
                placeholder={"E000R\nPRINT 2+2\n"}
                value={inputText}
                onChange={(event) => setInputText(event.target.value)}
                spellCheck={false}
              />
              <p className="input-hint">
                命令后保留换行，机器才会执行。输入通过键盘队列逐字送入。
              </p>
              {formError && (
                <p role="alert" className="form-error">
                  {formError}
                </p>
              )}
              <DialogFooter>
                <Button variant="ghost" onClick={() => setModal(null)}>
                  取消
                </Button>
                <Button onClick={submitText} disabled={!emu.apple.loaded}>
                  <Keyboard />
                  发送文本
                </Button>
              </DialogFooter>
            </>
          )}
          {modal === "status" && (
            <div className="mobile-status">{statusPanel}</div>
          )}
          {modal === "help" && (
            <div className="help-content">
              <p>
                使用现有 Rust 核心，在浏览器中运行 Apple-1 和独立的 CPU
                6502。所有文件和执行都留在当前浏览器。
              </p>
              <h3>Apple-1 操作</h3>
              <ul>
                <li>
                  启动后点击屏幕输入，Enter 发送 CR，Backspace 发送下划线，Esc
                  发送取消字符。
                </li>
                <li>
                  使用“粘贴 / 输入文本”发送多行命令。BASIC (Huston) 输入{" "}
                  <code>E000R</code> 启动。
                </li>
                <li>
                  RESET 保留 RAM
                  与屏幕；重新上电会恢复初始程序。弹窗和后台标签页会暂停自由运行。
                </li>
                <li>
                  <code>Alt + Shift + P</code>{" "}
                  暂停或继续；其他操作通过工具栏完成。
                </li>
              </ul>
              <h3>CPU 计数演示</h3>
              <p>
                从 <code>$8000</code> 执行，在 <code>$800F</code>{" "}
                前停止。可逐指令或逐周期观察真实总线访问；最近 256
                个周期保存在执行记录中。
              </p>
              <h3>显示与兼容范围</h3>
              <p>
                屏幕是 40 × 24 字符投影；辉光属于界面效果。当前为固定 NMOS 6502
                与 Apple-1 模型，预置可加载不等于所有功能已验收。
              </p>
              <div className="help-links">
                <a
                  href={`${import.meta.env.BASE_URL}machine/program-notes.md`}
                  target="_blank"
                  rel="noreferrer"
                >
                  程序来源与兼容记录 <ArrowUpRight size={12} />
                </a>
                <a
                  href={`${import.meta.env.BASE_URL}machine/rom-notes.md`}
                  target="_blank"
                  rel="noreferrer"
                >
                  ROM 来源 <ArrowUpRight size={12} />
                </a>
                <a
                  href={`${import.meta.env.BASE_URL}machine/apple1-wasm-notes.md`}
                  target="_blank"
                  rel="noreferrer"
                >
                  Wasm 字模署名 <ArrowUpRight size={12} />
                </a>
              </div>
              {emu.apple.loaded && (
                <Button
                  variant="destructive"
                  onClick={() => {
                    setModal(null);
                    setConfirm({
                      title: "结束 Apple-1 会话？",
                      description:
                        "当前 RAM、屏幕和输入将被丢弃，并返回启动界面。",
                      action: () => {
                        send({ type: "stop" });
                        setConfirm(null);
                        setMode("apple1");
                      },
                    });
                  }}
                >
                  结束会话，返回启动中心
                </Button>
              )}
            </div>
          )}
        </DialogContent>
      </Dialog>
      <Dialog
        open={!!confirm}
        onOpenChange={(value) => {
          if (!value) setConfirm(null);
        }}
      >
        <DialogContent className="app-dialog">
          <DialogHeader>
            <DialogTitle>{confirm?.title}</DialogTitle>
            <DialogDescription>{confirm?.description}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm(null)}>
              取消
            </Button>
            <Button onClick={() => confirm?.action()}>确认</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
