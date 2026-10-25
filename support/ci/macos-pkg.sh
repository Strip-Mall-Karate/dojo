#!/bin/bash

# Fail if there are any unset variables and whenever a command returns a
# non-zero exit code.
set -eu

src_root=$(dirname "$0")/../../

if [ "${TRAVIS_PULL_REQUEST}" = "false" ]; then
  sudo -E "$src_root"/components/doj/mac/mac-build.sh "$src_root"/components/doj/mac
  mkdir -p "$src_root"/out/doj-x86_64-darwin
  source "$src_root"/results/last_build.env
  # shellcheck disable=2154
  cp /doj/pkgs/"$pkg_ident"/bin/doj "$src_root"/out/doj-x86_64-darwin/doj
  zip -9 -r "$src_root"/out/doj-x86_64-darwin.zip "$src_root"/out/doj-x86_64-darwin

# shellcheck disable=2154
cat <<- EOF > "$src_root"/out/doj-bintray.json
{
  "package": {
    "name": "doj-x86_64-darwin",
    "repo": "unstable",
    "subject": "dojo",
    "desc": "",
    "website_url": "https://www.habitat.sh",
    "issue_tracker_url": "https://github.com/habitat-sh/habitat/issues",
    "vcs_url": "https://github.com/reset/dojo",
    "github_use_tag_release_notes": true,
    "github_release_notes_file": "RELEASE.md",
    "licenses": ["Apache-2.0"],
    "labels": [],
    "public_download_numbers": false,
    "public_stats": false
  },
  "version": {
    "name": "${pkg_version}-${pkg_release}",
    "gpgSign": true
  },
  "files": [
    {
      "includePattern": "out/doj-x86_64-darwin.zip",
      "uploadPattern": "darwin/x86_64/doj-${pkg_version}-${pkg_release}-x86_64-darwin.zip"
    }
  ],
  "publish": true
}
EOF
fi
