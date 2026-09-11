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
#   make wasm      仅 CPU 库 Wasm 编译检查；需要已安装 wasm32-unknown-unknown 目标
#                  （未安装时跳过，不在 verify 中强制）
#   make wozmon [ROM=<path>]      用 Bun 下载并校验 ROM（默认 .cache/apple1/wozmon.bin）
#   make wozmon-verify ROM=<path>  校验用户自备 Woz Monitor ROM 的大小与哈希
#   make wozmon-tests ROM=<path>   显式运行需要该 ROM 的 --ignored 集成测试
#                  （ROM 不内嵌或提交；下载需显式请求，见 crates/apple1/tests/data/README.md）
#   make tools-test  用 Bun 运行 tools/ 下 5 个验证脚本自身的回归测试
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

.PHONY: verify fmt fix check test test-release clippy demo diff build build-release wasm \
        tools-test wozmon wozmon-verify wozmon-tests \
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
	cargo run -p hesper
	cargo run -p hesper -- --trace
	cargo run -p hesper -- --bus-trace --trace-limit 4

diff:
	git diff --check
	git diff --cached --check

build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

wasm:
	cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown

# ---- 验证脚本自身的回归（Bun；不属于 cargo test --workspace） ----
# 冷缓存时真实下载固定上游数据，因此不加入 verify。
tools-test:
	bun test tools/

# ---- Woz Monitor ROM (explicit download only; never committed) ----
ROM ?= .cache/apple1/wozmon.bin
# Cargo runs tests from each crate directory; pass an absolute ROM path.
ROM_PATH = $(if $(filter /%,$(ROM)),$(ROM),$(CURDIR)/$(ROM))

wozmon:
	bun tools/prepare_wozmon.ts "$(ROM_PATH)"

# make wozmon-verify ROM=/path/to/wozmon.bin
wozmon-verify:
	bun tools/prepare_wozmon.ts --verify "$(ROM_PATH)"

# make wozmon-tests ROM=/path/to/wozmon.bin
wozmon-tests:
	HESPER_APPLE1_ROM="$(ROM_PATH)" cargo test -p hesper-apple1 --test wozmon -- --ignored
	HESPER_APPLE1_ROM="$(ROM_PATH)" cargo test -p hesper --test apple1 -- --ignored

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