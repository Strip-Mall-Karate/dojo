# Sourced by build.sh and test.sh inside the builder VM: the build environment
# for the tree (the Rust its rust-toolchain names, OpenSSL 1.0.2) and run(),
# which runs a command in it.
base=$HOME/dojo-build
# nixos-26.05, the commit support/vm/default.nix pins.
np=github:NixOS/nixpkgs/1f607aa79b6356ed45d3cd3c8079c196e79efa2c
export NIX_CONFIG="extra-experimental-features = nix-command flakes"
export NIXPKGS_ALLOW_INSECURE=1 GIT_CONFIG_GLOBAL=$base/gitconfig RUSTUP_HOME=$base/rustup CARGO_HOME=$base/cargo
mkdir -p "$base"
: >"$GIT_CONFIG_GLOBAL"
pkgs=("${np}#rustup" "${np}#gcc" "${np}#pkg-config" "${np}#protobuf" "${np}#gnumake" "${np}#perl"
  "${np}#patchelf" "${np}#libsodium.dev" "${np}#libsodium.out" "${np}#libarchive.dev"
  "${np}#libarchive.lib" "${np}#zeromq" "${np}#zlib.dev" "${np}#zlib.out")
# The 2019 openssl crates know OpenSSL 1.0.x and 1.1.0 only; the code shipped against 1.0.2.
old=https://github.com/NixOS/nixpkgs/archive/nixos-19.09.tar.gz
ssl_dev=$(nix eval --impure --raw --expr "(import (fetchTarball \"${old}\") {}).openssl_1_0_2.dev.outPath")
ssl_lib=$(nix eval --impure --raw --expr "(import (fetchTarball \"${old}\") {}).openssl_1_0_2.out.outPath")
nix-store -r "$ssl_lib" "$ssl_dev" >/dev/null
pcpath=$(nix build --impure --no-link --print-out-paths "${np}#libsodium.dev" \
  "${np}#libarchive.dev" "${np}#zeromq" "${np}#zlib.dev" | sed 's|$|/lib/pkgconfig|' | paste -sd:)
libpath=$ssl_lib/lib:$(nix build --impure --no-link --print-out-paths "${np}#libsodium.out" \
  "${np}#libarchive.lib" "${np}#zeromq" "${np}#zlib.out" | sed 's|$|/lib|' | paste -sd:)
# prost-build's bundled protoc is a generic Linux binary, which NixOS can't run.
protoc=$(nix build --impure --no-link --print-out-paths "${np}#protobuf" | head -1)/bin/protoc
# libcrypto 1.0.2 finds libdl.so.2 through its own RUNPATH, which is glibc
# 2.27, and that libdl cannot run beside glibc 2.39: a test binary that loads
# libcrypto dies at load. Preloading glibc 2.39's libdl makes libcrypto reuse
# it. build.sh's payload binaries name it with patchelf instead.
libdl=$(nix build --impure --no-link --print-out-paths "${np}#glibc.out")/lib/libdl.so.2
# The nightly rustfmt the tree is formatted with: .rustfmt.toml uses unstable
# options. test.sh runs its cargo-fmt.
rustfmt_nightly=$(nix build --impure --no-link --print-out-paths \
  --expr "(builtins.getFlake \"${np}\").legacyPackages.aarch64-linux.rustfmt.override { asNightly = true; }")
export rustfmt_nightly

run() {
  nix shell --impure "${pkgs[@]}" -c env PROTOC="$protoc" PKG_CONFIG_PATH="$pcpath" \
    LIBRARY_PATH="$libpath" LD_LIBRARY_PATH="$libpath" OPENSSL_LIB_DIR="$ssl_lib/lib" \
    OPENSSL_INCLUDE_DIR="$ssl_dev/include" LD_PRELOAD="$libdl" "$@"
}
