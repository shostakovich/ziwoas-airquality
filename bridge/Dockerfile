# syntax=docker/dockerfile:1
# Builds a static musl binary for the build platform (x86_64 on the server, arm64 on Apple Silicon).
FROM rust:1.97-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src

# Dependency layer: rebuilt only when Cargo.toml/Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/deps/sen66_bridge* target/release/deps/libsen66_bridge* \
       target/release/sen66-bridge* target/release/.fingerprint/sen66-bridge-*

COPY src ./src
RUN cargo build --release --locked \
    && cp target/release/sen66-bridge /sen66-bridge

FROM scratch
COPY --from=build /sen66-bridge /sen66-bridge
# Runs as root on purpose: a device passed in via `devices:` keeps the host's owner and mode
# (usually root:dialout 0660), and dialout's GID differs between distros. The image contains
# nothing but this binary. To run unprivileged instead, add `user: "65534"` and
# `group_add: ["<host dialout gid>"]` in compose.yml.
ENV SERIAL_PORT=/dev/ttySEN66
ENTRYPOINT ["/sen66-bridge"]
