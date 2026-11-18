// Use real Chromium, production assets, native file selection and key events.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { createInterface } from "node:readline";
import { Readable } from "node:stream";

const [dist, images, corrupt, logs] = process.argv.slice(2);
const lock = JSON.parse(readFileSync(new URL("images.lock.json", import.meta.url), "utf8"));
const root = resolve(dist);
const profile = mkdtempSync(join(tmpdir(), "hesper-riscv-chrome-"));
const chrome = process.env.CHROME_BIN || "google-chrome";
const tab = "/hesper/";
const guestNames = new Set(lock.images.map(entry => entry.path));
let guestDownloadedBytes = 0;
const server = Bun.serve({
  hostname: "127.0.0.1", port: 0,
  async fetch(request) {
    const path = decodeURIComponent(new URL(request.url).pathname);
    if (!path.startsWith(tab)) return new Response(null, { status: 404 });
    const file = resolve(root, path.slice(tab.length) || "index.html");
    if (!file.startsWith(root + sep)) return new Response(null, { status: 404 });
    const asset = Bun.file(file);
    if (!await asset.exists()) return new Response(null, { status: 404 });
    if (guestNames.has(path.slice(path.lastIndexOf("/") + 1))) guestDownloadedBytes += asset.size;
    return new Response(asset);
  },
});

function check(condition, message) {
  if (!condition) throw new Error(message);
}
function emit(message) { console.log(JSON.stringify(message)); }

