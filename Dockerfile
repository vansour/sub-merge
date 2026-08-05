# sub-merge 多阶段 Dockerfile
#
# 阶段 1（web-builder）：独立构建 dioxus 前端 WASM。
#   crates/server/web 是独立 crate（不在 workspace members 内），不能参与
#   cargo build --workspace，只能通过 dx build --web 单独构建。
# 阶段 2（server-builder）：编译 axum 后端（release）。
# 阶段 3（runtime）：debian trixie-slim + 编译产物 + 前端 dist，运行 server。
#
# 注意：cargo install dioxus-cli --version 0.8.0-alpha.1 需要从 crates.io
# 拉取并编译 dioxus-cli（含大量依赖），首次构建较慢（数分钟到十几分钟），
# 但保证确定性：失败即构建失败，不会静默产出空 dist。
#
# 构建：
#   docker build -t sub-merge .

# ---- 阶段 1: 构建前端 WASM ----
FROM rust:1.97 AS web-builder
RUN rustup target add wasm32-unknown-unknown
# dx 0.8.0-alpha.1 与 dioxus 0.8.0-alpha.1 配套（已实测可 dx build --web）。
# 固定精确版本，不用 || true 掩盖失败。
RUN cargo install dioxus-cli --version 0.8.0-alpha.1
WORKDIR /app/crates/server/web
COPY crates/server/web /app/crates/server/web
# 独立 crate：本 crate 有空的 [workspace] 表 opt-out，无需根 Cargo.toml。
# 注意：crates/server/web/dist 是指向 target/dx/... 的 symlink，且被 .dockerignore
# 排除（本地开发才由 dx 生成）。容器内 dx build 输出到真实目录
# target/dx/submerge-web/debug/web/public。把它拷贝到规范的 /out/web/dist，
# 供 runtime 阶段 COPY（不能直接用 symlink 路径）。
RUN dx build --web \
    && mkdir -p /out/web \
    && cp -r target/dx/submerge-web/debug/web/public /out/web/dist

# ---- 阶段 2: 构建 Rust 服务端 ----
FROM rust:1.97 AS server-builder
WORKDIR /app
COPY Cargo.toml /app/Cargo.toml
COPY Cargo.lock /app/Cargo.lock
COPY crates/proxy-core /app/crates/proxy-core
COPY crates/server /app/crates/server
# 注意：--mount=type=cache 的挂载目录不进入本阶段 filesystem layer，
# 无法被后续 COPY --from 取到。故把编译产物从 cache 拷贝到普通路径 /out，
# 再由 runtime 阶段 COPY。
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p server \
    && mkdir -p /out && cp /app/target/release/server /out/server

# ---- 阶段 3: 运行时 ----
# 必须与 rust:1.97 的 glibc 匹配（rust:1.97 基于 Debian 13/trixie，glibc 2.41）。
# bookworm-slim 只有 glibc 2.36，运行编译产物会报 GLIBC_2.38 not found。
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server-builder /out/server /app/server
COPY --from=web-builder /out/web/dist /app/web/dist
ENV WEB_DIST=/app/web/dist \
    DATABASE_PATH=/app/data/submerge.db \
    PORT=8080
VOLUME ["/app/data"]
EXPOSE 8080
CMD ["/app/server"]
