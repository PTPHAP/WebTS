FROM node:22.20.0-bookworm-slim AS web
WORKDIR /build/web
COPY web/package*.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
RUN npm run build
RUN mkdir -p /licenses && for name in react react-dom scheduler; do mkdir -p /licenses/npm-$name; cp node_modules/$name/LICENSE /licenses/npm-$name/; done

FROM rust:1.99.0-bookworm AS gateway
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY server/ server/
COPY vendor/ vendor/
COPY patches/ patches/
COPY config.example.toml ./
COPY LICENSE THIRD_PARTY_NOTICES.md ./
COPY licenses/ licenses/
COPY scripts/collect-licenses.sh scripts/collect-licenses.sh
RUN git -C vendor/tsclientlib apply --reverse --check ../../patches/raw-audio.patch 2>/dev/null || git -C vendor/tsclientlib apply ../../patches/raw-audio.patch
RUN cargo build --release --package web-ts --locked
RUN sh scripts/collect-licenses.sh /licenses

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home webts
WORKDIR /app
COPY --from=gateway /build/target/release/web-ts /usr/local/bin/web-ts
COPY --from=web /build/web/dist ./web/dist
COPY --from=gateway /licenses /usr/share/doc/webts/licenses
COPY --from=web /licenses /usr/share/doc/webts/licenses
COPY scripts/container-entrypoint.sh /usr/local/bin/entrypoint
RUN chmod 755 /usr/local/bin/entrypoint
USER 10001:10001
EXPOSE 8080/tcp 40000-40100/udp
ENTRYPOINT ["/usr/local/bin/entrypoint"]
CMD ["serve", "/app/config.local.toml"]
