// Use real Chromium, production assets, native file selection and key events.
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve, sep } from "node:path";
import { createInterface } from "node:readline";
import { Readable } from "node:stream";

const [dist, images, logs, mode, ...input] = process.argv.slice(2);
const alpineDiskGate = mode === "alpine-disk";
const diskGate = mode === "disk" || alpineDiskGate;
const initramfsGate = mode === "initramfs";
const alpineGate = mode === "alpine";
const benchmark = mode === "alpine-bench";
const benchDisk = benchmark ? input[0] : undefined;
const [cliDisk, webDisk, oversized, alpineSerialImages, standardImages] = input;
const [corrupt] = input;
const lockName = alpineDiskGate || benchDisk ? "images-alpine-lts-blk.lock.json" : diskGate ? "images-blk.lock.json"
  : alpineGate || benchmark ? "images-alpine-lts.lock.json" : "images.lock.json";
const readLock = name => JSON.parse(readFileSync(new URL(name, import.meta.url), "utf8"));
const lock = readLock(lockName);
const root = resolve(dist);
const profile = mkdtempSync(join(tmpdir(), "hesper-riscv-chrome-"));
const chrome = process.env.CHROME_BIN || "google-chrome";
const tab = "/hesper/";
const guestNames = new Set(lock.images.map(entry => entry.path));
if (initramfsGate) for (const file of input.slice(1)) guestNames.add(basename(file));
if (diskGate) for (const file of [cliDisk, webDisk]) guestNames.add(basename(file));
if (benchDisk) guestNames.add(basename(benchDisk));
const isGuestPath = path => guestNames.has(basename(path)) || (alpineGate && /\.apk$/i.test(path));
const alpineWorkerEvents = [], networkTrace = [], networkRequests = new Map(), servedWasm = new Set();
const guestRequests = [];
let guestDownloadedBytes = 0;
let firstLinuxWasm, releaseFirstLinuxWasm;
// Hold only the first real Linux Wasm response so cancellation exercises an
// outstanding production load, regardless of CPU speed or filesystem cache.
const cancelledFetch = new Promise(done => { releaseFirstLinuxWasm = done; });
const server = Bun.serve({
  hostname: "127.0.0.1", port: 0,
  async fetch(request) {
    const path = decodeURIComponent(new URL(request.url).pathname);
    const guestPath = isGuestPath(path);
    if (guestPath || !["GET", "HEAD"].includes(request.method)) guestRequests.push(path);
    if (!path.startsWith(tab)) return new Response(null, { status: 404 });
    const file = resolve(root, path.slice(tab.length) || "index.html");
    if (!file.startsWith(root + sep)) return new Response(null, { status: 404 });
    const asset = Bun.file(file);
    if (!await asset.exists()) return new Response(null, { status: 404 });
    if (guestPath) guestDownloadedBytes += asset.size;
    if (alpineGate && /\/hesper_riscv_linux_bg-[^/]+\.wasm$/.test(path)) servedWasm.add(path);
    if (initramfsGate && !firstLinuxWasm && /\/hesper_riscv_linux_bg-[^/]+\.wasm$/.test(path)) {
      firstLinuxWasm = path;
      await cancelledFetch;
    }
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
let socket, browserSocket;
const delay = ms => new Promise(done => setTimeout(done, ms));
async function connect(url) {
  const connection = new WebSocket(url);
  await new Promise((done, fail) => {
    connection.addEventListener("open", done, { once: true });
    connection.addEventListener("error", fail, { once: true });
  });
  return connection;
}
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
  socket = await connect(page.webSocketDebuggerUrl);
  if (diskGate) browserSocket = await connect(version.webSocketDebuggerUrl);
  let sequence = 0, streaming, serialBytes = 0;
  const requests = new Map();
  const replies = [];
  const workerEvents = alpineGate ? alpineWorkerEvents : [];
  const downloads = [], downloadProgress = new Map(), dialogs = [];
  let networkError;
  const onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id !== undefined) {
      const complete = requests.get(message.id);
      if (complete) complete(message);
    } else if (alpineGate && message.method === "Target.attachedToTarget") {
      void observeTarget(message.params);
    } else if (alpineGate && message.method === "Network.requestWillBeSent") {
      const { requestId, request, type } = message.params;
      const url = new URL(request.url);
      if (url.protocol !== "http:" && url.protocol !== "https:") return;
      const requestEvent = { url: url.href, method: request.method, type, target: message.sessionId ?? "page",
        guest: isGuestPath(decodeURIComponent(url.pathname)) };
      networkTrace.push(requestEvent);
      networkRequests.set(`${requestEvent.target}:${requestId}`, requestEvent);
    } else if (alpineGate && ["Network.loadingFinished", "Network.loadingFailed"].includes(message.method)) {
      const { requestId, encodedDataLength, errorText } = message.params;
      const request = networkRequests.get(`${message.sessionId ?? "page"}:${requestId}`);
      if (request) Object.assign(request, { encodedDataLength, errorText });
    } else if (message.method === "Browser.downloadWillBegin") {
      downloads.push(message.params);
    } else if (message.method === "Browser.downloadProgress") {
      downloadProgress.set(message.params.guid, message.params);
    } else if (message.method === "Page.javascriptDialogOpening") {
      dialogs.push(message.params);
    } else if (message.method === "Runtime.bindingCalled" && message.params.name === "observeHesper") {
      const event = JSON.parse(message.params.payload);
      if (diskGate || initramfsGate || alpineGate) workerEvents.push(event);
      if (event.request) return;
      replies.push(event);
      if (event.sessionId !== streaming) return;
      if (event.type === "stdout") {
        serialBytes += event.bytes.length;
        emit({ type: "serial", bytes: Buffer.from(event.bytes).toString("base64") });
      } else if (event.type === "error") emit({ type: "error", message: event.message });
    }
  };
  socket.onmessage = onmessage;
  if (browserSocket) browserSocket.onmessage = onmessage;
  const call = (method, params = {}, ms = 12_000, connection = socket, sessionId) => new Promise((done, fail) => {
    const id = ++sequence;
    const timer = setTimeout(() => { requests.delete(id); fail(new Error(`${method}: deadline`)); }, ms);
    requests.set(id, message => {
      requests.delete(id);
      clearTimeout(timer);
      if (message.error) fail(new Error(`${method}: ${message.error.message}`));
      else done(message.result);
    });
    connection.send(JSON.stringify({ id, method, params, sessionId }));
  });
  async function observeTarget({ sessionId, targetInfo }) {
    try {
      // Enable network observation before the production Worker starts any requests.
      if (targetInfo.type === "worker") await call("Network.enable", {}, 12_000, socket, sessionId);
      await call("Runtime.runIfWaitingForDebugger", {}, 12_000, socket, sessionId);
    } catch (error) {
      networkError = String(error);
      emit({ type: "error", message: `Chromium target monitoring: ${networkError}` });
    }
  }
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
  async function waitFor(predicate, name) {
    const started = performance.now();
    while (performance.now() - started < 8000) {
      const result = predicate();
      if (result) return result;
      await delay(12);
    }
    throw new Error(`Chrome did not report ${name}`);
  }
  const enabled = selector => `(() => {
    const control = document.querySelector(${JSON.stringify(selector)});
    return !!control && !control.disabled && control.checkVisibility();
  })()`;
  const profilePicker = 'select[data-linux-profile][aria-label="启动配置"]';
  const picker = `document.querySelector(${JSON.stringify(profilePicker)})`;
  async function selectProfile(value) {
    const key = value === "alpine" ? "End" : "Home";
    check(await evaluate(`${enabled(profilePicker)} && (${picker}.focus(), document.activeElement === ${picker})`),
      "could not focus the launch configuration selector with the session stopped");
    await call("Input.dispatchKeyEvent", { type: "rawKeyDown", key, code: key, windowsVirtualKeyCode: key === "End" ? 35 : 36 });
    await call("Input.dispatchKeyEvent", { type: "keyUp", key, code: key });
    await until(`${picker}.value === ${JSON.stringify(value)} &&
      document.querySelector('[data-linux-files]').files.length === 0 && !${enabled("[data-linux-start]")}`,
      `keyboard selection of ${value} with cleared local files`);
  }
  async function controlPoint(selector, scroll) {
    const point = await evaluate(`(() => {
      const target = document.querySelector(${JSON.stringify(selector)});
      if (!target || target.disabled || !target.checkVisibility()) return null;
      if (${scroll}) target.scrollIntoView({block:'center'});
      const rect = target.getBoundingClientRect();
      const x = rect.x + rect.width/2, y = rect.y + rect.height/2;
      return target.contains(document.elementFromPoint(x, y)) ? {x, y} : null;
    })()`);
    check(point, `control is unavailable: ${selector}`);
    return point;
  }
  function mouse(point, type) {
    return call("Input.dispatchMouseEvent", { ...point, type, button: "left", clickCount: 1 });
  }
  async function click(selector) {
    const point = await controlPoint(selector, true);
    await mouse(point, "mousePressed");
    await mouse(point, "mouseReleased");
  }
  async function setFiles(selector, files) {
    const { root } = await call("DOM.getDocument");
    const { nodeId } = await call("DOM.querySelector", { nodeId: root.nodeId, selector: `${selector}[type=file]` });
    check(nodeId, `native file input missing: ${selector}`);
    await call("DOM.setFileInputFiles", { nodeId, files });
  }
  async function selectFiles(directory, profile = lock, override = {}) {
    await setFiles("[data-linux-files]", profile.images.map(entry => override[entry.path] ?? join(directory, entry.path)));
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
  if (alpineGate) {
    await call("Network.enable");
    await call("Target.setAutoAttach", { autoAttach: true, waitForDebuggerOnStart: true, flatten: true,
      filter: [{ type: "worker" }, { exclude: true }] });
  }
  await call("Runtime.addBinding", { name: "observeHesper" });
  // Passively record actual Worker replies alongside the unmodified app handler.
  // No synthetic worker, reply, guest state, or rendering.
  await call("Page.addScriptToEvaluateOnNewDocument", { source: `(() => {
    const Worker = globalThis.Worker;
    globalThis.Worker = class extends Worker {
      #observedSessionId;
      constructor(...args) {
        super(...args);
        this.addEventListener('message', ({data}) => {
          if (!data || typeof data.sessionId !== 'number') return;
          if (data.type === 'disk') {
            observeHesper(JSON.stringify({sessionId: data.sessionId, type: data.type,
              byteLength: data.bytes?.byteLength, byteKind: data.bytes?.constructor?.name}));
          } else {
            observeHesper(JSON.stringify(data, (key, value) => typeof value === 'bigint' ? String(value)
              : value instanceof Uint8Array ? [...value] : value));
          }
        });
      }
      postMessage(message, ...transfer) {
        if (${diskGate || initramfsGate || alpineGate} && typeof message?.sessionId === 'number' && message.type !== 'stdin') {
          this.#observedSessionId = message.sessionId;
          const file = value => value instanceof File ? {name: value.name, size: value.size} : undefined;
          observeHesper(JSON.stringify({request: true, sessionId: message.sessionId, type: message.type,
            files: Object.values(message).flat().filter(value => value instanceof File).map(file),
            initramfs: file(message.initramfs), disk: file(message.disk), profile: message.profile}));
        }
        return super.postMessage(message, ...transfer);
      }
      terminate() {
        if (this.#observedSessionId !== undefined) {
          observeHesper(JSON.stringify({request: true, sessionId: this.#observedSessionId, type: 'terminate'}));
        }
        return super.terminate();
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
  if (diskGate) await runDiskGate();
  else if (initramfsGate) await runInitramfsGate();
  else if (alpineGate) await runAlpineGate();
  else if (benchmark) await runBenchmark();
  else await runSerialGate();

  async function runBenchmark() {
    const start = "[data-linux-start]";
    const result = "HESPER_BENCH:42";
    await selectProfile("alpine");
    if (benchDisk) await setFiles("[data-linux-disk]", [benchDisk]);
    const imageBytes = await selectFiles(images);
    await evaluate(`(() => {
      const clock = () => performance.timeOrigin + performance.now();
      const sample = globalThis.hesperBootBenchmark = {};
      document.addEventListener("click", () => {
        sample.clicked = clock();
        const observer = new MutationObserver(() => {
          const rows = ${tty()}.split("\\n");
          if (!rows.some(row => row.startsWith("hesper# "))) return;
          sample.prompt ??= clock();
          const proof = rows.findIndex(row => row.trimEnd() === ${JSON.stringify(result)});
          if (proof >= 0 && rows.slice(proof + 1).some(row => row.startsWith("hesper# ") &&
              row.trimEnd() === "hesper#")) {
            sample.command = clock();
            observer.disconnect();
          }
        });
        observer.observe(document.querySelector("[data-linux-terminal]"), {
          subtree: true, childList: true, characterData: true,
        });
      }, { capture: true, once: true });
    })()`);
    streaming = 1;
    emit({ type: "start", browser: version.Browser, imageBytes });
    await click(start);
    for await (const line of createInterface({ input: process.stdin })) {
      const request = JSON.parse(line);
      if (request.type === "keys") {
        await until("globalThis.hesperBootBenchmark.prompt", "complete first xterm prompt");
        await type(request.text);
      } else if (request.type === "finish") {
        try {
          await until("globalThis.hesperBootBenchmark.command", "independent command result and second xterm prompt");
        } catch (error) {
          writeFileSync(join(logs, "browser-terminal.txt"), await evaluate(tty()));
          throw error;
        }
        const timing = await evaluate("globalThis.hesperBootBenchmark");
        const initial = replies.find(reply => reply.sessionId === 1 && reply.state === "running");
        check(initial?.time === "0" && initial.retired === "0" && initial.fuel === "0",
          "benchmark did not start from a fresh machine");
        await click("[data-linux-stop]");
        await until(state("ended"), "stopped benchmark session");
        const ended = await waitFor(() => replies.find(reply => reply.sessionId === 1 && reply.state === "ended"),
          "final benchmark counters");
        check(guestRequests.length === 0, "benchmark fetched guest images over HTTP");
        emit({ type: "done", timing, fuel: ended.fuel, retired: ended.retired, tick: ended.time,
          stages: replies.filter(reply => reply.sessionId === 1 && reply.type === "benchmark") });
        return;
      } else throw new Error(`unknown benchmark request ${request.type}`);
    }
  }

  async function runAlpineGate() {
    const [standardImages, legacyImages, damagedKernel] = input;
    const legacyLock = JSON.parse(readFileSync(new URL("images-alpine.lock.json", import.meta.url), "utf8"));
    const standardLock = JSON.parse(readFileSync(new URL("images.lock.json", import.meta.url), "utf8"));
    const files = "[data-linux-files]", initramfs = "[data-linux-initramfs]", disk = "[data-linux-disk]";
    const start = "[data-linux-start]", stop = "[data-linux-stop]", error = "[data-linux-error]";
    const rows = `${tty()}.split('\\n').map(line => line.trimEnd())`;
    const url = `http://127.0.0.1:${server.port}${tab}`;
    const phases = {}, rejected = {};
    const matches = id => replies.filter(reply => reply.sessionId === id);
    const inactive = id => matches(id).every(reply => !["ready", "stdout"].includes(reply.type));
    const nextId = () => 1 + (workerEvents.findLast(event => event.request && event.type === "boot")?.sessionId ?? 0);

    async function chooseProfile(value) {
      await selectProfile(value);
      const customRoot = value === "standard";
      check(await evaluate(`Boolean(${enabled(initramfs)}) === ${customRoot} && ${enabled(disk)} && !${visible(error)}`),
        `optional pickers did not match the ${value} profile`);
      if (!customRoot) {
        check(!await evaluate(visible(initramfs)), "Alpine exposed a custom root picker");
        const required = await evaluate(`Array.from(${picker}.closest('section').querySelectorAll('ul code'), item => item.textContent)`);
        check(JSON.stringify(required?.toSorted()) === JSON.stringify(lock.images.map(item => item.path).toSorted()),
          "Alpine did not list exactly the four required image files");
      }
    }

    async function capture(label) {
      const path = join(logs, `browser-${label}.png`);
      const screenshot = await call("Page.captureScreenshot", { format: "png" });
      writeFileSync(path, Buffer.from(screenshot.data, "base64"));
      return path;
    }

    async function bootRequest(id, profile, profileLock, directory, override = {}) {
      const request = await waitFor(() => workerEvents.find(event =>
        event.request && event.type === "boot" && event.sessionId === id), `Worker boot ${id}`);
      const expected = profileLock.images.map(image => ({
        name: image.path, size: statSync(override[image.path] ?? join(directory, image.path)).size,
      }));
      const byName = (left, right) => left.name.localeCompare(right.name);
      check(request.profile === profile && request.initramfs === undefined && request.disk === undefined &&
        JSON.stringify(request.files.toSorted(byName)) === JSON.stringify(expected.toSorted(byName)),
        `Worker did not receive the four selected ${profile} files for session ${id}`);
    }

    async function openPhase(phase, profile, directory, profileLock, reuse = false) {
      const imageBytes = reuse
        ? await evaluate(`Array.from(document.querySelector('${files}').files, file => file.size).reduce((a,b) => a+b, 0)`)
        : await selectFiles(directory, profileLock);
      streaming = nextId();
      const id = streaming;
      // Mark the serial stream before clicking, even if output precedes CDP's click reply.
      emit({ type: "start", phase, profile, id, browser: version.Browser, url, imageBytes });
      await click(start);
      await bootRequest(id, profile, profileLock, directory);
      phases[phase] = { id, profile, imageBytes };
    }

    async function closePhase(phase, expectedLine) {
      const id = streaming;
      const rendered = `Array.from(document.querySelectorAll('[data-linux-terminal] .xterm-rows > div'))
        .find(row => row.textContent.trimEnd() === ${JSON.stringify(expectedLine)})`;
      await until(rendered, `exact ${expectedLine} line in the xterm DOM`);
      const xtermLine = await evaluate(`(() => {
        const row = ${rendered};
        row.scrollIntoView({block:'nearest'});
        return row.textContent.trimEnd();
      })()`);
      if (phase !== "install") {
        check(!await evaluate(`${rows}.includes("7.500")`), `stale Alpine output in ${phase} terminal`);
      }
      check(await evaluate(`${picker}.disabled && document.querySelector('${files}').disabled`),
        "the launch configuration or local files could change during the running session");
      const replies = matches(id);
      const initial = replies.find(reply => reply.type === "state" && reply.state === "running");
      check(replies.filter(reply => reply.type === "ready").length === 1 &&
        initial?.time === "0" && initial.retired === "0" && initial.fuel === "0",
        `session ${id} lacked a fresh, ready machine`);
      const screenshot = await capture(phase);
      await click(stop);
      await until(state("ended"), `Stop ended ${phase}`);
      const ended = await waitFor(() => matches(id).find(reply => reply.type === "state" && reply.state === "ended"),
        `Worker end state for ${phase}`);
      check(workerEvents.some(event => event.request && event.type === "stop" && event.sessionId === id) &&
        matches(id).every(reply => reply.type !== "error"), `session ${id} did not stop normally`);
      check(BigInt(ended.fuel) > 0n, `session ${id} did not report cumulative transitions`);
      Object.assign(phases[phase], { ready: true, initialTick: initial.time, initialRetired: initial.retired,
        xtermLine, screenshot, exitState: ended.state, tick: ended.time, retired: ended.retired,
        fuel: ended.fuel });
      streaming = undefined;
    }

    async function reject(label, directory, profileLock, expected, override = {}) {
      const imageBytes = await selectFiles(directory, profileLock, override);
      const id = nextId();
      await click(start);
      await bootRequest(id, "alpine", profileLock, directory, override);
      await until(`${state("failed")} && ${visible(error)}`, `Alpine rejection of ${label}`);
      const message = await evaluate(`document.querySelector('${error}').textContent.trim()`);
      check(message.includes(expected) && inactive(id) && await evaluate(`${tty()}.trim() === ""`),
        `${label} did not fail before guest output: ${message}`);
      const failed = matches(id).find(reply => reply.type === "state" && reply.state === "failed");
      check(failed?.time === "0" && failed.retired === "0" && failed.fuel === "0" &&
        matches(id).some(reply => reply.type === "error" && reply.message === message),
        `Worker did not report pre-boot validation failure for ${label}`);
      rejected[label] = { id, message, imageBytes, ready: false, serialBytes: 0, state: failed.state,
        screenshot: await capture(`rejected-${label}`) };
    }

    check(await evaluate(`${picker}?.value === 'standard'`), "the existing standard default profile changed");
    await selectFiles(standardImages, standardLock);
    await chooseProfile("alpine");
    let phase = "install";
    await openPhase(phase, "alpine", images, lock);
    for await (const line of createInterface({ input: process.stdin })) {
      const request = JSON.parse(line);
      if (request.type === "keys") {
        await type(request.text);
      } else if (request.type === "cold" && phase === "install") {
        await closePhase(phase, "7.500");
        phase = "cold";
        await openPhase(phase, "alpine", images, lock, true);
      } else if (request.type === "legacy" && phase === "cold") {
        await closePhase(phase, request.resultLine);
        await reject("stock-images", standardImages, standardLock, "initramfs.cpio: expected");
        await reject("old-root", images, lock, "initramfs.cpio: expected",
          { "initramfs.cpio": join(legacyImages, "initramfs.cpio") });
        await reject("corrupt-kernel", images, lock, "Image: SHA-256", { Image: damagedKernel });
        phase = "legacy";
        await openPhase(phase, "alpine", legacyImages, legacyLock);
      } else if (request.type === "standard" && phase === "legacy") {
        await closePhase(phase, request.resultLine);
        await chooseProfile("standard");
        phase = "standard";
        await openPhase(phase, "standard", standardImages, standardLock);
      } else if (request.type === "finish" && phase === "standard") {
        await closePhase(phase, "HESPER_OK:42");
        check(Object.values(rejected).every(({ id }) => inactive(id)), "a rejected session later emitted Ready or serial");
        const guestNetwork = networkTrace.filter(request => request.guest);
        const guestNetworkBytes = guestNetwork.reduce((total, request) => total + (request.encodedDataLength ?? 0), 0);
        check(!networkError && networkTrace.some(request => request.target !== "page" &&
          /\/hesper_riscv_linux_bg-[^/]+\.wasm$/.test(new URL(request.url).pathname)),
          "Chromium did not observe a production Linux Worker Wasm request");
        check(guestRequests.length === 0 && guestDownloadedBytes === 0 && guestNetwork.length === 0 &&
          guestNetworkBytes === 0, `guest resources requested over HTTP: ${JSON.stringify({ guestRequests, guestNetwork })}`);
        check(servedWasm.size === 1, "sessions did not load one production Wasm asset");
        const maxIntervalMs = await evaluate("globalThis.hesperMaxIntervalMs");
        emit({ type: "done", phases, rejected, maxIntervalMs, wasmPath: [...servedWasm][0],
          guestRequests: guestRequests.length, guestDownloadedBytes, guestNetworkRequests: guestNetwork.length,
          guestNetworkBytes });
        return;
      } else throw new Error(`unexpected Alpine browser gate request ${request.type} during ${phase}`);
    }
    throw new Error(`browser input closed during Alpine ${phase} phase`);
  }

  async function runInitramfsGate() {
    const [serialImages, fixture, disk, oversized, damagedBoard, malformed] = input;
    const serialLock = JSON.parse(readFileSync(new URL("images.lock.json", import.meta.url), "utf8"));
    const files = "[data-linux-files]", picker = "[data-linux-initramfs]", diskPicker = "[data-linux-disk]";
    const start = "[data-linux-start]", stop = "[data-linux-stop]", clear = "[data-linux-clear-initramfs]";
    const pickerError = "[data-linux-initramfs-error]";
    const basenameIs = (selector, value) => `document.querySelector('${selector}').files[0]?.name === ${JSON.stringify(basename(value))}`;
    const text = selector => `document.querySelector('${selector}')?.textContent.trim()`;
    const nextId = () => 1 + (workerEvents.findLast(event => event.request && event.type === "boot")?.sessionId ?? 0);
    const bootRequest = sessionId => waitFor(() => workerEvents.find(event =>
      event.request && event.type === "boot" && event.sessionId === sessionId), `boot request for session ${sessionId}`);
    const inactive = id => replies.every(reply => reply.sessionId !== id || !["ready", "stdout"].includes(reply.type));
    const paths = (directory, profile, full = false) => profile.images
      .filter(entry => full || entry.path !== "initramfs.cpio").map(entry => join(directory, entry.path));
    const serialBase = paths(serialImages, serialLock);
    const serialFull = paths(serialImages, serialLock, true);
    const blkBase = paths(images, lock);
    const expectedCustom = "ROOT_OK:custom-v1:42";
    const negative = {}, phases = {};
    const url = `http://127.0.0.1:${server.port}${tab}`;

    async function chooseCustom(path) {
      await setFiles(picker, [path]);
      await until(`${basenameIs(picker, path)} && ${enabled(clear)} && !${visible(pickerError)}`, "local custom initramfs");
    }

    async function invalidLoad(name, selector, expected, disabled = false) {
      const id = nextId();
      await click(start);
      await until(`${state("failed")} && ${visible(selector)}`, `${name} rejection before guest execution`);
      const message = await evaluate(text(selector));
      check(message.includes(expected) && inactive(id), `${name} did not fail before guest execution: ${message}`);
      if (disabled) check(!await evaluate(enabled(start)), `${name} left Start enabled`);
      negative[name] = message;
    }

    async function openPhase(phase, custom, dataDisk = false) {
      const id = nextId();
      const imageBytes = await evaluate(`Array.from(document.querySelector('${files}').files, file => file.size).reduce((a,b) => a+b, 0)`);
      streaming = id;
      // Mark the serial stream before clicking: Chromium can boot while CDP
      // delivers the response to the click or the passive request observer.
      emit({ type: "start", phase, browser: version.Browser, url, imageBytes });
      await click(start);
      const request = await bootRequest(id);
      check(custom ? request.initramfs?.name === basename(fixture) && request.initramfs.size === statSync(fixture).size
        : request.initramfs === undefined, `Worker did not boot with the intended archive for ${phase}`);
      check(dataDisk ? request.disk?.name === basename(disk) && request.disk.size === statSync(disk).size
        : request.disk === undefined, `Worker did not boot with the intended disk for ${phase}`);
      phases[phase] = { id, imageBytes, custom: request.initramfs, disk: request.disk };
    }

    async function closePhase(phase, custom) {
      const id = streaming;
      if (custom) await until(`${tty()}.includes('${expectedCustom}')`, "rendered custom proof");
      else {
        await until(`${tty()}.includes('/bin/root-proof: not found')`, "rendered default root proof rejection");
        check(!await evaluate(`${tty()}.includes('ROOT_OK:')`), "stale custom output in default terminal");
      }
      check(!await evaluate(`${enabled(files)} || ${enabled(picker)} || ${enabled(diskPicker)} || ${enabled(clear)}`),
        "Linux boot inputs can be changed while the session is live");
      const screenshot = await call("Page.captureScreenshot", { format: "png" });
      const path = join(logs, `browser-${phase}.png`);
      writeFileSync(path, Buffer.from(screenshot.data, "base64"));
      await click(stop);
      await until(state("ended"), `stopped ${phase}`);
      const ended = await waitFor(() => replies.find(reply => reply.sessionId === id && reply.state === "ended"),
        `Worker end state for ${phase}`);
      Object.assign(phases[phase], { screenshot: path, exitState: ended.state, tick: ended.time, retired: ended.retired });
      streaming = undefined;
    }

    await setFiles(files, serialBase);
    await chooseCustom(fixture);
    const cancelled = nextId();
    await click(start);
    const requested = await bootRequest(cancelled);
    check(requested.initramfs?.size === statSync(fixture).size, "cancelled session did not select the reference fixture");
    const pendingWasm = await waitFor(() => firstLinuxWasm, "outstanding production Wasm response");
    check(await evaluate(state("loading")), "custom session A was not loading before cancellation");
    await click(stop);
    await until(state("ended"), "cancelled custom session A");
    await waitFor(() => workerEvents.some(event => event.request && event.type === "terminate" && event.sessionId === cancelled),
      "termination of loading Worker A");
    releaseFirstLinuxWasm();
    await click(clear);
    await until(`document.querySelector('${picker}').files.length === 0 && !${enabled(start)} &&
      ${text("[data-linux-files-error]")}?.includes('initramfs.cpio')`, "missing default cpio after clearing");
    negative["missing-after-clear"] = await evaluate(text("[data-linux-files-error]"));

    await setFiles(files, serialFull);
    await until(enabled(start), "four default serial images for B");
    let phase = "default-after-cancel";
    await openPhase(phase, false);
    for await (const line of createInterface({ input: process.stdin })) {
      const request = JSON.parse(line);
      if (request.type === "keys") {
        await type(request.text);
      } else if (request.type === "custom-serial" && phase === "default-after-cancel") {
        await closePhase(phase, false);
        check(inactive(cancelled), "cancelled session A emitted Ready or serial output during default B");
        const next = nextId();
        await setFiles(picker, [oversized]);
        await until(`${text(pickerError)}?.includes('16 MiB') && !${enabled(start)}`, "local oversized custom archive rejected");
        check(nextId() === next, "oversized selection started a Worker");
        negative.oversized = await evaluate(text(pickerError));
        await click(clear);
        await until(enabled(start), "default Start restored after clearing invalid selection");

        await chooseCustom(fixture);
        await setFiles(files, serialBase.map(path => basename(path) === "board.dtb" ? damagedBoard : path));
        await invalidLoad("damaged-dtb", "[data-linux-error]", "board.dtb: SHA-256");
        await setFiles(files, serialBase);
        await chooseCustom(malformed);
        await invalidLoad("malformed-archive", pickerError, "malformed custom initramfs", true);
        await chooseCustom(fixture);
        phase = request.type;
        await openPhase(phase, true);
      } else if (request.type === "custom-blk" && phase === "custom-serial") {
        await closePhase(phase, true);
        await setFiles(diskPicker, [disk]);
        await until(basenameIs(diskPicker, disk), "local ext2 data disk");
        await invalidLoad("wrong-profile", "[data-linux-error]", "Image: expected");
        // Even an unusable default cpio must be ignored in explicit replacement mode.
        await setFiles(files, [...blkBase, malformed]);
        await until(`${visible("[data-linux-ignored-initramfs]")} && ${enabled(start)}`, "four base files with unused default cpio");
        phase = request.type;
        await openPhase(phase, true, true);
      } else if (request.type === "finish" && phase === "custom-blk") {
        await closePhase(phase, true);
        check(inactive(cancelled), "cancelled session A emitted Ready or serial output");
        check(guestRequests.length === 0 && guestDownloadedBytes === 0, `guest resources requested over HTTP: ${guestRequests.join(", ")}`);
        emit({ type: "done", phases, negative, cancelled: { id: cancelled, pendingWasm, noReadyOrOutput: true },
          guestRequests: guestRequests.length, guestDownloadedBytes });
        break;
      } else throw new Error(`unexpected browser initramfs gate request ${request.type} during ${phase}`);
    }
  }

  async function runDiskGate() {
    const diskInput = "[data-linux-disk]";
    const clear = "[data-linux-clear-disk]";
    const diskError = "[data-linux-disk-error]";
    const exporter = "[data-linux-export]";
    const startButton = "[data-linux-start]";
    const stop = "[data-linux-stop]";
    const diskFiles = `document.querySelector('${diskInput}').files`;
    const downloadName = "hesper-data.raw";
    const exportLabels = ["first", "after-resume"];
    const completed = [];
    const sendRequest = event => event.request && event.sessionId === streaming;
    const pausedSnapshots = [];
    const nextId = () => 1 + (workerEvents.findLast(event => event.request && event.type === "boot")?.sessionId ?? 0);
    const alpineSerialLock = alpineDiskGate && readLock("images-alpine-lts.lock.json");
    const standardLock = alpineDiskGate && readLock("images.lock.json");
    let paused;
    let race;
    let rejected;
    let dirtyStop;

    async function selectDisk(path) {
      await setFiles(diskInput, [path]);
      await until(`${diskFiles}.length === 1 && ${diskFiles}[0].size === ${statSync(path).size}
        && ${enabled(clear)} && !${visible(diskError)}`, "accepted disk file");
    }

    async function boot(path, phase, profile = alpineDiskGate ? "alpine" : "standard") {
      if (path) await selectDisk(path);
      streaming = nextId();
      const bytes = await evaluate("Array.from(document.querySelector('[data-linux-files]').files, file => file.size).reduce((a,b) => a+b, 0)");
      emit({ type: "start", phase, browser: version.Browser, imageBytes: bytes,
        url: `http://127.0.0.1:${server.port}${tab}` });
      await click(startButton);
      const boot = await waitFor(() => workerEvents.find(event => sendRequest(event) && event.type === "boot"), `boot for ${phase}`);
      check(boot.profile === profile && (path
        ? boot.disk?.name === basename(path) && boot.disk.size === statSync(path).size
        : boot.disk === undefined), "Worker boot did not use the selected profile and disk");
    }

    const counters = () => evaluate(`[document.querySelector('[data-linux-time]')?.dataset.linuxTime,
      document.querySelector('[data-linux-retired]')?.dataset.linuxRetired]`);
    async function pauseDisk() {
      check(!await evaluate(enabled(exporter)), "disk export was enabled while the guest could advance");
      const before = workerEvents.length;
      await click("[data-linux-pause]");
      await until(`${state("paused")} && ${enabled(exporter)}`, "paused session with export enabled");
      const snapshot = await counters();
      check(snapshot.every(value => /^\d+$/.test(value)), "paused session lacks numeric guest counters");
      const reply = workerEvents.slice(before).find(event => !event.request && event.sessionId === streaming &&
        event.type === "state" && event.state === "paused");
      check(reply?.time === snapshot[0] && reply?.retired === snapshot[1], "paused counters did not match the Worker");
      pausedSnapshots.push(snapshot);
      paused = { snapshot, serial: serialBytes };
    }

    async function downloadDirectory(label) {
      const directory = join(logs, `download-${label}`);
      await call("Browser.setDownloadBehavior", { behavior: "allow", downloadPath: directory, eventsEnabled: true },
        12_000, browserSocket);
      return directory;
    }

    async function exportDisk(label) {
      check(label === exportLabels[completed.length], `unexpected export label: ${label}`);
      const directory = await downloadDirectory(label);
      const start = downloads.length;
      const since = workerEvents.length;
      await pauseDisk();
      await click(exporter);
      const begin = await waitFor(() => downloads[start], "disk download start");
      const progress = await waitFor(() => {
        const status = downloadProgress.get(begin.guid);
        return status && status.state !== "inProgress" && status;
      }, "disk download completion");
      check(progress.state === "completed", `Chrome canceled disk download: ${progress.state}`);
      const path = join(directory, downloadName);
      const length = statSync(webDisk).size;
      const disk = workerEvents.slice(since).find(event => !event.request && event.sessionId === streaming && event.type === "disk");
      check(disk?.byteLength === length, "Worker did not return a complete disk for this session");
      check(begin.suggestedFilename === downloadName && begin.url.startsWith("blob:") &&
        progress.receivedBytes === length && statSync(path).size === length && downloads.length === start + 1,
        "Chrome did not complete exactly one correctly named local raw disk download");
      completed.push(begin.guid);
      emit({ type: "download", label, filename: begin.suggestedFilename, bytes: length, workerBytes: disk.byteLength });
    }

    async function resumeWithKeys(text) {
      await type(text);
      await delay(150);
      const snapshot = await counters();
      check(snapshot.every((value, i) => value === paused.snapshot[i]) && serialBytes === paused.serial,
        "queued disk command advanced the guest while paused");
      const since = workerEvents.length;
      const [time, retired] = paused.snapshot;
      await click("[data-linux-resume]");
      const resumed = await waitFor(() => workerEvents.slice(since).find(event => !event.request &&
        event.sessionId === streaming && event.type === "state" && event.state === "running"), "resumed guest state");
      check(resumed.time === time && resumed.retired === retired, "Continue did not preserve paused guest counters");
      paused = undefined;
      check(!await evaluate(enabled(exporter)), "disk export stayed enabled after Continue");
    }

    async function raceStop() {
      const directory = await downloadDirectory("race");
      const since = workerEvents.length;
      const start = downloads.length;
      await pauseDisk();
      const exportPoint = await controlPoint(exporter, false);
      const stopPoint = await controlPoint(stop, false);
      // Queue genuine CDP mouse events together: either the Worker reply or the
      // following Stop can win. No event or reply is delayed, reordered, or injected.
      await Promise.all([
        mouse(exportPoint, "mousePressed"), mouse(exportPoint, "mouseReleased"),
        mouse(stopPoint, "mousePressed"), mouse(stopPoint, "mouseReleased"),
      ]);
      await until(state("ended"), "Stop following an export request");
      await delay(350);
      const trace = workerEvents.slice(since).filter(event => event.sessionId === streaming &&
        ["export", "stop", "terminate", "disk"].includes(event.type));
      const requested = trace.findIndex(event => event.request && event.type === "export");
      const stopped = trace.findIndex(event => event.request && ["stop", "terminate"].includes(event.type));
      const reply = trace.findIndex(event => !event.request && event.type === "disk");
      const count = downloads.length - start;
      check(requested >= 0 && stopped > requested, "Export and Stop requests were not observed in order");
      check(reply >= 0 && reply < stopped ? count <= 1 : count === 0 && readdirSync(directory).length === 0,
        "a disk reply not received before Stop caused a download");
      check(!await evaluate(enabled(exporter)), "export stayed enabled after Stop");
      race = { trace, downloads: count, outcome: reply < 0 ? "no-disk-reply-observed"
        : reply < stopped ? "reply-before-stop" : "reply-after-stop-no-download" };
      streaming = undefined;
      paused = undefined;
    }

    async function rejectOrDiscard(accept) {
      const before = dialogs.length;
      const since = workerEvents.length;
      const pressed = click(stop);
      const dialog = await waitFor(() => dialogs[before], "dirty-stop confirmation");
      check(dialog.type === "confirm" && !!dialog.message, "dirty Stop did not open a native confirmation");
      check(workerEvents.slice(since).some(event => !event.request && event.sessionId === streaming &&
        event.type === "discard-required"), "Stop confirmation did not follow a dirty Worker reply");
      await call("Page.handleJavaScriptDialog", { accept });
      await pressed;
      if (accept) await until(state("ended"), "confirmed dirty discard");
      else await until(`${state("paused")} && ${enabled(stop)}`, "paused session retained after declining discard");
      return dialog.message;
    }

    if (alpineDiskGate) {
      await selectProfile("alpine");
      await selectFiles(alpineSerialImages, alpineSerialLock);
      await selectDisk(cliDisk);
      const id = nextId();
      await click(startButton);
      const error = "[data-linux-error]";
      await until(`${state("failed")} && ${visible(error)}`, "Alpine requires the matching data-disk images");
      const message = await evaluate(`document.querySelector('${error}').textContent.trim()`);
      const events = replies.filter(reply => reply.sessionId === id);
      check(message.includes("requires images-alpine-lts-blk") &&
        events.some(reply => reply.type === "error" && reply.message === message) &&
        events.every(reply => !["ready", "stdout"].includes(reply.type)), "Alpine accepted no-disk images with a disk");
      rejected = { id, message };
    }
    await selectFiles(images);
    await selectDisk(cliDisk);
    await click(clear);
    await until(`${diskFiles}.length === 0 && !${enabled(clear)}`, "cleared native disk selection");
    check(!await evaluate(enabled(exporter)), "cleared disk left export enabled");
    const beforeOversize = workerEvents.length;
    await setFiles(diskInput, [oversized]);
    await until(`${visible(diskError)} && document.querySelector('${diskError}').textContent.trim().length > 0`,
      "oversized disk rejected by the file picker");
    const oversizedError = await evaluate(`document.querySelector('${diskError}').textContent.trim()`);
    check(workerEvents.length === beforeOversize, "oversized disk selection contacted a Worker");
    await boot(cliDisk, "cli-to-web");
    async function verifyDirtyStop() {
      check(completed.length === exportLabels.length && race, "missing completed disk exports");
      const declined = await rejectOrDiscard(false);
      const accepted = await rejectOrDiscard(true);
      await delay(350);
      check(downloads.length === completed.length + race.downloads, "unexpected download during or after dirty Stop");
      const screenshot = await call("Page.captureScreenshot", { format: "png" });
      writeFileSync(join(logs, "browser-terminal.png"), Buffer.from(screenshot.data, "base64"));
      return { declined, accepted };
    }
    async function stopSerial() {
      check(!await evaluate(enabled(exporter)), "disk export enabled for a serial-only guest");
      await click(stop);
      await until(state("ended"), "Stop ended the serial-only session");
      streaming = undefined;
    }

    for await (const line of createInterface({ input: process.stdin })) {
      const request = JSON.parse(line);
      if (request.type === "keys") {
        if (paused) await resumeWithKeys(request.text);
        else await type(request.text);
      } else if (request.type === "write-phase") {
        check(race === undefined, "repeated write phase");
        if (alpineDiskGate) {
          await until(`${tty()}.split('\\n').some(line => line.trimEnd() === 'CLI_TO_WEB:42')`,
            "xterm rendered the script from the CLI disk");
        }
        await raceStop();
        await boot(webDisk, "web-to-cli");
      } else if (request.type === "export") {
        await exportDisk(request.label);
      } else if (request.type === "alpine-serial" && alpineDiskGate) {
        dirtyStop = await verifyDirtyStop();
        paused = undefined;
        await click(clear);
        await selectFiles(alpineSerialImages, alpineSerialLock);
        await boot(undefined, "alpine-serial");
      } else if (request.type === "standard" && alpineDiskGate) {
        await stopSerial();
        await selectProfile("standard");
        await selectFiles(standardImages, standardLock);
        await boot(undefined, "standard", "standard");
      } else if (request.type === "finish") {
        if (alpineDiskGate) await stopSerial();
        else dirtyStop = await verifyDirtyStop();
        check(replies.every(reply => reply.type !== "error" || reply.sessionId === rejected?.id),
          "Worker reported an error during disk acceptance");
        check(guestDownloadedBytes === 0 && guestRequests.length === 0,
          "the site requested guest files over HTTP");
        emit({ type: "done", race, oversizedError, ...dirtyStop, rejected, pausedSnapshots, guestDownloadedBytes });
        break;
      } else throw new Error(`unknown browser disk gate request ${request.type}`);
    }
  }

  async function runSerialGate() {
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
          await delay(150);
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
        const retire = await waitFor(() => replies.findLast(reply => reply.sessionId === 3 && reply.state === "ended"),
          "final state after exceeding the former transition limit");
        check(BigInt(retire.fuel) > 450_000_000n, "standard session did not exceed the former transition limit");
        await click("[data-linux-start]");
        await until(`${tty()}.includes('OpenSBI v1.6')`, "fresh firmware after destroying the session");
        check(!(await evaluate(`${tty()}.includes('HESPER_OK:42')`)), "old terminal output leaked into the restart");
        check(replies.some(reply => reply.sessionId === 4 && reply.state === "running" && reply.time === "0" && reply.retired === "0"), "restart lacked an initial zero-time, zero-retirement state");
        await click("[data-linux-stop]");
        await until(state("ended"), "ended fresh session");
        check(replies.every(reply => (reply.sessionId !== 1 && reply.sessionId !== 2) || reply.type !== "stdout"), "stale output from cancelled or corrupt boot");
        check(guestDownloadedBytes === 0, "the site fetched guest images instead of reading local Files");
        emit({ type: "done", pausedTick: paused, maxIntervalMs, retired: retire.retired, tick: retire.time, fuel: retire.fuel, guestDownloadedBytes });
        break;
      } else throw new Error(`unknown browser gate request ${request.type}`);
    }
  }
} catch (error) {
  emit({ type: "error", message: String(error) });
  console.error(error);
  process.exitCode = 1;
} finally {
  process.stdin.destroy();
  releaseFirstLinuxWasm();
  socket?.close();
  browserSocket?.close();
  await server.stop(true);
  child.kill();
  await child.exited;
  writeFileSync(join(logs, "browser-chromium.stderr.log"), chromeError);
  if (alpineGate) {
    writeFileSync(join(logs, "browser-worker-events.jsonl"), alpineWorkerEvents.map(event => JSON.stringify(event)).join("\n") + "\n");
    writeFileSync(join(logs, "browser-network.jsonl"), networkTrace.map(event => JSON.stringify(event)).join("\n") + "\n");
  }
  rmSync(profile, { recursive: true, force: true });
}
