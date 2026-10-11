FROM mcr.microsoft.com/devcontainers/base:2-bookworm

# Tauri v2 build dependencies, also needed to load the NAPI test addon.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    file \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libxdo-dev \
    patchelf \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*
