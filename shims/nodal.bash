# The nodal integration for bash. `nodal shell-init --install bash` writes this file
# into Nodal's state directory and puts one line in ~/.bashrc that sources it, and
# `nodal uninstall` removes both again. To load it without installing anything, put
# this line in ~/.bashrc instead:
#
#     eval "$(nodal shell-init bash)"
#
# It adds two things and no more. First, `nodal` becomes a function, so that the two
# commands which name a directory, `nodal new` and `nodal cd`, can move this shell into
# it; every other command goes straight to the binary. Second, a prompt hook exports the
# environment of the unit home the shell is in, and unsets it again on the way out.
#
# Nodal starts no shell of its own and asks nothing when a shell ends.

__nodal_bin='@NODAL_BIN@'

# The binary, found when a command runs and not when this file was written. The path
# above is where the binary that printed this file was; an upgrade moves it, and a
# shell restored from a snapshot can carry the function with the variable empty. Then
# the PATH answers. `type -P` reads the PATH and never this function, which
# `command -v` inside the function would name, and calling that would recurse.
__nodal_program() {
  if [ -x "$__nodal_bin" ]; then
    printf '%s' "$__nodal_bin"
    return 0
  fi
  type -P nodal 2> /dev/null && return 0
  printf 'nodal is not on the path\n' >&2
  return 127
}

# The nearest directory at or above the working directory that carries a manifest.
# The walk is shell code on purpose: it runs on every prompt, and a prompt that starts a
# process to learn that nothing has changed is a prompt a person feels.
__nodal_home() {
  local dir="$PWD"
  while [ -n "$dir" ] && [ "$dir" != '/' ]; do
    if [ -f "$dir/@NODAL_MANIFEST@" ]; then
      printf '%s' "$dir"
      return 0
    fi
    dir="${dir%/*}"
  done
  return 1
}

# Unset what the last activation exported. NODAL_EXPORTED names those variables, so
# this removes what nodal added and never what the person set.
__nodal_leave() {
  local name
  for name in ${NODAL_EXPORTED-}; do
    unset "$name"
  done
  unset NODAL_EXPORTED
  __nodal_entered=''
}

# Export the environment of the home in $1.
__nodal_enter() {
  local program exports
  # The hook runs on every prompt, so a machine with no binary is told once.
  if [ -n "${__nodal_missing-}" ]; then
    program="$(__nodal_program 2> /dev/null)" || return 1
  else
    program="$(__nodal_program)" || { __nodal_missing=1; return 1; }
  fi
  exports="$("$program" env --export --shell bash "$1")" || return 1
  eval "$exports"
  __nodal_entered="$1"
}

# What runs on each prompt: activate on the way in, deactivate on the way out.
# A home direnv has already activated is left alone, because NODAL_ROOT is then the
# home the shell is in and there is nothing to do.
__nodal_hook() {
  local home
  home="$(__nodal_home)" || home=''
  if [ "$home" = "${NODAL_ROOT-}" ] || [ "$home" = "${__nodal_entered-}" ]; then
    return 0
  fi
  __nodal_leave
  [ -n "$home" ] && __nodal_enter "$home"
  return 0
}

# The subcommand in an argument list, skipping global options and their values, so that
# `nodal -v cd unit` and `nodal --store db cd unit` are read the same as `nodal cd unit`.
__nodal_read_verb() {
  __nodal_verb=''
  local skip=0 arg
  for arg in "$@"; do
    if [ "$skip" = 1 ]; then
      skip=0
      continue
    fi
    case "$arg" in
      --store) skip=1 ;;
      -*) ;;
      *)
        __nodal_verb="$arg"
        return 0
        ;;
    esac
  done
  return 1
}

nodal() {
  local program
  program="$(__nodal_program)" || return $?
  __nodal_read_verb "$@"
  case "$__nodal_verb" in
    cd | new) ;;
    *)
      command "$program" "$@"
      return $?
      ;;
  esac
  local file answer
  file="$(mktemp "${TMPDIR:-/tmp}/nodal-cd.XXXXXX")" || return 1
  NODAL_CD_FILE="$file" command "$program" "$@"
  answer=$?
  if [ -s "$file" ]; then
    builtin cd -- "$(cat "$file")" && __nodal_hook
  fi
  rm -f "$file"
  return $answer
}

case "${PROMPT_COMMAND-}" in
  *__nodal_hook*) ;;
  '') PROMPT_COMMAND='__nodal_hook' ;;
  *) PROMPT_COMMAND="__nodal_hook;${PROMPT_COMMAND}" ;;
esac

# A shell that starts inside a home is activated at once, which is what an IDE terminal
# opened on a unit needs.
__nodal_hook
