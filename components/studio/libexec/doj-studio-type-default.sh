#!/bin/sh

# TESTING CHANGES
# Documentation on testing local changes to this lives here:
# https://github.com/Strip-Mall-Karate/dojo/blob/master/BUILDING.md#testing-changes

# shellcheck disable=2034
studio_type="default"
studio_path="$DOJ_ROOT_PATH/bin"
studio_enter_environment="STUDIO_ENTER=true"
studio_enter_command="$DOJ_ROOT_PATH/bin/doj pkg exec karate/doj-backline bash --login +h"
studio_build_environment=
studio_build_command="_record_build $DOJ_ROOT_PATH/bin/build"
studio_run_environment=
studio_run_command="$DOJ_ROOT_PATH/bin/doj pkg exec karate/doj-backline bash --login"

pkgs="${DOJ_STUDIO_BACKLINE_PKG:-core/doj-backline/$(
  echo "$version" | $bb cut -d / -f 1)}"

run_user="doj"
run_group="$run_user"

finish_setup() {
  if [ -n "$DOJ_ORIGIN_KEYS" ]; then
    # There's a method to this madness: `$doj` is the raw path to `doj`
    # will use the outside cache key path, whereas the `_doj` function has
    # the `$FS_ROOT` set for the inside of the Studio. We're copying from
    # the outside in, using `doj` twice. I love my job.
    for key in $(echo "$DOJ_ORIGIN_KEYS" | $bb tr ',' ' '); do
      # Import the secret origin key, required for signing packages
      info "Importing '$key' secret origin key"
      # shellcheck disable=2154
      if key_text=$($doj origin key export --type secret "$key"); then
        printf -- "%s" "${key_text}" | _doj origin key import
      else
        echo "Error exporting $key key"
        # key_text will contain an error message
        echo "${key_text}"
        echo "Dojo was unable to export your secret signing key. Please"
        echo "verify that you have a signing key for $key present in either"
        # shellcheck disable=2088
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
      if key_text=$($doj origin key export --type public "$key" 2> /dev/null); then
        info "Importing '$key' public origin key"
        printf -- "%s" "${key_text}" | _doj origin key import
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

  if [ -h "$DOJ_STUDIO_ROOT$DOJ_ROOT_PATH/bin/doj" ]; then
    return 0
  fi

  # In the overwhelming majority of instances, you will want to use
  # stable components in your Studio.
  #
  # However, the Dojo team needs to build studios using unstable
  # components in order to fully test things. This `CI_OVERRIDE_CHANNEL`
  # is supplied as a back-door to enable that to happen. It should be
  # set to the name of a channel to preferentially pull packages from.
  #
  # If you're not on the Dojo core team, you will likely never need
  # this, or even have to know it exists.
  #
  # (This is also why we're not using DOJ_BLDR_CHANNEL for this and
  # replicating the fallback logic from doj-plan-build; it'd be too
  # easy to create an unstable studio.)
  for pkg in $pkgs; do
    if [ -n "${CI_OVERRIDE_CHANNEL:-}" ]; then
      info "Override channel set; retrieving ${pkg} from ${CI_OVERRIDE_CHANNEL}"
      _doj install --channel="${CI_OVERRIDE_CHANNEL}" "${pkg}" || {
        info "Package not found in ${CI_OVERRIDE_CHANNEL}; falling back to stable"
        _doj install "${pkg}"
      }
    else
      _doj install "$pkg"
    fi
  done

  bash_path=$(_pkgpath_for karate/bash)
  coreutils_path=$(_pkgpath_for karate/coreutils)

  # shellcheck disable=2086,2154
  $bb mkdir -p $v "$DOJ_STUDIO_ROOT""$DOJ_ROOT_PATH"/bin

  # Put `doj` on the default `$PATH`
  _doj pkg binlink --dest "$DOJ_ROOT_PATH"/bin karate/doj doj

  # Create `/bin/{sh,bash}` for software that hardcodes these shells
  _doj pkg binlink karate/bash bash
  _doj pkg binlink karate/bash sh

  # Create a wrapper to `build` so that any calls to it have a super-stripped
  # `$PATH` and not whatever augmented version is currently in use. This should
  # mean that running `build` from inside a `studio enter` and running `studio
  # build` leads to the exact same experience, at least as far as initial
  # `$PATH` is concerned.
  $bb cat <<EOF > "$DOJ_STUDIO_ROOT""$DOJ_ROOT_PATH"/bin/build
#!$bash_path/bin/sh
exec $DOJ_ROOT_PATH/bin/doj pkg exec karate/doj-plan-build doj-plan-build "\$@"
EOF
  # shellcheck disable=2086
  $bb chmod $v 755 "$DOJ_STUDIO_ROOT""$DOJ_ROOT_PATH"/bin/build

  # Set the login shell for any relevant user to be `/bin/bash`
  $bb sed -e "s,/bin/sh,$bash_path/bin/bash,g" -i "$DOJ_STUDIO_ROOT"/etc/passwd

  $bb cat >> "$DOJ_STUDIO_ROOT"/etc/profile <<PROFILE
# Add doj to the default PATH at the front so any wrapping scripts will
# be found and called first
export PATH=$DOJ_ROOT_PATH/bin:\$PATH
export DOJ_BINLINK_DIR=/doj/bin

# Colorize grep/egrep/fgrep by default
alias grep='grep --color=auto'
alias egrep='egrep --color=auto'
alias fgrep='fgrep --color=auto'

# Set TERMINFO so doj can give us a delightful experience.
export TERMINFO
TERMINFO=$(_pkgpath_for karate/ncurses)/share/terminfo

emacs() {
  if command -v emacs > /dev/null; then
    emacs "\$@"
  else
    mg "\$@"
  fi
}

if [[ -n "\${DOJ_STUDIO_SUP}" ]]; then
  # This environment variable does not handle spaces well, so we'll re-add
  # them...
  DOJ_STUDIO_SUP="\$(echo "\$DOJ_STUDIO_SUP" | sed 's/__sp__/ /g')"
fi

sup-run() {
  mkdir -p /doj/sup/default
  echo "--> Launching the Dojo Supervisor in the background..."
  echo "    Running: doj sup run \$@"
  setsid doj sup run "\$@" > /doj/sup/default/sup.log 2>&1 &
  disown \$!
  echo "    * Use 'doj svc start' & 'doj svc stop' to start and stop services"
  echo "    * Use 'sup-log' to tail the Supervisor's output (Ctrl+c to stop)"
  echo "    * Use 'sup-term' to terminate the Supervisor"
  if [[ -z "\${DOJ_STUDIO_SUP:-}" ]]; then
    echo "    * To pass custom arguments to run the Supervisor, export"
    echo "      'DOJ_STUDIO_SUP' with the arguments before running"
    echo "      'doj studio enter'."
  fi
  echo ""
}

sup-term() {
  if doj sup term ; then
    echo "--> Killed background running Dojo Supervisor."
  else
    echo "--> Error killing Supervisor; it may not be running"
  fi
}

sup-log() {
  mkdir -p /doj/sup/default
  touch /doj/sup/default/sup.log
  echo "--> Tailing the Dojo Supervisor's output (use 'Ctrl+c' to stop)"
  tail -f /doj/sup/default/sup.log
}

alias sr='sup-run'
alias st='sup-term'
alias sl='sup-log'

if [[ -n "\${STUDIO_ENTER:-}" ]]; then
  unset STUDIO_ENTER
  source /etc/profile.enter
fi

# Add command line completion
source <(doj cli completers --shell bash)
PROFILE

  $bb cat > "$DOJ_STUDIO_ROOT"/etc/profile.enter <<PROFILE_ENTER
# Source /src/.studiorc so we can apply user-specific configuration
if [[ -f /src/.studiorc && -z "\${DOJ_STUDIO_NOSTUDIORC:-}" ]]; then
  echo "--> Detected and loading /src/.studiorc"
  echo ""
  source /src/.studiorc
fi

# Automatically run the Dojo Supervisor
case "\${DOJ_STUDIO_SUP:-}" in
  false|FALSE|no|NO|0)
    # If false, we don't run the Supervisor
    ;;
  *)
    # shellcheck disable=2086
    sup-run \${DOJ_STUDIO_SUP:-}
    echo "--> To prevent a Supervisor from running automatically in your"
    echo "    Studio, export 'DOJ_STUDIO_SUP=false' before running"
    echo "    'doj studio enter'."
    echo ""
    ;;
esac
PROFILE_ENTER

  echo "${run_user}:x:42:42:root:/:/bin/sh" >> "$DOJ_STUDIO_ROOT"/etc/passwd
  echo "${run_group}:x:42:${run_user}" >> "$DOJ_STUDIO_ROOT"/etc/group

  studio_env_command="$coreutils_path/bin/env"
}

# Intentionally using a subshell here so `unset` doesn't affect the
# caller's environment.
_doj() (
    # We remove a couple of env vars we do not want for this instance of the studio
    unset DOJ_CACHE_KEY_PATH
    unset DOJ_BLDR_CHANNEL
    $bb env FS_ROOT="$DOJ_STUDIO_ROOT" "$doj" "$@"
)

_pkgpath_for() {
  _doj pkg path "$1" | $bb sed -e "s,^$DOJ_STUDIO_ROOT,,g"
}
