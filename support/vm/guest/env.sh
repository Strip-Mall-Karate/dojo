# Sourced by build.sh and test.sh inside the builder VM: the build environment
# for the 0.79.1 tree (Rust 1.33.0, OpenSSL 1.0.2) and run(), which runs a
# command in it.
base=$HOME/dojo-build
np=github:NixOS/nixpkgs/nixos-24.05
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

run() {
  nix shell --impure "${pkgs[@]}" -c env PROTOC="$protoc" PKG_CONFIG_PATH="$pcpath" \
    LIBRARY_PATH="$libpath" LD_LIBRARY_PATH="$libpath" OPENSSL_LIB_DIR="$ssl_lib/lib" \
    OPENSSL_INCLUDE_DIR="$ssl_dev/include" "$@"
}
