# sub-merge 构建编排
# dx 由 cargo install 安装，位于 $(HOME)/.cargo/bin，加入 PATH。
PATH := $(HOME)/.cargo/bin:$(PATH)

.PHONY: build-web build-server build run docker smoke clean

# 前端 WASM：cd 到 web crate 单独构建。
# web 是独立 crate（不在 workspace members 内），只能由 dx build 构建，
# 不能走 cargo build --workspace。
build-web:
	cd crates/server/web && dx build --web

# 后端二进制（release）。
build-server:
	cargo build --release -p server

# 完整构建：前端 + 后端。
build: build-web build-server

# 本地运行：先构建前端，再用 WEB_DIST 指向 dist 启动 server。
# DATABASE_PATH 默认 ./submerge.db（首次运行自动建库并生成 token）。
run: build-web
	WEB_DIST=./crates/server/web/dist cargo run -p server

# Docker 镜像（多阶段：容器内 dx build + cargo build --release）。
docker:
	docker build -t sub-merge .

# 端到端冒烟测试：构建前端 → 起 server（临时 DB）→ curl 验证 SPA/静态/API。
smoke:
	bash scripts/smoke.sh

clean:
	rm -rf crates/server/web/target
	cargo clean
