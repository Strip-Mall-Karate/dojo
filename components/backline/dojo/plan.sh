pkg_name=doj-backline
pkg_origin=karate
pkg_version=$(cat "$SRC_PATH/../../VERSION")
pkg_maintainer="Strip Mall Karate Company"
pkg_license=('Apache-2.0')
pkg_build_deps=()

pkg_deps=(karate/doj-plan-build
          karate/diffutils
          karate/less
          karate/make
          karate/mg
          karate/util-linux
          karate/vim
          karate/ncurses)

do_build() {
  return 0
}

do_install() {
  return 0
}
