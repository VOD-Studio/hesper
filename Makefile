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
#   make wasm      CPU / Apple I / RISC-V Linux 库及 JS 绑定的 Wasm 编译检查
#                  （需要已安装 wasm32 目标，不在 verify 中强制）
#   make wasm-setup 安装目标和匹配版本的 wasm-bindgen 到 .cache/wasm-tools
#   make wasm-build 生成三个库的 web / nodejs 包（需要 Bun）
#   make wasm-test  构建包并运行实际 JS 绑定与原生 Rust 对照测试
#   make wasm-browser-test  用独立无头 Chrome 加载 web 产物运行同组检查
#   make wasm-typecheck  用固定版本 TypeScript 检查产物的调用类型（首次联网）
#   make tools-test  用 Bun 运行 tools/ 下 4 个验证脚本自身的回归测试
#                  （不在 verify 中：冷缓存首次运行需要联网真实下载）
#   make riscv-linux-sources  获取并校验参考板的固定源码（显式联网）
#   make riscv-linux-data     构建并校验固定的四份参考板镜像（需要 Linux 构建宿主）
#   make riscv-linux-qemu     从固定源码构建 TCG 参考模拟器
#   make riscv-linux-reference 用已准备的 QEMU 和锁定镜像做实际串口验收（离线）
#   make riscv-linux-blk-data / riscv-linux-blk-reference  构建固定块配置 / QEMU 两次冷启动读写
#   make riscv-linux-firmware  用已验固件交接 S 态探针，验证 SBI TIME 与 UART 中断
#   make riscv-linux-native   验收实际固件、CLI 的 Linux shell、PTY 与管道
#   make riscv-linux-initramfs-native  CLI 的替换根、默认根与数据盘冷启动
#   make riscv-linux-wasm     用已验镜像重放实际 Bun / Node.js Wasm 串口
#   make riscv-linux-alpine-browser  生产 Chrome 的 Alpine 串口、冷启动与拒绝验收
#   make riscv-linux-browser  构建生产站点，用 Chrome 交互验收已验本地镜像
#   make riscv-linux-blk-native  管道与 PTY 两次数据盘冷启动
#   make riscv-linux-blk-browser  生产 Chrome 与 CLI 双向互换数据盘
#   make riscv-linux-initramfs-reference  复建替换根并在固定 QEMU 验证
#   make riscv-linux-initramfs-browser  生产 Chrome 的替换根、默认根与数据盘启动
#   make riscv-linux-alpine-sources / riscv-linux-alpine-data  验签发行版输入 / 复建 Alpine 镜像
#   make riscv-linux-alpine-reference  固定 QEMU 验收 Alpine 客体
#   make riscv-linux-alpine-blk-data / riscv-linux-alpine-blk-reference  复建并验证官方 Alpine 数据盘配置
#   make riscv-linux-fd-reference  对照 QEMU 与 Hesper 的浮点位签名
#   make riscv-linux-alpine-native  原生管道、PTY、冷启动和镜像拒绝验收
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
        tools-test riscv-linux-sources riscv-linux-data riscv-linux-qemu riscv-linux-reference \
        riscv-linux-firmware riscv-linux-native riscv-linux-wasm riscv-linux-browser \
        riscv-linux-blk-data riscv-linux-blk-reference \
        riscv-linux-alpine-sources riscv-linux-alpine-data riscv-linux-alpine-legacy-data riscv-linux-alpine-reference \
        riscv-linux-fd-reference \
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
	cargo check -p hesper-cpu6502 -p hesper-apple1 -p hesper-cpu6502-wasm -p hesper-apple1-wasm \
		-p hesper-riscv-linux -p hesper-riscv-linux-wasm --target wasm32-unknown-unknown --locked

wasm-setup:
	bun tools/build_wasm.ts setup

wasm-build:
	bun tools/build_wasm.ts

wasm-test: wasm-build
	cargo run -p hesper-apple1-wasm --example reference --release --locked > target/wasm-packages/reference.json
	cargo run -p hesper-riscv-linux --example wasm_reference --release --locked > target/wasm-packages/riscv-reference.json
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

# ---- RISC-V Linux 参考板（不属于普通 Cargo 验证） ----

riscv-linux-sources:
	python3 tools/riscv-linux/sources.py

riscv-linux-data: riscv-linux-sources
	python3 tools/riscv-linux/build_guest.py

