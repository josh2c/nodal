#!/usr/bin/env sh
# Acceptance test for the adversarial generator: the proof protocol as a test.
#
# One shape is one value from each of five axes — the topology of the store offered as the
# copy, where in the home the work is, what a witness saw of the remote and when, the state
# of the working tree, and what is running against the home. Each shape is built in a
# temporary machine of its own, and two answerers that share no code are asked the same
# question: `nodal reclaim --check --json`, and an oracle of `git` plumbing, `/proc` and
# `lsof` reading the safety contract as a procedure.
#
# One difference fails this job: Nodal says a home is safe to remove and the oracle says a
# member of the contracted loss set has no proven copy outside it. That is work lost. The
# other direction — Nodal refuses and the oracle finds a copy of everything — is a cost and
# not a defect; several of them are deliberate, so each one is printed and counted and none
# of them fails a run.
#
# Two sizes run here:
#
#   - the full shape set: every false-safe closed on the way to rc.4, as named cases that
#     always run. FS-1 in both orderings of a dropped branch, FS-2 on a side branch, a
#     stash, a `wip` record and a detached `HEAD`, FS-3's rewrite of `packed-refs`, FS-6's
#     process this account may not read, FS-8's write from a directory elsewhere, FS-14's
#     blobless clone, FS-15's second predicate, DL-069's copy that went, DL-072's hold whose
#     holder cannot be read, the served remote whose absence hid a regression, and a trash
#     record nothing can parse;
#   - the sample: sixty shapes drawn from one constant seed, holding every value of every
#     axis at least once. It is the same sixty shapes on every run and on both hosts,
#     because a grid that drew at random would turn a defect into a flake and a flake into a
#     restarted job.
#
# The full grid is 7,200 shapes. It is not run here: it runs from `NODAL_ADVERSARIAL=full`,
# which the nightly workflow sets, and it blocks nothing.
#
# Output is not captured. A shape a host will not build says so on standard output, and the
# point of saying it is that somebody reads it.
set -eu

# The suite drives the binary, and Cargo names a binary to the package that declares it, so
# the package under test has to be built before the package that types it runs.
cargo build --locked -p nodal-cli
cargo test --locked -p nodal-adversarial -- --nocapture

echo "acceptance (adversarial): every shape of the full set is reproduced and agrees with the oracle;"
echo "acceptance (adversarial): no shape of the sample is called safe while work goes with the home"
