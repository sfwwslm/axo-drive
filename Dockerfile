FROM node:20-bullseye-slim AS frontend-builder

WORKDIR /app
COPY frontend/package.json frontend/pnpm-lock.yaml ./frontend/
RUN corepack enable && corepack prepare pnpm@10.24.0 --activate
RUN cd frontend && pnpm install --frozen-lockfile
COPY frontend ./frontend
RUN cd frontend && pnpm build

FROM rust:1.92-bullseye AS rust-builder

WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
  pkg-config \
  libssl-dev \
  ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY frontend ./frontend
COPY --from=frontend-builder /app/frontend/dist ./frontend/dist
RUN cargo build --release

FROM debian:bullseye-slim

WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
  ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY --from=rust-builder /app/target/release/axo-drive /app/axo-drive
RUN mkdir -p /app/.axo

ENV AXO_BIND=0.0.0.0
ENV AXO_HTTP_PORT=5005
ENV AXO_HTTPS_PORT=5006
ENV HOME=/app

EXPOSE 5005 5006
ENTRYPOINT ["/app/axo-drive"]
