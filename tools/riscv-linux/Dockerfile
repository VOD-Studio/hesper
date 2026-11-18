FROM debian:12-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc

RUN apt-get update && apt-get install -y --no-install-recommends \
    bc bison build-essential bzip2 ca-certificates cpio curl device-tree-compiler \
    file flex git gpg gpgv libelf-dev libfdt-dev libglib2.0-dev libpixman-1-dev libssl-dev \
    locales ninja-build openssl patch pkg-config python3-venv rsync unzip wget xz-utils zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

ENV HOME=/tmp LC_ALL=C.UTF-8
