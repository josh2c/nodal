#!/usr/bin/env sh
# Acceptance test for developer reliability: the suite leaves no process of its own running.
#
# Every other acceptance script asserts a property of the product. This one asserts a
# property of the tests themselves, because that property was broken and nothing failed. A
# fixture that kills the one process it has a handle on leaves the family that process
# started running, reparented, standing in a temporary directory that has been unlinked
# underneath it. It leaves one on every run, and no test fails because of it.
# `tests/safety/src/process.rs` tells the story and holds the owner that fixed it.
#
# So the question is asked from outside the suite, once, here.
#
# How the question is scoped. `TEST_OWNER_RUN` is exported before the suite runs, and every
# process a fixture starts carries it, as does every process the product starts from a
# command a fixture built. The value names this run of this script and nothing else on the
# machine, so the scan reports this run's processes and never a person's own work. Nothing
# is matched by command name and nothing is ever signalled.
#
# The suite is not allowed to take the scan down with it. A failing test is the likeliest
# thing to leave a process behind — that is the case fixtures unwind through — so the status
# is kept and the scan runs either way, and the job fails if either of them did.
#
# What it does not cover. A fixture that starts a process under `env -i` hands it an empty
# environment on purpose, and the marker goes with everything else;
# `tests/safety/tests/hook_processes.rs` is the one that does, and what it starts is the
# product's to stop and is asserted as such.
#
# Linux only, and the job is registered for Linux only. The scan reads the environment of
# every process, which is `/proc/<pid>/environ`; macOS publishes no such thing for a script
# to read, so the claim cannot be made there. A host without it fails this script rather than
# passing it, because a green run that asserted nothing is worse than no run at all.
set -eu

if [ ! -d /proc ]; then
    echo "this host publishes no process environments to scan, so the claim that the suite left nothing running cannot be made here" >&2
    exit 1
fi

TEST_OWNER_RUN="nodal-suite-$$-$(date +%s)"
export TEST_OWNER_RUN

trees_now() {
    find "${TMPDIR:-/tmp}" -maxdepth 1 -name '.tmp*' 2>/dev/null | wc -l | tr -d ' '
}

before=$(trees_now)
echo "process hygiene: marker $TEST_OWNER_RUN, ${before} temporary trees before the run"

suite=0
cargo test --workspace --locked || suite=$?

echo "process hygiene: $(trees_now) temporary trees after the run"

left=""
for entry in /proc/[0-9]*; do
    pid=${entry#/proc/}
    # A process belonging to somebody else has an environment this account may not read, and
    # that is not a finding: the marker is only ever on a process of this run's own. The
    # redirection is inside a subshell so that the refusal is silent as well.
    [ -r "$entry/environ" ] || continue
    if (tr '\0' '\n' < "$entry/environ") 2>/dev/null | grep -qx "TEST_OWNER_RUN=$TEST_OWNER_RUN"; then
        left="$left $pid"
    fi
done

if [ -n "$left" ]; then
    echo "the suite left these processes running:" >&2
    # shellcheck disable=SC2086 # the list is process identifiers this script just read.
    ps -o pid=,pgid=,lstart=,args= -p $left >&2 || true
    echo "each one was started by a test of this run and is still here after it." >&2
    exit 1
fi

if [ "$suite" -ne 0 ]; then
    echo "the suite left no process of its own running, and it failed: cargo test exited $suite" >&2
    exit "$suite"
fi

echo "acceptance (process hygiene): the whole suite left zero processes of its own running"
