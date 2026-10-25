#!/usr/bin/env bash
# Runs inside the builder VM. Builds doj-launch, doj-sup and doj with Rust
# 1.33.0 and packs them, with every shared library they load, into
# <out-dir>/payload.cpio.gz for support/vm/boot --payload.
set -euo pipefail

usage() {
  echo "usage: build.sh <tree-dir> <out-dir> [--update-lock]" >&2
  exit 2
}
[[ $# -ge 2 && $# -le 3 ]] || usage
mkdir -p "$2"
tree=$(realpath "$1") out=$(realpath "$2")
lock=(--locked)
case "${3-}" in
  "") ;;
  --update-lock) lock=() ;;
  *) usage ;;
esac

. "$(dirname "$0")/env.sh"
cd "$tree"
run rustup toolchain install 1.33.0 --profile minimal
run cargo +1.33.0 build --release "${lock[@]}" -p dojo-launcher -p dojo_sup -p doj

# The binaries keep their /nix/store paths in the VM. Store paths are copied
# read-only, so make the last staging tree writable before replacing it.
if [[ -d $out/root ]]; then
  chmod -R u+w "$out/root"
  rm -rf "$out/root"
fi
mkdir -p "$out/root/bin" "$out/root/nix/store"
for b in doj-launch doj-sup doj; do
  cp "target/release/$b" "$out/root/bin/$b"
  # Append, so the binary's own RUNPATH (its glibc) still comes first.
  run patchelf --add-rpath "$libpath" "$out/root/bin/$b"
  # libcrypto 1.0.2 finds libdl.so.2 through its own RUNPATH, which is glibc
  # 2.27. Needing it here loads glibc 2.39's first, and libcrypto reuses it.
  run patchelf --add-needed libdl.so.2 "$out/root/bin/$b"
done
interp=$(run patchelf --print-interpreter "$out/root/bin/doj-sup")
mapfile -t libdirs < <(tr : '\n' <<<"$libpath" | sed 's|/lib$||')
nix-store -qR "${libdirs[@]}" "$(dirname "$(dirname "$interp")")" | sort -u >"$out/closure"
while read -r p; do
  cp -a "$p" "$out/root/nix/store/"
done <"$out/closure"
(cd "$out/root" && find . | cpio --quiet -o -H newc -R 0:0) | gzip -9 >"$out/payload.cpio.gz"

status=0
for b in doj-launch doj-sup doj; do
  if ldd "$out/root/bin/$b" | grep -i "not found" >&2; then
    echo "build.sh: $b has unresolved libraries" >&2
    status=1
  fi
done
exit "$status"
