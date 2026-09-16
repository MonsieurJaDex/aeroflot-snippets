FROM rust:1.97.1-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY backend/ ./

RUN cargo build --release


FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    libpq5 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /app/assets

COPY --from=builder /app/target/release/aeroflot-snippets /app/aeroflot-snippets
COPY et.tmj /app/assets/map2.json

EXPOSE 3001

CMD ["/app/aeroflot-snippets", "-p", "/app/assets/map2.json"]
