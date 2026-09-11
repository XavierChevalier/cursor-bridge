# Build a minimal runtime image for Cursor Bridge.
# The Cursor CLI is NOT baked in: mount or install `agent` at deploy time so
# HOME bind-mounts cannot mask the binary.

FROM rust:1.98.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --create-home --uid 10001 --shell /usr/sbin/nologin bridge

COPY --from=build /src/target/release/cursor_bridge /usr/local/bin/cursor_bridge

USER bridge
WORKDIR /home/bridge
ENV CURSOR_BRIDGE_HOST=0.0.0.0 \
    CURSOR_BRIDGE_PORT=8787 \
    CURSOR_BRIDGE_WORKSPACE=/workspace \
    CURSOR_BRIDGE_AGENT_BIN=/usr/local/bin/agent

EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/cursor_bridge"]
