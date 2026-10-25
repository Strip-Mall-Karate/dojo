#!/bin/bash

source /etc/dojo-studio/logging.sh

: "${DOJ_ORIGIN_KEYS:=${DOJ_ORIGIN:-}}"

if [ -n "$DOJ_ORIGIN_KEYS" ]; then
  # There's a method to this madness: `doj` is the raw path to `doj`
  # will use the outside cache key path, whereas the `doj` function has
  # the `$FS_ROOT` set for the inside of the Studio. We're copying from
  # the outside in, using `doj` twice. I love my job.
  for key in $(echo "$DOJ_ORIGIN_KEYS" | doj pkg exec karate/doj-backline tr ',' ' '); do
    key_text=""
    # Import the secret origin key, required for signing packages
    info "Importing '$key' secret origin key"
    if key_text=$(doj origin key export --type secret "$key"); then
      printf -- "%s" "${key_text}" | doj origin key import
    else
      echo "Error exporting $key key"
      # key_text will contain an error message
      echo "${key_text}"
      echo "Dojo was unable to export your secret signing key. Please"
      echo "verify that you have a signing key for $key present in either"
      # shellcheck disable=SC2088
      echo "~/.doj/cache/keys (if running via sudo) or /doj/cache/keys"
      echo "(if running as root). You can test this by running:"
      echo ""
      echo "    doj origin key export --type secret $key"
      echo ""
      echo "This test will print your signing key to the console or error"
      echo "if it cannot find the key. To create a signing key, you can run: "
      echo ""
      echo "    doj origin key generate $key"
      echo ""
      echo "You'll also be prompted to create an origin signing key when "
      echo "you run 'doj setup'."
      echo ""
      exit 1
    fi
    # Attempt to import the public origin key, which can be used for local
    # package installations where the key may not yet be uploaded.
    if key_text=$(doj origin key export --type public "$key" 2> /dev/null); then
      info "Importing '$key' public origin key"
      printf -- "%s" "${key_text}" | doj origin key import
    else
      info "Tried to import '$key' public origin key, but key was not found"
    fi
  done
else
  info "No secret keys imported! Did you mean to set DOJ_ORIGIN?"
  echo "To specify a DOJ_ORIGIN, either set the DOJ_ORIGIN environment"
  echo "variable to your origin name or run 'doj setup' and specify a"
  echo "default origin."
fi
