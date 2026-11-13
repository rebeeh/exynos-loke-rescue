# Multi-stage minimal OCI image for exynos-loke-rescue
# Stage 1: Build static musl binary
FROM rust:1.80-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /usr/src/exynos-loke-rescue
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ ./src/
COPY udev/ ./udev/
COPY tests/ ./tests/

RUN cargo build --release

# Stage 2: Minimal runtime image
FROM alpine:3.20

RUN apk add --no-cache eudev

COPY --from=builder /usr/src/exynos-loke-rescue/target/release/exynos-loke-rescue /usr/local/bin/exynos-loke-rescue
COPY --from=builder /usr/src/exynos-loke-rescue/udev/60-exynos-loke-rescue.rules /etc/udev/rules.d/60-exynos-loke-rescue.rules

LABEL org.opencontainers.image.title="exynos-loke-rescue"
LABEL org.opencontainers.image.description="Deterministic Linux firmware recovery engine for Samsung Exynos SoCs"
LABEL org.opencontainers.image.url="https://github.com/rebeeh/exynos-loke-rescue"
LABEL org.opencontainers.image.licenses="MIT"

ENTRYPOINT ["/usr/local/bin/exynos-loke-rescue"]
CMD ["--help"]
