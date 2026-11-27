# Everything needed to build, lint, and test a port. It stays in this image so
# running the harness doesn't require installing Rust on the host.
FROM python:3.12.14-bookworm

ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH

# Optional: trust extra CAs, for networks that re-sign TLS (a cloud container's
# egress proxy, say). Pass a PEM file of one or more certificates with
# --secret id=extra_ca,src=<file>; without it this step does nothing. They go into the
# system store, which curl, rustup and cargo read, and Claude Code reads the same store.
RUN --mount=type=secret,id=extra_ca \
    if [ -s /run/secrets/extra_ca ]; then \
      awk '/BEGIN CERTIFICATE/{n++} n{print > ("/usr/local/share/ca-certificates/portwright-extra-" n ".crt")}' \
        /run/secrets/extra_ca \
      && update-ca-certificates; \
    fi
ENV NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt

RUN apt-get update && apt-get install -y --no-install-recommends clang lld && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
      --default-toolchain stable -c clippy -c rustfmt -c llvm-tools-preview \
    && rustup toolchain install nightly --profile minimal
RUN pip install --no-cache-dir "maturin>=1.7,<2"
RUN cargo install --locked cargo-nextest cargo-llvm-cov cargo-mutants cargo-deny cargo-fuzz \
    && rm -rf /opt/cargo/registry /opt/cargo/git
RUN chmod -R a+rwX /opt/cargo /opt/rustup
# freezegun: a test dependency of ported libraries (humanize).
RUN pip install --no-cache-dir "pytest>=8" "hypothesis>=6.100" "coverage>=7.5" "freezegun>=1.5"
# Claude Code for agent sandboxes, pinned so runs are repeatable.
# pipefail: a failed download must fail the build, not leave a dangling link.
RUN HOME=/opt/claude bash -o pipefail -c 'mkdir -p $HOME && curl -fsSL https://claude.ai/install.sh | bash -s 2.1.316' \
    && ln -s /opt/claude/.local/bin/claude /usr/local/bin/claude \
    && chmod -R a+rX /opt/claude \
    && claude --version

WORKDIR /work
