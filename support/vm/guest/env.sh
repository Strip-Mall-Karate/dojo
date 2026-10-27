# shellcheck shell=bash
# Sourced by build.sh and test.sh inside the builder VM: the build environment
# for the tree (the Rust its rust-toolchain names) and run(), which runs a
# command in it.
base=$HOME/dojo-build
# nixos-26.05, the commit support/vm/default.nix pins.
np=github:NixOS/nixpkgs/1f607aa79b6356ed45d3cd3c8079c196e79efa2c
export NIX_CONFIG="extra-experimental-features = nix-command flakes"
export GIT_CONFIG_GLOBAL=$base/gitconfig RUSTUP_HOME=$base/rustup CARGO_HOME=$base/cargo
mkdir -p "$base"
: >"$GIT_CONFIG_GLOBAL"
pkgs=("${np}#rustup" "${np}#gcc" "${np}#pkg-config" "${np}#protobuf" "${np}#gnumake" "${np}#perl"
  "${np}#patchelf" "${np}#openssl.dev" "${np}#openssl.out" "${np}#zlib.dev" "${np}#zlib.out")
ssl_dev=$(nix build --impure --no-link --print-out-paths "${np}#openssl.dev")
ssl_lib=$(nix build --impure --no-link --print-out-paths "${np}#openssl.out")
pcpath=$(nix build --impure --no-link --print-out-paths "${np}#openssl.dev" \
  "${np}#zlib.dev" | sed 's|$|/lib/pkgconfig|' | paste -sd:)
libpath=$(nix build --impure --no-link --print-out-paths "${np}#openssl.out" \
  "${np}#zlib.out" | sed 's|$|/lib|' | paste -sd:)
# prost-build's bundled protoc is a generic Linux binary, which NixOS can't run.
protoc=$(nix build --impure --no-link --print-out-paths "${np}#protobuf" | head -1)/bin/protoc
# The nightly rustfmt the tree is formatted with: .rustfmt.toml uses unstable
# options. test.sh runs its cargo-fmt.
rustfmt_nightly=$(nix build --impure --no-link --print-out-paths \
  --expr "(builtins.getFlake \"${np}\").legacyPackages.aarch64-linux.rustfmt.override { asNightly = true; }")
export rustfmt_nightly

run() {
  nix shell --impure "${pkgs[@]}" -c env PROTOC="$protoc" PKG_CONFIG_PATH="$pcpath" \
    LIBRARY_PATH="$libpath" LD_LIBRARY_PATH="$libpath" OPENSSL_LIB_DIR="$ssl_lib/lib" \
    OPENSSL_INCLUDE_DIR="$ssl_dev/include" "$@"
}
