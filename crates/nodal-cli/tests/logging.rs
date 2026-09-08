//! An empty `NODAL_LOG` must not silence the binary.
//!
//! `NODAL_LOG=` is what a shell leaves behind after `export NODAL_LOG=` or an unset
//! variable in a wrapper script. Read literally it means "no directives", which hides
//! errors as well as progress, so Nodal treats blank as unset.

#![allow(clippy::unwrap_used)]

mod state;

use state::Machine;

/// A bare `nodal` with the given `NODAL_LOG`, run where there is no project.
///
/// The directory matters. A bare `nodal` reads the one it is run in, and it now answers
/// with the empty list for a directory that holds a `nodal.toml` and no units, which is
/// what a checkout of Nodal itself is. This test is about the logging switches and
/// nothing else, so it runs where neither the registry nor the disk says anything.
fn stderr_with_log_value(machine: &Machine, value: &str) -> String {
    let output = machine
        .nodal()
        .current_dir(machine.path())
        .arg("-vv")
        .env("NODAL_LOG", value)
        .output()
        .unwrap();
    assert!(output.status.success(), "bare invocation exited with {:?}", output.status);
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn blank_log_value_still_traces_at_the_level_the_flags_ask_for() {
    let machine = Machine::new();
    for blank in ["", "   "] {
        let stderr = stderr_with_log_value(&machine, blank);
        assert!(
            stderr.contains("no subcommand given"),
            "NODAL_LOG={blank:?} silenced -vv output: {stderr:?}"
        );
    }
}

#[test]
fn a_set_log_value_still_wins() {
    let machine = Machine::new();
    let stderr = stderr_with_log_value(&machine, "error");
    assert!(stderr.is_empty(), "NODAL_LOG=error should have silenced the debug line: {stderr:?}");
}
