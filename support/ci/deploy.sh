#!/bin/bash

set -e

# now do the linux unstable build
DOJ_VERSION=$(cat VERSION)
BOOTSTRAP_DIR=/root/travis_bootstrap
TEST_BIN_DIR=/root/doj_bins
TRAVIS_DOJ=${BOOTSTRAP_DIR}/doj
DOJ_DOWNLOAD_URL="https://api.bintray.com/content/dojo/stable/linux/x86_64/doj-%24latest-x86_64-linux.tar.gz?bt_package=doj-x86_64-linux"

export DOJ_ORIGIN=core

# shellcheck disable=2153
if [ "${DOJ_VERSION}" == "${TRAVIS_TAG}" ]; then
    IS_RELEASE_BUILD=1
fi

if [ -n "${IS_RELEASE_BUILD}" ]; then
    # We'll upload to the stable repo, but not *publish* until we've
    # validated it
    BINTRAY_REPO=stable

    # Similarly, we'll promote Dojo packages to a release-specific
    # channel; once we've validated them outside of Travis, we'll
    # promote to stable
    export CI_OVERRIDE_CHANNEL="rc-${DOJ_VERSION}"
else
    # Not a release build, just a normal build from the master
    # branch. It's all unstable, all the time.
    BINTRAY_REPO=unstable
    export CI_OVERRIDE_CHANNEL=unstable
fi

mkdir -p ${BOOTSTRAP_DIR}
# download a doj binary to build doj from source in a studio
wget -O doj.tar.gz "${DOJ_DOWNLOAD_URL}"
# install it in a custom location
tar xvzf ./doj.tar.gz --strip 1 -C ${BOOTSTRAP_DIR}
rm doj.tar.gz

# so key stuff doesn't get funky
unset SUDO_USER

# create our origin key
cat << EOF > core.sig.key
SIG-SEC-1
core-20160810182414

${DOJ_ORIGIN_KEY}
EOF

if [ -n "$DOJ_ORIGIN_KEY" ]; then
  ${TRAVIS_DOJ} origin key import < ./core.sig.key
fi

rm ./core.sig.key

for package in $PACKAGES
do
  # Always build the package if it's a release; otherwise, only build a
  # package if there are relevant code changes.
  if [ -n "${IS_RELEASE_BUILD}" ] || ./support/ci/should_we_build.sh "${package}" ; then

     echo "Building ${package}..."

     echo "--> Clearing any pre-exisiting $DOJ_ORIGIN secret keys from the Studio"
     env DOJ_ORIGIN= ${TRAVIS_DOJ} studio run sh -c \'rm -f /doj/cache/keys/*-*.sig.key\'

     # The names of the Dojo packages we create are often different
     # from the directories in which they reside; specifically, we
     # often add the prefix "doj-" to the name of the package, but
     # leave this off of the directory for ease of navigation. Here,
     # we normalize this.
     if [ -d "components/${package}" ]; then
         directory_name=${package}
     else
         truncated_name=${package##doj-}
         if [ -d "components/${truncated_name}" ] ; then
             directory_name=${truncated_name}
         else
             echo "Cannot find code directory for package '${package}' after looking in the following locations:"
             echo
             echo "    components/${package}"
             echo "    components/${truncated_name}"
             echo
             exit 1
         fi
     fi

     ${TRAVIS_DOJ} studio run DOJ_CARGO_TARGET_DIR=/src/target build "components/${directory_name}"

     source ./results/last_build.env
     # shellcheck disable=2154
     HART="./results/$pkg_artifact"
     ${TRAVIS_DOJ} pkg install "$HART"

     # Once we have built the studio, switch over to bits built here
     if [[ "${package}" == "doj-studio" ]]; then
         # Marker variable that indicates we have a studio artifact to upload.
         PUBLISH_DOJ_STUDIO=true
         # We switch the Doj binary we use here, rather than when we
         # build the doj package, because the version of doj
         # determines the version of the studio we use. Thus, we have
         # to wait to make the switch until there is a studio artifact
         # for us to use.
         TRAVIS_DOJ=$(find /doj/pkgs/core/doj -type f -name doj | tail -1)
     elif [[ "${package}" == "doj" ]]; then
         DOJ_RELEASE_ARTIFACT="${HART}_keep"
         cp "$HART" "$DOJ_RELEASE_ARTIFACT"
     fi

     if [ -n "$DOJ_AUTH_TOKEN" ]; then
         ${TRAVIS_DOJ} pkg upload "$HART" --channel $CI_OVERRIDE_CHANNEL
     fi

     rm "$HART"
  else
     echo "Skipping build of ${package}; no relevant code changes"
  fi
done

echo "--> Removing origin secret keys from Studio"
env DOJ_ORIGIN= "${TRAVIS_DOJ}" studio run sh -c \'rm -f /doj/cache/keys/*-*.sig.key\'

if [ -n "$BINTRAY_USER" ]; then
  if [ ! -d "/doj/pkgs/core/doj-bintray-publish" ] ; then
      # If we didn't build core/doj-bintray-publish and subsequently
      # install it on this run (which is possible on master branch
      # builds), let's ensure that we can still publish things to
      # Bintray, shall we?
      echo "Installing core/doj-bintray-publish from Builder..."
      ${TRAVIS_DOJ} pkg install core/doj-bintray-publish
  fi

  # On 'master' branch builds, we might not build doj-studio. Only
  # publish if we do.
  if [ -n "${PUBLISH_DOJ_STUDIO}" ]; then
      echo "Publishing doj-studio to Bintray"
      env DOJ_BLDR_CHANNEL=$CI_OVERRIDE_CHANNEL "${TRAVIS_DOJ}" pkg exec core/doj-bintray-publish publish-studio
  fi

  # On 'master' branch builds, we might not build a doj artifact to
  # publish. It'll always be there on release builds, though.
  if [ -n "${DOJ_RELEASE_ARTIFACT}" ]; then
      echo "Publishing doj to $BINTRAY_REPO"
      if [ -n "${IS_RELEASE_BUILD}" ]; then
          env DOJ_BLDR_CHANNEL=$CI_OVERRIDE_CHANNEL "${TRAVIS_DOJ}" pkg exec core/doj-bintray-publish publish-doj -s -r $BINTRAY_REPO "$DOJ_RELEASE_ARTIFACT"
      else
          env DOJ_BLDR_CHANNEL=$CI_OVERRIDE_CHANNEL "${TRAVIS_DOJ}" pkg exec core/doj-bintray-publish publish-doj -r $BINTRAY_REPO "$DOJ_RELEASE_ARTIFACT"
      fi
  fi
fi
