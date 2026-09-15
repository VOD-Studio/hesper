# Hesper Makefile
#
# 本地验证（对应 AGENTS.md「必须执行」与 README「验证」）：
#   make           等价于 make verify
#   make verify    格式、全目标检查、debug/release 测试、Clippy、CLI 演示、git 空白检查
#   make fmt / check / test / test-release / clippy / demo / diff
#                 verify 的单个步骤，可单独运行
#   make fmt-write 直接运行 cargo fmt --all 改写代码（fmt 只检查不修改）
#   make fix       运行 cargo fmt --all 后，再对 workspace 全部 crate（含全部 target）
#                  cargo fix --allow-dirty 自动修复
#   make build     cargo build --workspace（debug）或 --release
#   make wasm      CPU / Apple I 库及 JS 绑定的 Wasm 编译检查
#                  （需要已安装 wasm32 目标，不在 verify 中强制）
#   make wasm-setup 安装目标和匹配版本的 wasm-bindgen 到 .cache/wasm-tools
#   make wasm-build 生成两个库的 web / nodejs 包（需要 Bun）
#   make wasm-test  构建包并运行实际 JS 绑定与原生 Rust 对照测试
#   make wasm-browser-test  用独立无头 Chrome 加载 web 产物运行同组检查
#   make wasm-typecheck  用固定版本 TypeScript 检查产物的调用类型（首次联网）
#   make tools-test  用 Bun 运行 tools/ 下 4 个验证脚本自身的回归测试
#                  （不在 verify 中：冷缓存首次运行需要联网真实下载）
#
# 全量外部一致性（对应 .github/workflows/full-cpu.yml，需 Bun）：
#   make data      下载并校验固定版本官方数据、Klaus 镜像与 revD 模型到忽略缓存
#   make full      重现全部外部验证：151 万 SingleStep、Klaus、decimal、中断、revD 对照
#   make singlestep / functional / decimal / interrupt / visual6502 / pins
#                 full 的单个步骤
#
# full 显式联网且耗时，普通回归不使用；CI 中相关命令另加 --locked --offline。

.NOTPARALLEL:

.PHONY: verify fmt fix check test test-release clippy demo diff build build-release \
        wasm wasm-setup wasm-build wasm-test wasm-browser-test wasm-typecheck \
        tools-test \
        data singlestep functional decimal interrupt visual6502 pins full

.DEFAULT_GOAL := verify

verify: fmt check test test-release clippy demo diff
	@echo "verify: 本地检查全部通过"

fmt:
	cargo fmt --all -- --check

fix:
	cargo fmt --all
	cargo fix --workspace --all-targets --allow-dirty

check:
	cargo check --workspace --all-targets

test:
	cargo test --workspace

test-release:
	cargo test --workspace --release

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

demo:
	cargo run -p hesper -- demo
	cargo run -p hesper -- demo --trace
	cargo run -p hesper -- demo --bus-trace --trace-limit 4

diff:
	git diff --check
	git diff --cached --check

build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

wasm:
	cargo check -p hesper-cpu6502 -p hesper-apple1 -p hesper-cpu6502-wasm -p hesper-apple1-wasm --target wasm32-unknown-unknown --locked

wasm-setup:
	bun tools/build_wasm.ts setup

wasm-build:
	bun tools/build_wasm.ts

wasm-test: wasm-build
	cargo run -p hesper-apple1-wasm --example reference --release --locked > target/wasm-packages/reference.json
	bun test tests/wasm/
	node --test tests/wasm/bindings.test.mjs

wasm-typecheck: wasm-build
	bunx --package typescript@7.0.2 tsc --noEmit --strict --target ES2022 --module ESNext --moduleResolution bundler --lib ESNext,DOM tests/wasm/types.ts

wasm-browser-test: wasm-test wasm-typecheck
	bun tools/test_wasm_browser.ts

# ---- 验证脚本自身的回归（Bun；不属于 cargo test --workspace） ----
# 冷缓存时真实下载固定上游数据，因此不加入 verify。
tools-test:
	bun test tools/

# ---- 全量外部一致性（显式准备数据后运行） ----

data:
	bun tools/prepare_singlestep.ts --full
	bun tools/prepare_klaus.ts
	bun tools/prepare_visual6502.ts

singlestep:
	cargo run -p hesper-cpu6502 --example singlestep --release -- --full

functional:
	cargo run -p hesper-cpu6502 --example functional --release

decimal:
	cargo run -p hesper-cpu6502 --example functional --release -- --decimal

interrupt:
	cargo run -p hesper-cpu6502 --example interrupt --release -- --feedback-delay 4

visual6502:
	bun tools/verify_visual6502.ts

pins:
	cargo test -p hesper-cpu6502 --test pins --release

full: data singlestep functional decimal interrupt visual6502 pins
	@echo "full: 全量外部一致性检查通过"
