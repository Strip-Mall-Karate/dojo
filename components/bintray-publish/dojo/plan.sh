# shellcheck disable=2154
pkg_name=doj-bintray-publish
pkg_origin=core
pkg_version=$(cat "$PLAN_CONTEXT/../../../VERSION")
pkg_maintainer="The Dojo Maintainers <humans@habitat.sh>"
pkg_license=('apachev2')
pkg_deps=(core/coreutils
          core/util-linux
          core/bash
          core/zip
          core/tar
          core/xz
          core/gzip
          core/jfrog-cli
          core/grep
          core/findutils
          core/gawk
          core/doj)
pkg_bin_dirs=(bin)

do_build() {
    cp -v "${SRC_PATH}/bin/publish-doj.sh" "${CACHE_PATH}/publish-doj"

    # Use the bash from our dependency list as the shebang. Also, embed the
    # release version of publish-doj.
    sed \
      -e "s,#!/bin/bash$,#!$(pkg_path_for bash)/bin/bash," \
      -e "s,@author@,$pkg_maintainer,g" \
      -e "s,@version@,$pkg_version/$pkg_release,g" \
      -i "${CACHE_PATH}/publish-doj"
}

do_install() {
    install -v -D "${CACHE_PATH}/publish-doj" "${pkg_prefix}/bin/publish-doj"
}
