// Real Chromium loading of the shipped web ES modules, served only on loopback.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const chrome = process.env.CHROME_BIN ?? (process.platform === "darwin"
  ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" : "google-chrome");
const profile = mkdtempSync(resolve(tmpdir(), "hesper-wasm-chrome-"));
const server = Bun.serve({
  hostname: "127.0.0.1", port: 0,
  async fetch(request) {
    const pathname = new URL(request.url).pathname;
    if (!pathname.startsWith("/tests/wasm/") && !pathname.startsWith("/target/wasm-packages/")) {
      return new Response("Not found", { status: 404 });
    }
    const file = Bun.file(resolve(root, `.${pathname}`));
    if (!await file.exists()) return new Response("Not found", { status: 404 });
    return new Response(file);
  },
});

try {
  const child = Bun.spawn([chrome, "--headless", "--disable-gpu", "--no-first-run", "--no-default-browser-check",
    `--user-data-dir=${profile}`, "--dump-dom", "--virtual-time-budget=10000",
    `http://127.0.0.1:${server.port}/tests/wasm/browser.html`], { stdout: "pipe", stderr: "pipe" });
  const timeout = setTimeout(() => child.kill(), 45000);
  let stdout: string, stderr: string, status: number;
  try {
    [stdout, stderr, status] = await Promise.all([
      new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
    ]);
  } finally { clearTimeout(timeout); }
  if (status !== 0 || !/<html[^>]*data-result="pass"/.test(stdout)) {
    throw new Error(`Browser validation failed (${status})\n${stdout.slice(0, 6000)}\n${stderr.slice(-2000)}`);
  }
  console.log(stdout.match(/<pre[^>]*>([\s\S]*?)<\/pre>/)?.[1] ?? "Browser validation passed");
} finally {
  await server.stop(true);
  rmSync(profile, { recursive: true, force: true });
}
