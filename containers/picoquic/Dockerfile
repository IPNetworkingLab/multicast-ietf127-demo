# NOTE: the original dockerfile was based on Anthony Doeraene's work and modified by a human to fit this context
# it was modified by an LLM to compile for arm64 and x86 much faster, it was all thoroughly reviewed by a human

# the builder always runs natively on the build host and cross-compiles for TARGETARCH,
# so only the small runtime stage below runs under QEMU when building a foreign arch
FROM --platform=$BUILDPLATFORM debian:bookworm-slim AS builder
ARG TARGETARCH

RUN apt update && apt install -y --no-install-recommends \
    curl git gcc g++ cmake clang make ca-certificates pkg-config libssl-dev \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross \
    gcc-x86-64-linux-gnu g++-x86-64-linux-gnu libc6-dev-amd64-cross \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu

# cross toolchains used by cargo and by the cc/cmake crates (BoringSSL)
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar \
    BINDGEN_EXTRA_CLANG_ARGS_aarch64_unknown_linux_gnu="--sysroot=/usr/aarch64-linux-gnu" \
    CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \
    BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_linux_gnu="--sysroot=/usr/x86_64-linux-gnu"

# map the docker arch name to the rust target triple
RUN case "$TARGETARCH" in \
    amd64) echo x86_64-unknown-linux-gnu ;; \
    arm64) echo aarch64-unknown-linux-gnu ;; \
    *) echo "unsupported arch: $TARGETARCH" >&2; exit 1 ;; \
    esac > /rust-target

# copy all code dirs but without the target dir
COPY --exclude=*target* ./fcquic-beacon /fcquic-beacon
# multicast-quic repository is outside the build context, so it's passed with --build-context by the build_images script
COPY --from=multicast-quic --exclude=*target* . /multicast-quic

# TODO: add NORM and gpac dirs here

# compile everything, binaries are collected in /out
RUN mkdir -p /out

RUN cd fcquic-beacon && CARGO_NET_GIT_FETCH_WITH_CLI=true cargo build --release --target $(cat /rust-target) \
    && cp target/$(cat /rust-target)/release/source \
    target/$(cat /rust-target)/release/receiver /out/
COPY --from=picoquic_flexicast ./picoquic_sf_fc /out/

# runtime section, only keep the binaries and certs (this stage is the target arch)
FROM debian:bookworm-slim

RUN apt update && apt install -y --no-install-recommends \
    iproute2 curl \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /shared

COPY --from=builder /out/source  /
COPY --from=builder /out/receiver  /
COPY --from=builder /fcquic-beacon/cert.crt /
COPY --from=builder /fcquic-beacon/cert.key /

# relay binaries
COPY --from=builder /out/picoquic_sf_fc  /

COPY ./containers/picoquic/entrypoint.sh /
RUN chmod +x /entrypoint.sh

ENTRYPOINT [ "/entrypoint.sh" ]
