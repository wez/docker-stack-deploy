FROM rust:latest AS rust

WORKDIR /app
COPY . .

RUN apt update && apt install -y musl musl-tools
RUN rustup target add x86_64-unknown-linux-musl
RUN --mount=type=ssh \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --target x86_64-unknown-linux-musl --release && \
    cp  /app/target/x86_64-unknown-linux-musl/release/docker-stack-deploy /app/docker-stack-deploy

FROM alpine:latest

# Install essential dependencies, and remove cache and unnecessary files
RUN apk --no-cache add \
    git \
    curl \
    bash \
    docker-cli \
    docker-compose && \
    rm -rf /var/cache/apk/* /tmp/*

COPY --from=rust /app/docker-stack-deploy /usr/bin/docker-stack-deploy
COPY docker-entrypoint.sh /entrypoint.sh

STOPSIGNAL SIGINT
CMD ["/bin/bash", "/entrypoint.sh"]
