#!/bin/bash

doj pkg exec core/doj-backline mkdir -m 1777 -p /tmp
doj pkg exec core/doj-backline mkdir -m 0750 -p /root
doj pkg exec core/doj-backline mkdir -m 0755 -p /usr/bin

source /etc/dojo-studio/import_keys.sh
source /etc/dojo-studio/environment.sh

declare -a secrets
readarray -t secrets < <(load_secrets)

case "$1" in
  enter)
    doj pkg exec core/doj-backline env STUDIO_ENTER=true "${secrets[@]}" bash --login +h;;
  build)
    shift
    doj pkg exec core/doj-backline env "${secrets[@]}" /bin/build "$@";;
  run)
    shift
    doj pkg exec core/doj-backline env "${secrets[@]}" bash --login -c "$@";;
  *)
    echo "Unknown Studio Command" && exit 1;;
esac
