FROM ubuntu:latest 

USER root
ENV USER root
ENV DEBIAN_FRONTEND noninteractive

# Install package dependencies.
RUN apt-get update \
    && apt-get install -y software-properties-common \
    && add-apt-repository ppa:saiarcot895/chromium-dev \
    && apt-get update \
    && apt-get install -y \
    apt-utils \
    bash  \
    curl \
    gcc \
    build-essential \
    libssl-dev \
    ca-certificates \
    xvfb \
    pulseaudio \
    dbus \
    dbus-user-session \
    chromium-browser \
    pkg-config \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-bad1.0-dev \
    gstreamer1.0-plugins-base \
    gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly \
    gstreamer1.0-libav \
    gstreamer1.0-tools \
    gstreamer1.0-x \
    gstreamer1.0-gl \
    gstreamer1.0-pulseaudio \
    && rm -rf /var/lib/apt/lists/*

#Disable chrome first run
RUN mkdir -p /etc/skel/.config/chromium/Default \
    && touch "/etc/skel/.config/chromium/First Run" \
    && echo '{ "browser": { "has_seen_welcome_page": true }}' > /etc/skel/.config/chromium/Default/Preferences

RUN groupadd -g 2000 tapedeck \
&& useradd -m -u 2001 -g tapedeck tapedeck


# Install Rust
RUN curl https://sh.rustup.rs -sSf > /tmp/rustup-init.sh \
    && chmod +x /tmp/rustup-init.sh \
    && sh /tmp/rustup-init.sh -y \
    && rm -rf /tmp/rustup-init.sh

ENV PATH="/root/.cargo/bin:${PATH}"
WORKDIR /usr/src/app

COPY . .

RUN cargo install --path . 

EXPOSE 8000
ENV RUST_LOG debug
ENV ROCKET_ADDRESS 0.0.0.0

RUN cp /root/.cargo/bin/tapedeck /usr/bin
USER tapedeck


CMD ["tapedeck", "record", "https://youtube.com"]

