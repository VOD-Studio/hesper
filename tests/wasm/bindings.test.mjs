import { test } from "node:test";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { cases } from "./checks.mjs";

const require = createRequire(import.meta.url);
const { Cpu6502Ram } = require("../../target/wasm-packages/cpu6502/nodejs/hesper_cpu6502.js");
const { Apple1 } = require("../../target/wasm-packages/apple1/nodejs/hesper_apple1.js");
const reference = JSON.parse(readFileSync(new URL("../../target/wasm-packages/reference.json", import.meta.url), "utf8"));

for (const [name, run] of cases(Cpu6502Ram, Apple1, reference)) test(name, run);
