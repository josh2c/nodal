//! One module makes a `Proof`, and every surface that removes a home is a caller.
//!
//! `Proof` is the value a safe verdict carries, and a safe verdict is permission to remove a
//! person's working directory. The compiler already stops a second maker: the fields are
//! private, there is no `Default`, no public constructor and no `Deserialize`, and
//! `lifecycle/kernel.rs` carries two `compile_fail` doc tests that hold each of those.
//!
//! This is the other half, and it is a different kind of assertion: the compiler says a
//! caller *cannot* build one, and this says the crate *does not* — that the literal appears
//! in one file, that no second type in the product is called `Proof` or `Evidence`, and that
//! the two readings a surface used to reach for instead are gone.
//!
//! Why read the source rather than run the binary. Every property here is about which code
//! exists, and a run cannot see the difference between "there is one maker" and "the second
//! maker was not reached this time". The three properties below each failed against the tree
//! before this suite was written:
//!
//! - `refs/remotes/` was read as proof of a push in two functions under `git/`, and doctor
//!   printed "pushed" about work a remote had dropped;
//! - `doctor::unique` held its own `Proof` and its own `Evidence`, so a reader who found one
//!   word had to ask which of two things it meant;
//! - the verdict was made in six places, and the two that disagreed were the ones nothing
//!   held to the others.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

/// The one file allowed to write the literal, relative to the workspace root.
const KERNEL: &str = "crates/nodal-core/src/lifecycle/kernel.rs";

/// The argument that answers "is it pushed" from a repository's own remote-tracking refs.
///
/// A repository writes those refs when it fetches or pushes and never corrects them, so one
/// that pushed a branch once reported the branch as pushed for the rest of its life. The
/// argument is asserted rather than a function name, so that a new copy of the same mistake
/// fails this test too. `--not` on its own is not the fault and is still used: `git/outside.rs`
/// excludes named identifiers with it, and `--not --all` asks which objects no ref reaches.
const WEAK: &str = "--remotes";

#[test]
fn the_proof_literal_appears_in_one_file() {
    let mut writers = Vec::new();
    for file in product() {
        if source(&file).contains("Proof {") {
            writers.push(relative(&file));
        }
    }
    assert_eq!(
        writers,
        vec![PathBuf::from(KERNEL)],
        "a `Proof` literal outside the kernel is a second maker of permission to remove a home"
    );
}

/// One name, one thing. A second `Proof` or `Evidence` in the product is a reader being
/// asked which of two values a word means, and the safety core is the last place for that.
#[test]
fn no_second_type_in_the_product_is_called_proof_or_evidence() {
    let kernel = PathBuf::from(KERNEL);
    for word in ["Proof", "Evidence"] {
        let mut declared = Vec::new();
        for file in product() {
            let text = source(&file);
            for shape in ["pub struct", "struct", "pub enum", "enum"] {
                if text.contains(&format!("{shape} {word} "))
                    || text.contains(&format!("{shape} {word}\n"))
                {
                    declared.push(relative(&file));
                    break;
                }
            }
        }
        declared.sort();
        declared.dedup();
        assert_eq!(declared, vec![kernel.clone()], "`{word}` is declared outside the kernel");
    }
}

/// The weak reading is gone from the product, and no file has written it again.
///
/// The safety core reads what a witness vouches for
/// ([`nodal_core::doctor::unique::believed`]) and nothing else. A `rev-list --not --remotes`
/// anywhere in the product is that doctrine being worked around.
#[test]
fn nothing_in_the_product_reads_refs_remotes_as_proof_of_a_push() {
    let mut found = Vec::new();
    for file in product() {
        if code(&source(&file)).iter().any(|line| line.contains(WEAK)) {
            found.push(relative(&file));
        }
    }
    assert!(found.is_empty(), "`--not --remotes` is read as proof of a push in {found:?}");
}

/// Every product `.rs` file of the two crates that ship, inline tests and all.
///
/// The test suites are left out on purpose. A test may name a weak reading to assert that it
/// is gone, and this one does.
fn product() -> Vec<PathBuf> {
    let root = workspace();
    let mut found = Vec::new();
    for crate_name in ["nodal-core", "nodal-cli"] {
        walk(&root.join("crates").join(crate_name).join("src"), &mut found);
    }
    found.sort();
    assert!(
        found.len() > 200,
        "the walk found {} files, so it did not find the source",
        found.len()
    );
    found
}

/// The lines of a file that are not a comment.
///
/// The prose is left out on purpose. Three modules explain in a doc comment why they no
/// longer read `refs/remotes/` as proof, and an assertion that could not tell an
/// explanation from an argument list would delete the explanation.
fn code(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//") && !line.starts_with('*'))
        .collect()
}

/// One file's text.
fn source(file: &Path) -> String {
    std::fs::read_to_string(file).unwrap_or_else(|why| panic!("{}: {why}", file.display()))
}

/// A file's path as this test prints it: relative to the workspace root.
fn relative(file: &Path) -> PathBuf {
    file.strip_prefix(workspace()).unwrap_or(file).to_path_buf()
}

/// The workspace root, from this crate's own manifest directory.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// Collect every `.rs` file under a directory.
fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}
