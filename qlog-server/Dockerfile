FROM rust:1 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
RUN cargo build --release

FROM debian:trixie-slim
COPY --from=build /src/target/release/qlog-server /qlog-server
# qlogs are saved in /data/uploaded-qlogs
WORKDIR /data
ENV RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/qlog-server"]
