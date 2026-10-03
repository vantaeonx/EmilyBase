FROM rust:1.99.0-slim-bookworm@sha256:452176c0cefca88c0b3184ce85a4eb03e3d4fa05d2afb5366abcba853221019e AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p emilybase-server -p emilybase-cli

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/emilybase && chown 10001:10001 /var/lib/emilybase \
    && chmod 0700 /var/lib/emilybase
COPY --from=build /build/target/release/emilybase-server /usr/local/bin/emilybase-server
COPY --from=build /build/target/release/emilybase /usr/local/bin/emilybase
USER 10001:10001
WORKDIR /var/lib/emilybase
ENV EMILYBASE_DATA_DIR=/var/lib/emilybase/projects EMILYBASE_LISTEN=0.0.0.0:7000
EXPOSE 7000
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:7000/health || exit 1
CMD ["emilybase-server"]
