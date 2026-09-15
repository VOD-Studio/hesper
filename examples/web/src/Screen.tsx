import { useState, type CSSProperties } from "react";
import { ArrowUpRight, Keyboard, Monitor, Power } from "lucide-react";
import { Button } from "./components/ui/button";
import {
  normalizePaste,
  screenText,
  type Command,
  type Snapshot,
} from "./machine";

export function Screen({
  state,
  color,
  glow,
  large,
  send,
  notify,
  start,
  openInput,
  disabled,
}: {
  state: Snapshot;
  color: string;
  glow: boolean;
  large: boolean;
  send: (c: Command) => void;
  notify: (text: string, error?: boolean) => void;
  start: () => void;
  openInput: () => void;
  disabled: boolean;
}) {
  const [focused, setFocused] = useState(false);
  const screen = state.screen ?? new Uint8Array(960).fill(32);
  const paste = (text: string) => {
    try {
      const input = normalizePaste(text);
      if (input.text) send({ type: "text", text: input.text });
      if (input.skipped) notify(`已跳过 ${input.skipped} 个不支持的字符。`);
    } catch (error) {
      notify(String(error), true);
    }
  };
  return (
    <section
      className={`monitor ${large ? "monitor-large" : ""}`}
      aria-label="Apple-1 显示器"
    >
      <div className="monitor-top">
        <span>
          <i className={state.loaded ? "power-led on" : "power-led"} /> APPLE-1
          VIDEO TERMINAL
        </span>
        <span>40 COL × 24 ROW</span>
      </div>
      <div
        className={`crt ${glow ? "crt-glow" : ""} ${focused ? "crt-focused" : ""}`}
        style={{ "--phosphor": color } as CSSProperties}
      >
        <div
          className="screen-grid"
          role="textbox"
          aria-label="Apple-1 屏幕，点击后键盘输入"
          aria-multiline="true"
          aria-readonly={!state.loaded}
          tabIndex={state.loaded ? 0 : -1}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onPaste={(event) => {
            if (!state.loaded || disabled) return;
            event.preventDefault();
            paste(event.clipboardData.getData("text"));
          }}
          onKeyDown={(event) => {
            if (
              !state.loaded ||
              disabled ||
              event.ctrlKey ||
              event.metaKey ||
              event.altKey ||
              event.nativeEvent.isComposing
            )
              return;
            let byte: number | undefined;
            if (event.key === "Enter") byte = 13;
            else if (event.key === "Backspace") byte = 95;
            else if (event.key === "Escape") byte = 27;
            else if (event.key.length === 1 && /^[\x20-\x7e]$/.test(event.key))
              byte = event.key.charCodeAt(0);
            if (byte !== undefined) {
              event.preventDefault();
              send({ type: "key", byte });
            }
          }}
        >
          <pre data-testid="apple-screen">{screenText(screen)}</pre>
          {state.cursor?.visible && state.loaded && (
            <span
              className="machine-cursor"
              style={{
                left: `${state.cursor.column * 2.5}%`,
                top: `${(state.cursor.row * 100) / 24}%`,
              }}
            />
          )}
        </div>
        {!state.loaded && (
          <div className="screen-empty">
            <div className="terminal-emblem">
              <Monitor size={38} strokeWidth={1} />
            </div>
            <span className="eyebrow">THE PERSONAL COMPUTER, REVISITED.</span>
            <h2>从一个光标开始。</h2>
            <p>回到 1976 年，探索一台计算机的起点。</p>
            <Button size="lg" onClick={start} disabled={disabled}>
              <Power size={16} />
              启动 Apple-1
              <ArrowUpRight size={16} />
            </Button>
            <small>Woz Monitor 已就绪 · 支持加载本地 ROM</small>
          </div>
        )}
      </div>
      <div className="monitor-bottom">
        <span>
          <Keyboard size={14} />
          {focused ? "键盘已连接 · 输入将发送给 Apple-1" : "点击屏幕连接键盘"}
        </span>
        <Button
          variant="ghost"
          size="sm"
          disabled={!state.loaded}
          onClick={openInput}
        >
          粘贴 / 输入文本 <ArrowUpRight size={13} />
        </Button>
      </div>
    </section>
  );
}
