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
#   make wasm      仅 CPU 库 Wasm 编译检查；需要已安装 wasm32-unknown-unknown 目标
#                  （未安装时跳过，不在 verify 中强制）
#
# 全量外部一致性（对应 .github/workflows/full-cpu.yml，需 Python 3.12+ 与 Node）：
#   make data      下载并校验固定版本官方数据、Klaus 镜像与 revD 模型到忽略缓存
#   make full      重现全部外部验证：151 万 SingleStep、Klaus、decimal、中断、revD 对照
#   make singlestep / functional / decimal / interrupt / visual6502 / pins
#                 full 的单个步骤
#
# full 显式联网且耗时，普通回归不使用；CI 中相关命令另加 --locked --offline。

.NOTPARALLEL:

.PHONY: verify fmt fix check test test-release clippy demo diff wasm \
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

wasm:
	cargo check -p hesper-cpu6502 --target wasm32-unknown-unknown

# ---- 全量外部一致性（显式准备数据后运行） ----

data:
	python3 tools/prepare_singlestep.py --full
	python3 tools/prepare_klaus.py
	python3 tools/prepare_visual6502.py

singlestep:
	cargo run -p hesper-cpu6502 --example singlestep --release -- --full

functional:
	cargo run -p hesper-cpu6502 --example functional --release

decimal:
	cargo run -p hesper-cpu6502 --example functional --release -- --decimal

interrupt:
	cargo run -p hesper-cpu6502 --example interrupt --release -- --feedback-delay 4

visual6502:
	node tools/verify_visual6502.cjs

pins:
	cargo test -p hesper-cpu6502 --test pins --release

full: data singlestep functional decimal interrupt visual6502 pins
	@echo "full: 全量外部一致性检查通过"