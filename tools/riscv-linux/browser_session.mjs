// Use real Chromium, production assets, native file selection and key events.
import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";
import { createInterface } from "node:readline";
import { Readable } from "node:stream";

const [dist, images, logs, mode, ...input] = process.argv.slice(2);
const diskGate = mode === "disk";
const [cliDisk, webDisk, oversized] = input;
const [corrupt] = input;
const lock = JSON.parse(readFileSync(new URL(diskGate ? "images-blk.lock.json" : "images.lock.json", import.meta.url), "utf8"));
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
  const workerEvents = [], downloads = [], downloadProgress = new Map(), dialogs = [];
  const onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.id !== undefined) {
      const complete = requests.get(message.id);
      if (complete) complete(message);
    } else if (message.method === "Browser.downloadWillBegin") {
      downloads.push(message.params);
    } else if (message.method === "Browser.downloadProgress") {
      downloadProgress.set(message.params.guid, message.params);
    } else if (message.method === "Page.javascriptDialogOpening") {
      dialogs.push(message.params);
    } else if (message.method === "Runtime.bindingCalled" && message.params.name === "observeHesper") {
      const event = JSON.parse(message.params.payload);
      if (diskGate) workerEvents.push(event);
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
  const call = (method, params = {}, ms = 12_000, connection = socket) => new Promise((done, fail) => {
    const id = ++sequence;
    const timer = setTimeout(() => { requests.delete(id); fail(new Error(`${method}: deadline`)); }, ms);
    requests.set(id, message => {
      requests.delete(id);
      clearTimeout(timer);
      if (message.error) fail(new Error(`${method}: ${message.error.message}`));
      else done(message.result);
    });
    connection.send(JSON.stringify({ id, method, params }));
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
  const enabled = selector => `(() => {
    const control = document.querySelector(${JSON.stringify(selector)});
    return !!control && !control.disabled && control.checkVisibility();
  })()`;
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
  async function selectFiles(directory) {
    await setFiles("[data-linux-files]", lock.images.map(entry => join(directory, entry.path)));
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
        if (${diskGate} && typeof message?.sessionId === 'number' && message.type !== 'input') {
          this.#observedSessionId = message.sessionId;
          observeHesper(JSON.stringify({request: true, sessionId: message.sessionId, type: message.type,
            files: Object.values(message).flat().filter(value => value instanceof File)
              .map(file => ({name: file.name, size: file.size}))}));
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
  else await runSerialGate();

  async function runDiskGate() {
    const diskInput = "[data-linux-disk]";
    const clear = "[data-linux-clear-disk]";
    const diskError = "[data-linux-disk-error]";
    const exporter = "[data-linux-export]";
    const stop = "[data-linux-stop]";
    const diskFiles = `document.querySelector('${diskInput}').files`;
    const downloadName = "hesper-data.raw";
    const exportLabels = ["first", "after-resume"];
    const completed = [];
    const sendRequest = event => event.request && event.sessionId === streaming;
    const pausedSnapshots = [];
    let paused;
    let race;

    async function waitFor(predicate, name) {
      const started = performance.now();
      while (performance.now() - started < 8000) {
        const result = predicate();
        if (result) return result;
        await delay(12);
      }
      throw new Error(`Chrome did not report ${name}`);
    }

    async function selectDisk(path) {
      await setFiles(diskInput, [path]);
      await until(`${diskFiles}.length === 1 && ${diskFiles}[0].size === ${statSync(path).size}
        && ${enabled(clear)} && !${visible(diskError)}`, "accepted disk file");
    }

    async function boot(path, phase) {
      await selectDisk(path);
      streaming = 1 + (workerEvents.findLast(event => event.request && event.type === "boot")?.sessionId ?? 0);
      const bytes = await evaluate("Array.from(document.querySelector('[data-linux-files]').files, file => file.size).reduce((a,b) => a+b, 0)");
      emit({ type: "start", phase, browser: version.Browser, imageBytes: bytes,
        url: `http://127.0.0.1:${server.port}${tab}` });
      await click("[data-linux-start]");
      const boot = await waitFor(() => workerEvents.find(event => sendRequest(event) && event.type === "boot"), `boot for ${phase}`);
      check(boot.files.some(file => file.name === path.split(sep).at(-1) && file.size === statSync(path).size),
        "Worker boot did not use the selected local disk");
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
    for await (const line of createInterface({ input: process.stdin })) {
      const request = JSON.parse(line);
      if (request.type === "keys") {
        if (paused) await resumeWithKeys(request.text);
        else await type(request.text);
      } else if (request.type === "write-phase") {
        check(race === undefined, "repeated write phase");
        await raceStop();
        await boot(webDisk, "web-to-cli");
      } else if (request.type === "export") {
        await exportDisk(request.label);
      } else if (request.type === "finish") {
        check(completed.length === exportLabels.length && race, "missing completed disk exports");
        const declined = await rejectOrDiscard(false);
        const accepted = await rejectOrDiscard(true);
        await delay(350);
        check(downloads.length === completed.length + race.downloads, "unexpected download during or after dirty Stop");
        check(replies.every(reply => reply.type !== "error"), "Worker reported an error during disk acceptance");
        check(guestDownloadedBytes === 0, "the site fetched guest images instead of reading local Files");
        const screenshot = await call("Page.captureScreenshot", { format: "png" });
        writeFileSync(join(logs, "browser-terminal.png"), Buffer.from(screenshot.data, "base64"));
        emit({ type: "done", race, oversizedError, declined, accepted, pausedSnapshots, guestDownloadedBytes });
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
  }
} catch (error) {
  emit({ type: "error", message: String(error) });
  console.error(error);
  process.exitCode = 1;
} finally {
  process.stdin.destroy();
  socket?.close();
  browserSocket?.close();
  await server.stop(true);
  child.kill();
  await child.exited;
  writeFileSync(join(logs, "browser-chromium.stderr.log"), chromeError);
  rmSync(profile, { recursive: true, force: true });
}
