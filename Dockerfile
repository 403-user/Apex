FROM rust:1.85-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libfontconfig-dev libwayland-dev libxkbcommon-dev \
    libegl1-mesa-dev libglib2.0-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libfontconfig1 libegl1 libgl1-mesa-glx && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/apex-terminal /usr/local/bin/apex

ENTRYPOINT ["apex"]
