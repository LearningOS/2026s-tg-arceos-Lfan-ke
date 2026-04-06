FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG QEMU_VERSION=10.2.1
ARG RUST_TOOLCHAIN=nightly-2026-02-25

ENV TZ=Etc/UTC \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/opt/qemu-${QEMU_VERSION}/bin:/opt/x86_64-linux-musl-cross/bin:/opt/aarch64-linux-musl-cross/bin:/opt/riscv64-linux-musl-cross/bin:/opt/loongarch64-linux-musl-cross/bin:${PATH}

# Base tools (including libssl-dev for openssl-sys)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        build-essential \
        clang \
        cmake \
        curl \
        git \
        libglib2.0-dev \
        libpixman-1-dev \
        libslirp-dev \
        libudev-dev \
        make \
        ninja-build \
        pkg-config \
        python3 \
        python3-venv \
        wget \
        xz-utils \
        zlib1g-dev \
        libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install seabios
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        seabios \
    && rm -rf /var/lib/apt/lists/*

# Install QEMU
RUN set -eux; \
    curl -L -o /tmp/qemu.tar.xz "https://download.qemu.org/qemu-${QEMU_VERSION}.tar.xz"; \
    tar -xf /tmp/qemu.tar.xz -C /tmp; \
    cd "/tmp/qemu-${QEMU_VERSION}"; \
    ./configure \
      --prefix="/opt/qemu-${QEMU_VERSION}" \
      --target-list=loongarch64-softmmu,riscv64-softmmu,aarch64-softmmu,x86_64-softmmu \
      --enable-slirp \
      --disable-docs \
      --disable-gtk \
      --disable-sdl \
      --disable-vte \
      --disable-werror; \
    make -j"$(nproc)"; \
    make install; \
    rm -rf "/tmp/qemu-${QEMU_VERSION}" /tmp/qemu.tar.xz

# Install musl cross-compilation toolchains
RUN set -eux; \
    for arch in aarch64 riscv64 x86_64 loongarch64; do \
      file="${arch}-linux-musl-cross.tgz"; \
      if ! wget -q "https://github.com/arceos-org/setup-musl/releases/download/prebuilt/${file}" -O "/tmp/${file}"; then \
        if [ "${arch}" = "loongarch64" ]; then \
          wget -q "https://github.com/LoongsonLab/oscomp-toolchains-for-oskernel/releases/download/loongarch64-linux-musl-cross-gcc-13.2.0/${file}" -O "/tmp/${file}"; \
        else \
          wget -q "https://musl.cc/${file}" -O "/tmp/${file}"; \
        fi; \
      fi; \
      tar -xzf "/tmp/${file}" -C /opt; \
      rm -f "/tmp/${file}"; \
    done

# Install Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}"

RUN rustup component add rust-src llvm-tools rustfmt clippy \
    && rustup target add \
      x86_64-unknown-none \
      riscv64gc-unknown-none-elf \
      aarch64-unknown-none-softfloat \
      loongarch64-unknown-none-softfloat \
    && cargo install cargo-binutils axconfig-gen cargo-axplat

# Install SSH server (for Remote-SSH), Zsh, and common utilities
RUN apt-get update && apt-get install -y --no-install-recommends \
        openssh-server \
        zsh \
        lsof \
        nload \
        htop \
        net-tools \
        dnsutils \
        unzip \
    && rm -rf /var/lib/apt/lists/*

# Install Oh My Zsh with plugins and configure .zshrc
RUN sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)" --unattended && \
    git clone https://github.com/zsh-users/zsh-autosuggestions ${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/plugins/zsh-autosuggestions && \
    git clone https://github.com/zsh-users/zsh-syntax-highlighting.git ${ZSH_CUSTOM:-~/.oh-my-zsh/custom}/plugins/zsh-syntax-highlighting && \
    sed -i 's/plugins=(git)/plugins=(git zsh-autosuggestions zsh-syntax-highlighting)/' ~/.zshrc && \
    echo 'setopt NO_AUTO_REMOVE_SLASH' >> ~/.zshrc && \
    mkdir -p ~/.oh-my-zsh/completions && \
    # Optional: generate docker completion (if docker is installed)
    if command -v docker > /dev/null; then \
        docker completion zsh > ~/.oh-my-zsh/completions/_docker; \
    fi && \
    # Prepend 'source /etc/profile' to .zshrc to ensure environment variables are loaded
    sed -i '1i source /etc/profile' ~/.zshrc && \
    chsh -s $(which zsh)

# Optional: uncomment the following lines to customize code-server settings (ensure settings.json exists in build context)
# COPY settings.json /root/.vscode-server/data/Machine/settings.json
# COPY settings.json /root/.local/share/code-server/Machine/settings.json

WORKDIR /workspace

# Use zsh as default shell
CMD ["/bin/zsh"]