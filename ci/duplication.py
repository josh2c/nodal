#!/usr/bin/env python3
"""Report how much of the repository's Rust is a copy of some other part of it.

The measure is a repeated window of lines. A file is normalised first: each line is
trimmed, its runs of whitespace are collapsed to one space, and blank lines and comment
lines are dropped. Every window of six adjacent normalised lines is then hashed, and a
line counts as duplicated when any window that contains it appears more than once
anywhere in its corpus. A window whose six lines hold fewer than 72 characters together
is ignored, because a run of short lines such as closing braces is shared by every file
and says nothing about duplication.

Two corpora are measured separately, and every Rust line of the workspace belongs to
exactly one of them:

  product   the lines of `crates/*/src/**/*.rs` before the first `#[cfg(test)]`
  test      the lines of those files from the first `#[cfg(test)]` onwards, plus every
            line of `crates/*/tests/`, `tests/*/src/` and `benches/*/src/`

The two are separate because they duplicate for different reasons and are fixed by
different work. Product duplication is the four lifecycle operations copying each other.
Test duplication is a fixture rebuilt in each test file.

The percentage is duplicated lines over normalised lines, not over raw lines, so adding
comments to a file cannot lower its score.

Usage: ci/duplication.py <workspace-root> [--json]
"""

import hashlib
import json
import pathlib
import sys

#: How many adjacent lines make a window.
WINDOW = 6

#: The least a window's six lines may hold together, in characters. Below this the
#: window is punctuation rather than code.
MIN_CHARS = 72

#: Where the product corpus comes from.
PRODUCT_GLOB = "crates/*/src/**/*.rs"

#: Where the test corpus comes from, beyond the inline test modules of the product files.
TEST_GLOBS = ("crates/*/tests/**/*.rs", "tests/*/src/**/*.rs", "benches/*/src/**/*.rs")


def split_at_tests(text):
    """The lines before the first `#[cfg(test)]`, and the lines from it onwards."""
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.strip().startswith("#[cfg(test)]"):
            return lines[:index], lines[index:]
    return lines, []


def normalise(lines):
    """Trim each line, collapse its whitespace, and drop blank and comment lines."""
    kept = []
    for line in lines:
        collapsed = " ".join(line.split())
        if not collapsed or collapsed.startswith("//"):
            continue
        kept.append(collapsed)
    return kept


def windows(lines):
    """Every window of the corpus, as (start index, digest)."""
    for start in range(len(lines) - WINDOW + 1):
        window = lines[start : start + WINDOW]
        if sum(len(line) for line in window) < MIN_CHARS:
            continue
        digest = hashlib.blake2b("\n".join(window).encode(), digest_size=16).hexdigest()
        yield start, digest


def measure(corpus):
    """The duplicated and total normalised line counts of one corpus.

    `corpus` is a list of (name, normalised lines). A window is counted across the whole
    corpus, so a block copied between two files is duplicated in both.
    """
    seen = {}
    for _, lines in corpus:
        for _, digest in windows(lines):
            seen[digest] = seen.get(digest, 0) + 1
    duplicated = 0
    total = 0
    worst = []
    for name, lines in corpus:
        total += len(lines)
        marked = [False] * len(lines)
        for start, digest in windows(lines):
            if seen[digest] > 1:
                for index in range(start, start + WINDOW):
                    marked[index] = True
        count = sum(marked)
        duplicated += count
        if count:
            worst.append((count, len(lines), name))
    worst.sort(reverse=True)
    return duplicated, total, worst


def collect(root):
    """The product and test corpora of the workspace at `root`."""
    product = []
    test = []
    for path in sorted(root.glob(PRODUCT_GLOB)):
        before, after = split_at_tests(path.read_text(errors="replace"))
        product.append((str(path.relative_to(root)), normalise(before)))
        if after:
            test.append((f"{path.relative_to(root)} (inline)", normalise(after)))
    for pattern in TEST_GLOBS:
        for path in sorted(root.glob(pattern)):
            lines = path.read_text(errors="replace").splitlines()
            test.append((str(path.relative_to(root)), normalise(lines)))
    return product, test


def main():
    arguments = sys.argv[1:]
    as_json = "--json" in arguments
    rest = [argument for argument in arguments if argument != "--json"]
    if len(rest) != 1:
        print("usage: ci/duplication.py <workspace-root> [--json]", file=sys.stderr)
        return 2
    root = pathlib.Path(rest[0])
    product, test = collect(root)
    answer = {}
    for label, corpus in (("product", product), ("test", test)):
        duplicated, total, worst = measure(corpus)
        percent = 100.0 * duplicated / total if total else 0.0
        answer[label] = {"duplicated": duplicated, "total": total, "percent": percent}
        if not as_json:
            print(f"{label}: {duplicated} of {total} normalised lines = {percent:.1f}%")
            for count, lines, name in worst[:5]:
                print(f"    {count:5d} of {lines:5d}  {name}")
    if as_json:
        print(json.dumps(answer))
    return 0


if __name__ == "__main__":
    sys.exit(main())
