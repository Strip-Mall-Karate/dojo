#!/bin/bash

source /etc/dojo-studio/logging.sh


# Removes any potential malicious secrets
sanitize_secrets() {
  for x in DOJ_BINLINK_DIR DOJ_ORIGIN HOME LC_ALL PATH PWD STUDIO_TYPE TERM TERMINFO; do
    unset "DOJ_STUDIO_SECRET_$x"
  done
}

# Builds up a secret environment based on the prefix `DOJ_STUDIO_SECRET_`
# to pass into the studio
load_secrets() {
  sanitize_secrets
  doj pkg exec karate/doj-backline env | doj pkg exec karate/doj-backline awk -F '=' '/^DOJ_STUDIO_SECRET_/ {gsub(/DOJ_STUDIO_SECRET_/, ""); print}'
}

if [ -n "${DOJ_CONFIG_EXCLUDE:-}" ]; then
info "Exported: DOJ_CONFIG_EXCLUDE=$DOJ_CONFIG_EXCLUDE"
fi
if [ -n "${DOJ_AUTH_TOKEN:-}" ]; then
info "Exported: DOJ_AUTH_TOKEN=[redacted]"
fi
if [ -n "${DOJ_ORIGIN:-}" ]; then
info "Exported: DOJ_ORIGIN=$DOJ_ORIGIN"
fi
if [ -n "${DOJ_BLDR_URL:-}" ]; then
info "Exported: DOJ_BLDR_URL=$DOJ_BLDR_URL"
fi
if [ -n "${DOJ_BLDR_CHANNEL:-}" ]; then
info "Exported: DOJ_BLDR_CHANNEL=$DOJ_BLDR_CHANNEL"
fi
if [ -n "${DOJ_NOCOLORING:-}" ]; then
info "Exported: DOJ_NOCOLORING=$DOJ_NOCOLORING"
fi
if [ -n "${DOJ_NONINTERACTIVE:-}" ]; then
info "Exported: DOJ_NONINTERACTIVE=$DOJ_NONINTERACTIVE"
fi
if [ -n "${DOJ_STUDIO_NOSTUDIORC:-}" ]; then
info "Exported: DOJ_STUDIO_NOSTUDIORC=$DOJ_STUDIO_NOSTUDIORC"
fi
if [ -n "${DOJ_STUDIO_SUP:-}" ]; then
info "Exported: DOJ_STUDIO_SUP=$DOJ_STUDIO_SUP"
fi
if [ -n "${DOJ_UPDATE_STRATEGY_FREQUENCY_MS:-}" ]; then
info "Exported: DOJ_UPDATE_STRATEGY_FREQUENCY_MS=$DOJ_UPDATE_STRATEGY_FREQUENCY_MS"
fi
if [ -n "${http_proxy:-}" ]; then
info "Exported: http_proxy=$http_proxy"
fi
if [ -n "${https_proxy:-}" ]; then
info "Exported: https_proxy=$https_proxy"
fi
if [ -n "${no_proxy:-}" ]; then
info "Exported: no_proxy=$no_proxy"
fi

for secret_name in $(load_secrets | doj pkg exec karate/coreutils cut -d = -f 1); do
info "Exported: $secret_name=[redacted]"
done
