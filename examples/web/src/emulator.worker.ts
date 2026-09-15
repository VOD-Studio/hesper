import initApple from "./generated/apple1/hesper_apple1";
import initCpu from "./generated/cpu6502/hesper_cpu6502";
import appleUrl from "./generated/apple1/hesper_apple1_bg.wasm?url";
import cpuUrl from "./generated/cpu6502/hesper_cpu6502_bg.wasm?url";
import { createRuntime } from "./runtime";
import type { Command, Reply } from "./machine";

const send = (reply: Reply) => self.postMessage(reply);
const ready = Promise.all([
  initApple({ module_or_path: appleUrl }),
  initCpu({ module_or_path: cpuUrl }),
]).then(() => {
  const runtime = createRuntime(send);
  send({ type: "ready" });
  setInterval(runtime.advance, 16);
  return runtime;
});
ready.catch((error) =>
  send({
    type: "notice",
    text: `Wasm 初始化失败：${String(error)}`,
    error: true,
  }),
);
self.onmessage = (event: MessageEvent<Command>) => {
  void ready
    .then((runtime) => runtime.handle(event.data))
    .catch(() => {
      /* Initialization error is reported above. */
    });
};
