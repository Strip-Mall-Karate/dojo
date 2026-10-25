# shellcheck disable=2034
pkg_name=doj-studio
pkg_origin=core
pkg_version=$(cat "$SRC_PATH/../../VERSION")
pkg_maintainer="Jamie Stormbreaker <jamie@reset.dev>"
pkg_license=('Apache-2.0')
pkg_deps=()
pkg_build_deps=(core/coreutils
                core/tar
                core/xz
                core/wget
                core/busybox-static
                core/doj)
pkg_bin_dirs=(bin)

do_build() {
  cp -v "$SRC_PATH"/bin/doj-studio.sh doj-studio
  cp -v "$SRC_PATH"/libexec/doj-studio-profile.sh .
  cp -v "$SRC_PATH"/libexec/doj-studio-type-*.sh .

  # Embed the release version and author information of the program.
  # shellcheck disable=2154
  sed \
    -e "s,@author@,$pkg_maintainer,g" \
    -e "s,@version@,$pkg_version/$pkg_release,g" \
    -i doj-studio
}

do_install() {
  # shellcheck disable=2154
  install -v -D doj-studio "$pkg_prefix"/bin/doj-studio
  install -v -D doj-studio-profile.sh "$pkg_prefix"/libexec/doj-studio-profile.sh
  for f in doj-studio-type-*.sh; do
    [[ -e $f ]] || break # see http://mywiki.wooledge.org/BashPitfalls#pf1
    install -v -D "$f" "$pkg_prefix"/libexec/"$f"
  done

  lbb="$pkg_prefix/libexec/busybox"

  # Install a copy of a statically built busybox under `libexec/`
  install -v -D "$(pkg_path_for busybox-static)"/bin/busybox "$lbb"

  doj_dir=$(tr '/' '-' < "$(pkg_path_for doj)"/IDENT)
  install -v -D "$(pkg_path_for doj)"/bin/doj \
    "$pkg_prefix"/libexec/"$doj_dir"/bin/doj
  ln -sv "$doj_dir"/bin/doj "$pkg_prefix"/libexec/doj
}
