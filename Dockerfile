# Stage 1: Build
ARG RUST_VERSION=1.76.0
ARG PACKAGE=monzo-ingestion
FROM lukemathwalker/cargo-chef:latest-rust-${RUST_VERSION} as chef
WORKDIR /build/
# hadolint ignore=DL3008
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    lld \
    clang \
    libclang-dev \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

FROM chef as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef as builder
COPY --from=planner /build/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release -p "$PACKAGE" --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release -p "$PACKAGE"

# Stage 2: Run
FROM debian:bookworm-slim AS runtime

RUN set -ex; \
    apt-get update && \
    apt-get -y install --no-install-recommends \
        ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

# Create a non-privileged user that the app will run under.
# See https://docs.docker.com/develop/develop-images/dockerfile_best-practices/#user
ARG UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser
USER appuser

# Copy the executable from the "build" stage.
COPY --from=builder /build/target/release/"$PACKAGE" /bin/server

# Expose the port that the application listens on.
EXPOSE 3000

ADD "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64" /bin/yt-dlp
ENV RECIPE_YT_DLP_PATH=/bin/yt-dlp
ENV RECIPE_REEL_DIR=/data/reels

HEALTHCHECK --interval=5s --timeout=3s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# What the container should run when it is started.
CMD ["/bin/server", "serve", "--address", "0.0.0.0:3000"]
