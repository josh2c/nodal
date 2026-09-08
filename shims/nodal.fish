# The nodal integration for fish. `nodal shell-init --install fish` writes this file
# into Nodal's state directory and puts one line in
# ~/.config/fish/config.fish that sources it, and `nodal uninstall` removes both
# again. To load it without installing anything, put this line in that file instead:
#
#     nodal shell-init fish | source
#
# It adds two things and no more. First, `nodal` becomes a function, so that the two
# commands which name a directory, `nodal new` and `nodal cd`, can move this shell into
# it; every other command goes straight to the binary. Second, a hook on the working
# directory exports the environment of the unit home the shell is in, and erases it
# again on the way out.
#
# Nodal starts no shell of its own and asks nothing when a shell ends.

set -g __nodal_bin '@NODAL_BIN@'
test -x "$__nodal_bin"; or set -g __nodal_bin nodal
set -q __nodal_entered; or set -g __nodal_entered ''

# The nearest directory at or above the working directory that carries a manifest.
function __nodal_home
    set -l dir $PWD
    while test -n "$dir"; and test "$dir" != /
        if test -f "$dir/@NODAL_MANIFEST@"
            echo -n $dir
            return 0
        end
        set dir (string replace -r '/[^/]*$' '' -- $dir)
    end
    return 1
end

# Erase what the last activation exported. NODAL_EXPORTED names those variables.
function __nodal_leave
    if set -q NODAL_EXPORTED
        for name in (string split ' ' -- $NODAL_EXPORTED)
            set -e $name
        end
        set -e NODAL_EXPORTED
    end
    set -g __nodal_entered ''
end

# Export the environment of the home in $argv[1].
function __nodal_enter
    set -l exports ($__nodal_bin env --export --shell fish $argv[1])
    or return 1
    printf '%s\n' $exports | source
    set -g __nodal_entered $argv[1]
end

# What runs on each change of directory. A home direnv has already activated is left
# alone, because NODAL_ROOT is then the home the shell is in.
function __nodal_hook --on-variable PWD
    set -l home (__nodal_home; or echo -n '')
    if test "$home" = "$NODAL_ROOT"; or test "$home" = "$__nodal_entered"
        return 0
    end
    __nodal_leave
    test -n "$home"; and __nodal_enter $home
    return 0
end

# The subcommand in an argument list, skipping global options and their values.
function __nodal_verb
    set -l skip 0
    for arg in $argv
        if test $skip = 1
            set skip 0
            continue
        end
        switch $arg
            case --store
                set skip 1
            case '-*'
            case '*'
                echo -n $arg
                return 0
        end
    end
    return 1
end

function nodal
    switch (__nodal_verb $argv)
        case cd new
            set -l file (mktemp)
            NODAL_CD_FILE=$file command $__nodal_bin $argv
            set -l answer $status
            if test -s $file
                cd (cat $file); and __nodal_hook
            end
            rm -f $file
            return $answer
        case '*'
            command $__nodal_bin $argv
    end
end

# A shell that starts inside a home is activated at once, which is what an IDE terminal
# opened on a unit needs.
__nodal_hook
