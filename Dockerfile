# hancic 多阶段镜像
#   builder: rust:1.88.0-alpine（musl 静态编译，默认 crates.io 直连）
#   runner : alpine:3.20（ca-certificates + 非 root hancic）
#
# 构建说明：
#   - 项目 .cargo/config.toml（rsproxy 镜像源）被 .dockerignore 排除，不进入构建
#     上下文，镜像内默认 crates.io；本机/CI 网络受限时可临时换镜像源（覆盖全部
#     构建阶段，含依赖层——上海主机直连 crates.io 的 sparse index 极慢会卡死）：
#       docker build --build-arg CARGO_SOURCE_INDEX="sparse+https://rsproxy.cn/index/"
#   - rust-toolchain.toml 固定 1.88.0，与基础镜像 tag 精确匹配，rustup 不会额外下载。
#   - 运行产物：/app/hancic（二进制）+ /app/assets（后台前端与 vendor，含本地化的
#     vditor i18n/lute/icons）+ /app/themes/default（内置主题种子，entrypoint 首启拷入
#     数据卷）+ /app/config.toml（config.example.toml 默认配置，data_dir=/data）。
FROM rust:1.88.0-alpine AS builder
RUN apk add --no-cache musl-dev gcc
WORKDIR /build
# 可选镜像源覆盖：默认空 = crates.io 直连（CI 在 GitHub 上使用默认源）。
# 必须置于依赖层之前：依赖下载与 sparse index 更新同样走镜像源。
ARG CARGO_SOURCE_INDEX=
RUN if [ -n "$CARGO_SOURCE_INDEX" ]; then \
      mkdir -p /build/.cargo && \
      printf '[source.crates-io]\nreplace-with = "mirror"\n[source.mirror]\nregistry = "%s"\n' "$CARGO_SOURCE_INDEX" > /build/.cargo/config.toml; \
    fi
# 依赖层：仅清单 + 虚拟源码，依赖编译结果保留在镜像层。
# Cargo.toml/Cargo.lock 未变时该层命中 Docker 层缓存 / CI 的 gha 层缓存（秒过）；
# 源码变化不会触发依赖重编。cargo 默认按 CPU 核数多线程并行（-j = 可用核数）。
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release --locked
RUN rm -rf src
COPY . .
# 关键：touch 源码强制重编本项目 crate。GitHub checkout 复制的文件 mtime 早于
# 依赖层 echo 写入虚拟 src 的时间，cargo 的 mtime 指纹会误判源码未变、跳过编译，
# 导致 /build/target/release/hancic 仍是依赖层空 main 的虚拟二进制（528KB，启动即退）。
# touch 后 mtime 最新 → cargo 只重编 hancic crate（依赖指纹未变，仍走增量）。
RUN find src -name '*.rs' -exec touch {} + \
    && cargo build --release --locked

FROM alpine:3.20
RUN apk add --no-cache ca-certificates \
    && adduser -D hancic \
    && mkdir -p /data \
    && chown hancic:hancic /data
WORKDIR /app
COPY --from=builder /build/target/release/hancic /app/hancic
COPY --from=builder /build/assets /app/assets
COPY --from=builder /build/themes /app/themes
COPY config.example.toml /app/config.toml
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh
USER hancic
ENV RUST_LOG=info
VOLUME /data
EXPOSE 8090
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8090/api/health || exit 1
ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["/app/hancic", "/app/config.toml"]
