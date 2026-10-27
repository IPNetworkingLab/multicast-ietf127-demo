FROM debian:bookworm-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl git gcc g++ cmake clang make ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# copy all code dirs but without the target dir
COPY --exclude=*target* ./fcquic-beacon /fcquic-beacon
# multicast-quic lives outside the build context, it is passed with --build-context
COPY --from=multicast-quic --exclude=*target* . /multicast-quic
# TODO: add NORM and gpac dirs here

# compile everything
RUN cd multicast-quic && CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --release
RUN cd fcquic-beacon && CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --release

# runtime section, only keep the binaries and certs
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    iproute2 curl \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /shared

COPY --from=builder /fcquic-beacon/target/release/source  /
COPY --from=builder /fcquic-beacon/target/release/receiver  /
COPY --from=builder /fcquic-beacon/cert.crt /
COPY --from=builder /fcquic-beacon/cert.key /

# relay binaries
COPY --from=builder /multicast-quic/target/release/fc-flow-file-transfer  /
COPY --from=builder /multicast-quic/target/release/fc-recv-file-transfer  /

