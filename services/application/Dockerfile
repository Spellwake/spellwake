# syntax=docker/dockerfile:1
FROM rust:1.99.1-bookworm@sha256:4715c3ad743951a3cea90e8b432f2a17e0ef5f3f3c3abf75108879e783b396f0 AS build
WORKDIR /source
COPY Cargo.toml rust-toolchain.toml ./
COPY crates/contracts ./crates/contracts
COPY crates/engine ./crates/engine
COPY crates/oracle_compiler ./crates/oracle_compiler
COPY crates/card_programs ./crates/card_programs
COPY crates/content ./crates/content
COPY services/application/Cargo.toml services/application/Cargo.lock services/application/build.rs ./services/application/
COPY services/application/build_support ./services/application/build_support
COPY services/application/src ./services/application/src
ENV CARGO_TARGET_DIR=/target \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=64 \
    CARGO_PROFILE_RELEASE_DEBUG=0
RUN --mount=type=cache,id=spellwake-cargo-reg,target=/usr/local/cargo/registry \
    --mount=type=cache,id=spellwake-cargo-target,target=/target \
    cargo build --locked --release --manifest-path services/application/Cargo.toml \
      --bin application_api \
      --config 'profile.release.opt-level=2' \
      --config 'profile.release.package."spellwake-card-programs".opt-level=0' \
  && install -d /release \
  && install -m 755 /target/release/application_api /release/ \
  && install -m 644 "$(ls -t /target/release/build/spellwake-application-*/out/native-runtime-inputs.json | head -n 1)" /release/native-runtime-inputs.json \
  && (cd /release && sha256sum application_api > SHA256SUMS)

FROM registry.fly.io/spellwake-api@sha256:82454d59587cd41fda8af998c264a6c04eabaa2e259bd34ff35827016f47356b AS runtime-assets

FROM debian:bookworm-slim@sha256:c67e92c8099ba22853ab4cfda7cfb494d253d6012ca82dee98ba8976772af018
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
COPY --from=build /release /opt/spellwake
COPY --from=runtime-assets --chown=65532:65532 /opt/spellwake/runtime /opt/spellwake/runtime
LABEL org.opencontainers.image.source="https://github.com/Spellwake/spellwake"
USER 65532:65532
EXPOSE 8080
CMD ["/opt/spellwake/application_api"]