riscv-linux-blk-data: riscv-linux-sources
	python3 tools/riscv-linux/build_guest.py --profile blk

riscv-linux-qemu: riscv-linux-sources
	python3 tools/riscv-linux/build_qemu.py

riscv-linux-reference:
	python3 tools/riscv-linux/reference.py

riscv-linux-blk-reference:
	python3 tools/riscv-linux/reference.py --profile blk

riscv-linux-alpine-sources:
	python3 tools/riscv-linux/alpine_kernel.py

riscv-linux-alpine-data:
	python3 tools/riscv-linux/build_guest.py --profile alpine

riscv-linux-alpine-legacy-data:
	python3 tools/riscv-linux/build_guest.py --profile alpine-legacy

riscv-linux-alpine-reference:
	python3 tools/riscv-linux/reference.py --profile alpine

.PHONY: riscv-linux-alpine-blk-data riscv-linux-alpine-blk-reference
riscv-linux-alpine-blk-data:
	python3 tools/riscv-linux/build_guest.py --profile alpine-blk

riscv-linux-alpine-blk-reference:
	python3 tools/riscv-linux/reference.py --profile alpine-blk

riscv-linux-fd-reference:
	cargo build --locked --release -p hesper-riscv-linux --example isa_probe
	prefix=$$(PYTHONPATH=tools/riscv-linux python3 -c 'from build_guest import guest_build_dir, guest_compiler; print(str(guest_compiler(guest_build_dir() / "buildroot")).removesuffix("gcc"))') && \
		python3 tools/riscv-linux/isa_reference.py --probe fd \
			--hesper "$(or $(CARGO_TARGET_DIR),target)/release/examples/isa_probe" "$$prefix"

.PHONY: riscv-linux-initramfs-reference
riscv-linux-initramfs-reference:
	python3 tools/riscv-linux/initramfs_reference.py

riscv-linux-firmware:
	python3 tools/riscv-linux/firmware_probe.py

riscv-linux-native: riscv-linux-firmware
	cargo build --locked --release -p hesper
	python3 tools/riscv-linux/native.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper"

.PHONY: riscv-linux-blk-native riscv-linux-alpine-blk-native
riscv-linux-blk-native:
	cargo build --locked --release -p hesper
	python3 tools/riscv-linux/native_disk.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper"

riscv-linux-alpine-blk-native:
	cargo build --locked --release -p hesper
	python3 tools/riscv-linux/native_disk.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper" --alpine

.PHONY: riscv-linux-initramfs-native
riscv-linux-initramfs-native:
	cargo build --locked --release -p hesper
	python3 tools/riscv-linux/native_initramfs.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper" \
		$(if $(RISCV_LINUX_BLK_IMAGES),--blk-images "$(RISCV_LINUX_BLK_IMAGES)")

.PHONY: riscv-linux-alpine-native
riscv-linux-alpine-native:
	cargo build --locked --release -p hesper
	python3 tools/riscv-linux/native_alpine.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper" \
		$(if $(RISCV_LINUX_BLK_IMAGES),--blk-images "$(RISCV_LINUX_BLK_IMAGES)")

riscv-linux-wasm: wasm-build
	python3 tools/riscv-linux/wasm_serial.py

riscv-linux-browser:
	cd examples/web && bun run build
	python3 tools/riscv-linux/browser.py

.PHONY: riscv-linux-blk-browser riscv-linux-alpine-blk-browser
riscv-linux-blk-browser:
	cargo build --locked --release -p hesper
	cd examples/web && bun run build
	python3 tools/riscv-linux/browser_disk.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper"

riscv-linux-alpine-blk-browser:
	cargo build --locked --release -p hesper
	cd examples/web && bun run build
	python3 tools/riscv-linux/browser_disk.py "$(or $(CARGO_TARGET_DIR),target)/release/hesper" --alpine

.PHONY: riscv-linux-initramfs-browser
riscv-linux-initramfs-browser:
	cd examples/web && bun run build
	python3 tools/riscv-linux/browser_initramfs.py \
		$(if $(RISCV_LINUX_BLK_IMAGES),--blk-images "$(RISCV_LINUX_BLK_IMAGES)")

.PHONY: riscv-linux-alpine-browser
riscv-linux-alpine-browser:
	cd examples/web && bun run build
	python3 tools/riscv-linux/browser_alpine.py

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
