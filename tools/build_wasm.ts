// Build the standalone Wasm packages. Bun is a host tool, not a runtime dependency.
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const manifest = readFileSync(resolve(root, "Cargo.toml"), "utf8");
const packageVersion = Bun.TOML.parse(manifest).workspace?.package?.version;
if (typeof packageVersion !== "string" || !packageVersion) throw new Error("Cargo.toml must define workspace.package.version");
const version = manifest.match(/^wasm-bindgen = "=([^"]+)"$/m)?.[1];
if (!version) throw new Error("Cargo.toml must pin wasm-bindgen exactly");
const local = resolve(root, ".cache/wasm-tools/bin/wasm-bindgen");
const bindgen = process.env.WASM_BINDGEN ?? (existsSync(local) ? local : "wasm-bindgen");
const mode = process.argv[2] ?? "build";

function run(args: string[]) {
  const result = Bun.spawnSync(args, { cwd: root, stdout: "inherit", stderr: "inherit" });
  if (result.exitCode !== 0) throw new Error(`Command failed (${result.exitCode}): ${args.join(" ")}`);
}

if (mode === "setup") {
  run(["rustup", "target", "add", "wasm32-unknown-unknown"]);
  run(["cargo", "install", "wasm-bindgen-cli", "--version", version, "--locked", "--root", ".cache/wasm-tools"]);
} else if (mode === "build") {
  const check = Bun.spawnSync([bindgen, "--version"], { cwd: root });
  if (check.exitCode !== 0 || check.stdout.toString().trim() !== `wasm-bindgen ${version}`) {
    throw new Error(`Need wasm-bindgen ${version}; run make wasm-setup or set WASM_BINDGEN`);
  }
  const libraries = ["cpu6502", "apple1", "riscv-linux"];
  run(["cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
    ...libraries.flatMap(library => ["-p", `hesper-${library}-wasm`])]);
  for (const library of libraries) {
    const module = `hesper_${library.replaceAll("-", "_")}`;
    for (const target of ["web", "nodejs"]) {
      const out = resolve(root, `target/wasm-packages/${library}/${target}`);
      mkdirSync(out, { recursive: true });
      run([bindgen, `target/wasm32-unknown-unknown/release/${module}_wasm.wasm`,
        "--target", target, "--out-dir", out, "--out-name", module]);
      if (library === "riscv-linux") {
        copyFileSync(resolve(root, "crates/riscv-linux/src/softfloat.LICENSE"), resolve(out, "softfloat.LICENSE"));
      }
      writeFileSync(resolve(out, "README.md"),
        `# hesper-${library} (${target})\n\nVersion: ${packageVersion}. Built with wasm-bindgen ${version}.\n\n` +
        "Usage and interface contract: docs/wasm.md in the Hesper source repository.\n" +
        (library === "apple1" ? "\nWoz Monitor ROM is supplied by the caller and is not included.\n\n" +
          "Includes P-Lab's Apple-1 2513 replacement glyph data, CC BY 4.0.\n" +
          "Source: https://p-l4b.github.io/2513/ (2513_Apple-1.bin).\n" +
          "License: https://creativecommons.org/licenses/by/4.0/\n" +
          "First 512 bytes of four identical banks, reformatted as a Rust 64 x 8 table; pixels unchanged.\n" : ""));
    }
  }
} else {
  throw new Error("Usage: bun tools/build_wasm.ts [build|setup]");
}
