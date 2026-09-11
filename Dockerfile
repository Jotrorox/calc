FROM rust:1.95.0-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends m4 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --no-create-home calc
COPY --from=build /app/target/release/calc-api /usr/local/bin/calc-api
USER 10001:10001
ENV HOST=0.0.0.0 PORT=8080
EXPOSE 8080
ENTRYPOINT ["calc-api"]
