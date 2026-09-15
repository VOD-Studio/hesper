import { useCallback, useEffect, useRef, useState } from "react";
import { blankSnapshot, type Command, type Reply } from "./machine";

export function useEmulator() {
  const worker = useRef<Worker | null>(null);
  const [ready, setReady] = useState(false);
  const [apple, setApple] = useState(() => blankSnapshot("apple1"));
  const [cpu, setCpu] = useState(() => blankSnapshot("cpu"));
  const [notice, setNotice] = useState({
    text: "正在连接模拟器…",
    error: false,
  });
  useEffect(() => {
    const instance = new Worker(
      new URL("./emulator.worker.ts", import.meta.url),
      { type: "module" },
    );
    worker.current = instance;
    instance.onmessage = ({ data }: MessageEvent<Reply>) => {
      if (data.type === "ready") {
        setReady(true);
        setNotice({ text: "模拟器已就绪。选择程序，开始探索。", error: false });
      }
      if (data.type === "snapshot")
        (data.snapshot.mode === "apple1" ? setApple : setCpu)(data.snapshot);
      if (data.type === "notice")
        setNotice({ text: data.text, error: !!data.error });
    };
    instance.onerror = () =>
      setNotice({ text: "模拟器加载失败，请刷新页面重试。", error: true });
    return () => {
      instance.terminate();
      worker.current = null;
    };
  }, []);
  const send = useCallback(
    (command: Command) => worker.current?.postMessage(command),
    [],
  );
  const notify = useCallback(
    (text: string, error = false) => setNotice({ text, error }),
    [],
  );
  return { ready, apple, cpu, notice, send, notify };
}
