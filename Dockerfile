# Build a minimal runtime image for Cursor Bridge.
# The Cursor CLI is NOT baked into image layers: the entrypoint installs it into
# a mounted volume under /opt/cursor-cli (never under HOME).

FROM rust:1.98.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --create-home --uid 10001 --shell /usr/sbin/nologin bridge \
  && mkdir -p /opt/cursor-cli /workspace \
  && chown -R bridge:bridge /opt/cursor-cli /workspace /home/bridge

COPY --from=build /src/target/release/cursor_bridge /usr/local/bin/cursor_bridge
COPY --chmod=0755 bin/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

USER bridge
WORKDIR /home/bridge
ENV HOME=/home/bridge \
    CURSOR_BRIDGE_HOST=0.0.0.0 \
    CURSOR_BRIDGE_PORT=8787 \
    CURSOR_BRIDGE_WORKSPACE=/workspace \
    CURSOR_CLI_HOME=/opt/cursor-cli \
    CURSOR_BRIDGE_AGENT_BIN=/opt/cursor-cli/.local/bin/agent \
    CURSOR_BRIDGE_INSTALL_CLI=1

EXPOSE 8787
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["/usr/local/bin/cursor_bridge"]