const child = Bun.spawn([chrome, "--headless", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
  "--disable-background-networking",
  "--remote-debugging-port=0", "--window-size=1440,960", `--user-data-dir=${profile}`, "about:blank"],
{ stdout: "ignore", stderr: "pipe" });
const stderr = createInterface({ input: Readable.fromWeb(child.stderr) });
let chromeError = "";
let socket;
try {
  const port = await new Promise((done, fail) => {
    const timer = setTimeout(() => fail(new Error(`Chromium DevTools did not open: ${chromeError}`)), 12_000);
    stderr.on("line", line => {
      chromeError = (chromeError + line + "\n").slice(-8192);
      const found = /DevTools listening on ws:\/\/127\.0\.0\.1:(\d+)\//.exec(line);
      if (found) { clearTimeout(timer); done(found[1]); }
    });
    child.exited.then(code => { clearTimeout(timer); fail(new Error(`Chromium exited (${code}): ${chromeError}`)); });
  });
  const origin = `http://127.0.0.1:${port}`;
  const version = await (await fetch(`${origin}/json/version`)).json();
  const pages = await (await fetch(`${origin}/json/list`)).json();
  const page = pages.find(target => target.type === "page");
  check(page, "Chromium did not expose its temporary tab");
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((done, fail) => {
    socket.addEventListener("open", done, { once: true });
    socket.addEventListener("error", fail, { once: true });
  });
  let sequence = 0, streaming, serialBytes = 0;
  const requests = new Map();
  const replies = [];
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id !== undefined) {
      const complete = requests.get(message.id);
      if (complete) complete(message);
    } else if (message.method === "Runtime.bindingCalled" && message.params.name === "observeHesper") {
      const event = JSON.parse(message.params.payload);
      replies.push(event);
      if (event.sessionId !== streaming) return;
      if (event.type === "stdout") {
        serialBytes += event.bytes.length;
        emit({ type: "serial", bytes: Buffer.from(event.bytes).toString("base64") });
      } else if (event.type === "error") emit({ type: "error", message: event.message });
    }
  };
  const call = (method, params = {}, ms = 12_000) => new Promise((done, fail) => {
    const id = ++sequence;
    const timer = setTimeout(() => { requests.delete(id); fail(new Error(`${method}: deadline`)); }, ms);
    requests.set(id, message => {
      requests.delete(id);
      clearTimeout(timer);
      if (message.error) fail(new Error(`${method}: ${message.error.message}`));
      else done(message.result);
    });
    socket.send(JSON.stringify({ id, method, params }));
  });
  async function evaluate(expression, awaitPromise = false) {
    const result = await call("Runtime.evaluate", { expression, awaitPromise, returnByValue: true });
    if (result.exceptionDetails) throw new Error(`${result.exceptionDetails.exception?.description}: ${expression}`);
    return result.result.value;
  }
  const state = name => `document.querySelector('[data-linux-state]')?.dataset.linuxState === '${name}'`;
  const visible = prefix => `document.querySelector(${JSON.stringify(prefix)})?.checkVisibility()`;
  const tty = () => `Array.from(document.querySelectorAll('[data-linux-terminal] .xterm-rows > div'), row => row.textContent).join('\\n')`;
  async function until(predicate, name) {
    const available = await evaluate(`new Promise(resolve => {
      const started = performance.now();
      function tick() {
        if (${predicate}) resolve(true);
        else if (performance.now() - started > 8000) resolve(false);
        else setTimeout(tick, 12);
      }
      tick();
    })`, true);
    check(available, `page did not reach ${name}`);
  }
  async function click(selector) {
    const point = await evaluate(`(() => {
      const target = document.querySelector(${JSON.stringify(selector)});
      if (!target || target.disabled || !target.checkVisibility()) return null;
      target.scrollIntoView({block:'center'});
      const rect = target.getBoundingClientRect();
      return {x: rect.x + rect.width/2, y: rect.y + rect.height/2};
    })()`);
    check(point, `control is unavailable: ${selector}`);
    await call("Input.dispatchMouseEvent", { ...point, type: "mousePressed", button: "left", clickCount: 1 });
    await call("Input.dispatchMouseEvent", { ...point, type: "mouseReleased", button: "left", clickCount: 1 });
  }
  async function selectFiles(directory) {
    const { root } = await call("DOM.getDocument");
    const { nodeId } = await call("DOM.querySelector", { nodeId: root.nodeId, selector: "[data-linux-files]" });
    check(nodeId, "Linux file input missing");
    await call("DOM.setFileInputFiles", { nodeId, files: lock.images.map(entry => join(directory, entry.path)) });
    await until("!document.querySelector('[data-linux-start]')?.disabled", "four selected local images");
    return evaluate("Array.from(document.querySelector('[data-linux-files]').files, file => file.size).reduce((a,b) => a+b, 0)");
  }
  async function type(text) {
    await click("[data-linux-focus]");
    check(await evaluate("document.activeElement?.matches('[data-linux-terminal] .xterm-helper-textarea')"), "xterm did not take focus");
    for (const char of text) {
      const key = char === "\n" ? "Enter" : char;
      const input = char === "\n" ? { code: "Enter", windowsVirtualKeyCode: 13 } : { text: char };
      await call("Input.dispatchKeyEvent", { type: "keyDown", key, ...input });
      await call("Input.dispatchKeyEvent", { type: "keyUp", key });
    }
  }
  const tick = () => evaluate("document.querySelector('[data-linux-time]')?.dataset.linuxTime");

  await call("Page.enable");
  await call("Runtime.enable");
  await call("Runtime.addBinding", { name: "observeHesper" });
  // Passively record actual Worker replies alongside the unmodified app handler.
  // No synthetic worker, reply, guest state, or rendering.
  await call("Page.addScriptToEvaluateOnNewDocument", { source: `(() => {
    const Worker = globalThis.Worker;
    globalThis.Worker = class extends Worker {
      constructor(...args) {
        super(...args);
        this.addEventListener('message', ({data}) => {
          if (!data || typeof data.sessionId !== 'number') return;
          observeHesper(JSON.stringify(data, (key, value) => typeof value === 'bigint' ? String(value)
            : value instanceof Uint8Array ? [...value] : value));
        });
      }
    };
    let previous = performance.now();
    globalThis.hesperMaxIntervalMs = 0;
    setInterval(() => {
      const now = performance.now();
      hesperMaxIntervalMs = Math.max(hesperMaxIntervalMs, now - previous);
      previous = now;
    }, 16);
  })();` });
  await call("Page.navigate", { url: `http://127.0.0.1:${server.port}${tab}` });
  await until(`${visible('[data-testid="apple-screen"]')} && !document.querySelector('.screen-empty button')?.disabled`, "Apple-1 before any Linux resources");
  check(await evaluate("performance.getEntriesByType('resource').every(entry => !entry.name.includes('/linux.worker-') && !entry.name.includes('/hesper_riscv_linux_bg-'))"), "Linux Worker loaded before selecting a Linux session");
  await click(".screen-empty button");
  await until("document.querySelector('.screen-grid')?.getAttribute('aria-readonly') === 'false' && document.querySelector('[data-testid=apple-screen]').textContent.trim().length > 0", "real Woz Monitor output");

  await click("[data-linux-tab]");
  await until(visible("[data-linux-files]"), "lazy Linux workspace");
  // Stop a real loading Worker, then immediately start a new one with a bad DTB.
  await selectFiles(images);
  await click("[data-linux-start]");
  check(await evaluate(state("loading")), "first session was not in its loading state");
  await click("[data-linux-stop]");
  await until(state("ended"), "cancelled loading");
  await selectFiles(corrupt);
  await click("[data-linux-start]");
  await until(state("failed"), "corrupt DTB rejection before guest execution");
  check(await evaluate("document.querySelector('[data-linux-error]')?.textContent.includes('SHA-256')"), "corrupt DTB did not report a digest error");
  check(replies.every(reply => reply.type !== "ready" && reply.type !== "stdout"), "cancelled or corrupt session executed before the valid boot");

  const bytes = await selectFiles(images);
  await evaluate("globalThis.hesperMaxIntervalMs = 0"); // reset only the passive timing probe
  streaming = 3;
  emit({ type: "start", browser: version.Browser, imageBytes: bytes, url: `http://127.0.0.1:${server.port}${tab}` });
  await click("[data-linux-start]");
  let paused;
  for await (const line of createInterface({ input: process.stdin })) {
    const request = JSON.parse(line);
    if (request.type === "keys") {
      if (paused === undefined) {
        await click("[data-linux-pause]");
        await until(state("paused"), "pause after the first prompt");
        paused = await tick();
        const before = serialBytes;
        await type(request.text);
        await new Promise(done => setTimeout(done, 150));
        check(await tick() === paused && serialBytes === before, "queued input ran while paused");
        await click("[data-linux-resume]");
      } else await type(request.text);
    } else if (request.type === "finish") {
      await until(`${tty()}.includes('HESPER_OK:42')`, "xterm rendered the guest result");
      const screenshot = await call("Page.captureScreenshot", { format: "png" });
      writeFileSync(join(logs, "browser-terminal.png"), Buffer.from(screenshot.data, "base64"));
      const maxIntervalMs = await evaluate("globalThis.hesperMaxIntervalMs");
      const resumed = replies.findLastIndex(reply => reply.sessionId === 3 && reply.type === "state" && reply.state === "paused");
      check(replies[resumed + 1]?.state === "running" && replies[resumed + 1].time === paused, "resume did not preserve the paused virtual tick");
      await click("[data-linux-stop]");
      await until(state("ended"), "destroyed session");
      streaming = undefined;
      const retire = replies.findLast(reply => reply.sessionId === 3 && reply.state === "ended");
      await click("[data-linux-start]");
      await until(`${tty()}.includes('OpenSBI v1.6')`, "fresh firmware after destroying the session");
      check(!(await evaluate(`${tty()}.includes('HESPER_OK:42')`)), "old terminal output leaked into the restart");
      check(replies.some(reply => reply.sessionId === 4 && reply.state === "running" && reply.time === "0" && reply.retired === "0"), "restart lacked an initial zero-time, zero-retirement state");
      await click("[data-linux-stop]");
      await until(state("ended"), "ended fresh session");
      check(replies.every(reply => (reply.sessionId !== 1 && reply.sessionId !== 2) || reply.type !== "stdout"), "stale output from cancelled or corrupt boot");
      check(guestDownloadedBytes === 0, "the site fetched guest images instead of reading local Files");
      emit({ type: "done", pausedTick: paused, maxIntervalMs, retired: retire?.retired, tick: retire?.time, guestDownloadedBytes });
      break;
    } else throw new Error(`unknown browser gate request ${request.type}`);
  }
} catch (error) {
  emit({ type: "error", message: String(error) });
  console.error(error);
  process.exitCode = 1;
} finally {
  process.stdin.destroy();
  socket?.close();
  await server.stop(true);
  child.kill();
  await child.exited;
  writeFileSync(join(logs, "browser-chromium.stderr.log"), chromeError);
  rmSync(profile, { recursive: true, force: true });
}
