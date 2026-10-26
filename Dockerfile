# syntax=docker/dockerfile:1
# Multi-stage build: compile with the pinned toolchain, run on a slim image.
FROM rust:1.99.1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY templates ./templates
COPY migrations ./migrations
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /app nycmsa
WORKDIR /app
COPY --from=build /src/target/release/nycmsa /usr/local/bin/nycmsa
COPY data ./data
USER nycmsa
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
  CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1
# Inside the container the server must bind all interfaces; Compose maps the
# port to loopback on the host. DATABASE_URL selects PostgreSQL storage.
ENTRYPOINT ["nycmsa"]
CMD ["serve", "--bind", "0.0.0.0:8080"]
