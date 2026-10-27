#!/usr/bin/env bash
# Runs inside the builder VM. Builds doj-launch, doj-sup and doj with the Rust
# the tree's rust-toolchain names and packs them, with every shared library they load, into
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
toolchain=$(tr -d '[:space:]' <rust-toolchain)
run rustup toolchain install "$toolchain" --profile minimal
run cargo "+$toolchain" build --release "${lock[@]}" -p dojo-launcher -p dojo_sup -p doj

# The binaries keep their /nix/store paths in the VM. Store paths are copied
# read-only, so make the last staging tree writable before replacing it.
if [[ -d $out/root ]]; then
  chmod -R u+w "$out/root"
  rm -rf "$out/root"
fi
mkdir -p "$out/root/bin" "$out/root/nix/store"
for b in doj-launch doj-sup doj; do
  cp "target/release/$b" "$out/root/bin/$b"
  # The release profile keeps full debug info, which target/ still has. The
  # guest unpacks the payload into RAM, so the copies drop it and keep symbols.
  run strip --strip-debug "$out/root/bin/$b"
  # Append, so the binary's own RUNPATH (its glibc) still comes first.
  run patchelf --add-rpath "$libpath" "$out/root/bin/$b"
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
