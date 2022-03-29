FROM ubuntu:latest 

USER root
ENV USER root
ENV DEBIAN_FRONTEND noninteractive

# Install package dependencies.
RUN apt-get update \
    && apt-get install -y \
    apt-utils \
    curl \
    gcc \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-good1.0-dev \
    libgstreamer-plugins-bad1.0-dev \
    build-essential \
    xvfb \
    pulseaudio \
    dbus \
    bash  \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl https://sh.rustup.rs -sSf > /tmp/rustup-init.sh \
    && chmod +x /tmp/rustup-init.sh \
    && sh /tmp/rustup-init.sh -y \
    && rm -rf /tmp/rustup-init.sh

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /usr/src/app

COPY . .

RUN cargo build && \
    cargo install --path . 

CMD ["/usr/local/cargo/bin/tapedeck"]

