# shellcheck disable=2034
pkg_name=doj-plan-build
pkg_origin=karate
pkg_version=$(cat "$SRC_PATH/../../VERSION")
pkg_maintainer="Strip Mall Karate Company"
pkg_license=('Apache-2.0')
pkg_bin_dirs=(bin)

pkg_deps=(karate/bash
          karate/binutils
          karate/bzip2
          karate/coreutils
          karate/file
          karate/findutils
          karate/gawk
          karate/grep
          karate/gzip
          karate/doj
          karate/rq
          karate/sed
          karate/tar
          karate/unzip
          karate/wget
          karate/xz)
pkg_build_deps=(karate/bats)

program=$pkg_name

do_build() {
  cp -v "$SRC_PATH"/bin/${program}.sh "$CACHE_PATH/$program"

  # Use the bash from our dependency list as the shebang. Also, embed the
  # release version of the program.
  # shellcheck disable=2154
  sed \
    -e "s,#!/bin/bash\$,#!$(pkg_path_for bash)/bin/bash," \
    -e "s,^DOJ_PLAN_BUILD=0\.0\.1\$,DOJ_PLAN_BUILD=$pkg_version/$pkg_release," \
    -e "s,^pkg_target='@@pkg_target@@'\$,pkg_target='$pkg_target'," \
    -i "$CACHE_PATH/$program"
}

do_check() {
  bats test
}

do_install() {
  # shellcheck disable=2154
  install -D "$CACHE_PATH/$program" "$pkg_prefix"/bin/$program
  install -D "$SRC_PATH"/bin/shared.bash "$pkg_prefix"/bin/
  install -D "$SRC_PATH"/bin/public.bash "$pkg_prefix"/bin/
  install -D "$SRC_PATH"/bin/environment.bash "$pkg_prefix"/bin/
}
