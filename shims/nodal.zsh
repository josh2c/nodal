# The nodal integration for zsh. `nodal shell-init --install zsh` writes this file
# into Nodal's state directory and puts one line in ~/.zshrc that sources it, and
# `nodal uninstall` removes both again. To load it without installing anything, put
# this line in ~/.zshrc instead:
#
#     eval "$(nodal shell-init zsh)"
#
# It adds two things and no more. First, `nodal` becomes a function, so that the two
# commands which name a directory, `nodal new` and `nodal cd`, can move this shell into
# it; every other command goes straight to the binary. Second, a chpwd hook exports the
# environment of the unit home the shell is in, and unsets it again on the way out.
#
# Nodal starts no shell of its own and asks nothing when a shell ends.

__nodal_bin='@NODAL_BIN@'
[ -x "$__nodal_bin" ] || __nodal_bin='nodal'

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
# this removes what nodal added and never what the person set. zsh does not split a
# parameter on spaces unless it is asked to, which is what ${=...} asks.
__nodal_leave() {
  local name
  for name in ${=NODAL_EXPORTED-}; do
    unset "$name"
  done
  unset NODAL_EXPORTED
  __nodal_entered=''
}

# Export the environment of the home in $1.
__nodal_enter() {
  local exports
  exports="$("$__nodal_bin" env --export --shell zsh "$1")" || return 1
  eval "$exports"
  __nodal_entered="$1"
}

# What runs on each prompt and each directory change: activate on the way in,
# deactivate on the way out. A home direnv has already activated is left alone, because
# NODAL_ROOT is then the home the shell is in and there is nothing to do.
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
  __nodal_read_verb "$@"
  case "$__nodal_verb" in
    cd | new) ;;
    *)
      command "$__nodal_bin" "$@"
      return $?
      ;;
  esac
  local file answer
  file="$(mktemp "${TMPDIR:-/tmp}/nodal-cd.XXXXXX")" || return 1
  NODAL_CD_FILE="$file" command "$__nodal_bin" "$@"
  answer=$?
  if [ -s "$file" ]; then
    builtin cd -- "$(cat "$file")" && __nodal_hook
  fi
  rm -f "$file"
  return $answer
}

# add-zsh-hook is the supported way to add a hook, and it dedupes, so a start-up file
# read twice adds one hook. A zsh whose function files are not installed cannot run it,
# and autoload leaves it looking defined until it is called, so the arrays it would have
# written are checked afterwards rather than asked about first.
autoload -Uz add-zsh-hook 2> /dev/null
add-zsh-hook precmd __nodal_hook 2> /dev/null
add-zsh-hook chpwd __nodal_hook 2> /dev/null
typeset -ag precmd_functions chpwd_functions
(( ${precmd_functions[(I)__nodal_hook]} )) || precmd_functions+=(__nodal_hook)
(( ${chpwd_functions[(I)__nodal_hook]} )) || chpwd_functions+=(__nodal_hook)

# A shell that starts inside a home is activated at once, which is what an IDE terminal
# opened on a unit needs.
__nodal_hook
