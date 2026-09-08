//! Nodal makes no network call of its own, and the line it puts in a start-up file
//! evaluates nothing.
//!
//! Two claims, and each is asserted the strong way. The other tests say that a command
//! did not reach a network; these say **no path exists** that could, and that the text
//! which runs in every shell a person opens cannot be made to run anything else.
//!
//! # The binary
//!
//! The only network activity Nodal ever causes is the `git` the user configured talking
//! to the remotes the user configured, with the user's own credentials, and it is
//! visible in progress output (DL-034). Nodal has no update check, no telemetry and no
//! client of any host's API. So the scan looks for the two ways a network call is
//! reached in a Rust program — the standard library's sockets, and an HTTP client
//! crate — over the whole of both crates and over the manifests that could pull one in.
//! A future change that adds one fails here and has to argue with the decision rather
//! than with a test that happened not to notice.
//!
//! # The shell hook
//!
//! The hook runs on every prompt, in every shell, for as long as a person has Nodal
//! installed. It is the most security-sensitive text Nodal writes, and the card for it
//! sets two rules: it evaluates no untrusted content, and it reaches no network. Both
//! are asserted here by reading the emitted script rather than by reading the file in
//! `shims/`, because the emitted script is what a shell runs.
//!
//! "Evaluates no untrusted content" is not "contains no eval". The script has exactly
//! one, and what it evaluates is the output of Nodal's own binary — the assignments
//! that activate a unit home, quoted by `runtime::shells` so that no character of a
//! value is interpreted. The test pins that: one evaluation, of one variable, and that
//! variable is assigned from Nodal's own binary and from nothing else.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::runtime::init;
use nodal_core::runtime::shells::Shell;
use nodal_core::setup::rc;

/// How a network call is reached in a Rust program. Neither half is in Nodal.
const NETWORK_IN_RUST: [&str; 12] = [
    "std::net",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "SocketAddr",
    "to_socket_addrs",
    "reqwest",
    "ureq",
    "isahc",
    "hyper::",
    "Command::new(\"curl\")",
    "Command::new(\"wget\")",
];

/// How a network call is reached from a shell script.
const NETWORK_IN_SHELL: [&str; 10] = [
    "curl", "wget", "/dev/tcp", "/dev/udp", "http://", "https://", "ftp://", "telnet", "nc ",
    "ssh ",
];

/// Every Rust source file both crates ship: the product, not the tests that read it.
fn sources() -> Vec<PathBuf> {
    let root = workspace();
    let mut found = Vec::new();
    for crate_name in ["nodal-core", "nodal-cli"] {
        walk(&root.join("crates").join(crate_name).join("src"), &mut found);
    }
    found
}

/// The manifests that decide what the binary is linked against.
fn manifests() -> Vec<PathBuf> {
    let root = workspace();
    let mut found = vec![root.join("Cargo.toml")];
    for crate_name in ["nodal-core", "nodal-cli"] {
        found.push(root.join("crates").join(crate_name).join("Cargo.toml"));
    }
    found
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

/// A script with its comment lines taken out: what the shell actually runs.
fn code(script: &str) -> String {
    script
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<&str>>()
        .join("\n")
}

/// The rendered integration for one shell, as `nodal shell-init` prints it.
fn script(shell: Shell) -> String {
    init::script(shell, Path::new("/opt/nodal/bin/nodal"))
}

#[test]
fn no_code_path_in_nodal_can_reach_a_network() {
    let mut checked = 0;
    for file in sources() {
        let text = std::fs::read_to_string(&file).unwrap();
        checked += 1;
        for token in NETWORK_IN_RUST {
            assert!(
                !text.contains(token),
                "{}: nodal makes no network call of its own, and {token:?} is how one would be \
                 made (DL-034)",
                file.display()
            );
        }
    }
    assert!(checked > 50, "the scan found only {checked} files, so it proved nothing");
}

#[test]
fn no_manifest_links_nodal_against_a_network_client() {
    for manifest in manifests() {
        let text = std::fs::read_to_string(&manifest).unwrap();
        for token in ["reqwest", "ureq", "isahc", "hyper", "curl"] {
            assert!(
                !text.contains(token),
                "{}: {token:?} is a network client, and nodal is linked against none",
                manifest.display()
            );
        }
    }
}

#[test]
fn the_shell_hook_reaches_no_network() {
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let text = script(shell);
        for token in NETWORK_IN_SHELL {
            assert!(
                !text.contains(token),
                "the {} integration runs in every shell and {token:?} is in it",
                shell.name()
            );
        }
    }
}

#[test]
fn the_shell_hook_evaluates_only_what_nodals_own_binary_printed() {
    for shell in [Shell::Bash, Shell::Zsh] {
        let body = code(&script(shell));
        let evaluations: Vec<&str> =
            body.lines().map(str::trim).filter(|line| line.starts_with("eval")).collect();
        assert_eq!(
            evaluations,
            vec![r#"eval "$exports""#],
            "the {} integration evaluates something else",
            shell.name()
        );
        let assignments: Vec<String> = body
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("exports="))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            assignments,
            vec![format!(
                r#"exports="$("$__nodal_bin" env --export --shell {} "$1")" || return 1"#,
                shell.name()
            )],
            "the {} integration fills $exports from something else",
            shell.name()
        );
    }
}

#[test]
fn the_fish_hook_sources_only_what_nodals_own_binary_printed() {
    let body = code(&script(Shell::Fish));
    let sourced: Vec<&str> =
        body.lines().map(str::trim).filter(|line| line.contains("source")).collect();
    assert_eq!(
        sourced,
        vec![r"printf '%s\n' $exports | source"],
        "the fish integration sources something else"
    );
    assert!(
        body.contains("set -l exports ($__nodal_bin env --export --shell fish $argv[1])"),
        "the fish integration fills $exports from something else: {body}"
    );
}

#[test]
fn the_line_in_a_start_up_file_evaluates_nothing_and_starts_no_process() {
    let shim = Path::new("/home/dev/.nodal/shims/nodal.bash");
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        for created in [false, true] {
            let block = code(&rc::block(shell, shim, created));
            assert!(!block.contains("eval"), "the block a start-up file gets evaluates something");
            assert!(!block.contains("$("), "the block runs a process on every shell start");
            assert!(!block.contains('`'), "the block runs a process on every shell start");
            for token in NETWORK_IN_SHELL {
                assert!(!block.contains(token), "the block reaches a network: {token:?}");
            }
        }
    }
}
