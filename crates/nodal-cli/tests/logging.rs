//! An empty `NODAL_LOG` must not silence the binary.
//!
//! `NODAL_LOG=` is what a shell leaves behind after `export NODAL_LOG=` or an unset
//! variable in a wrapper script. Read literally it means "no directives", which hides
//! errors as well as progress, so Nodal treats blank as unset.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn stderr_with_log_value(value: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_nodal"))
        .arg("-vv")
        .env("NODAL_LOG", value)
        .output()
        .unwrap();
    assert!(output.status.success(), "bare invocation exited with {:?}", output.status);
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn blank_log_value_still_traces_at_the_level_the_flags_ask_for() {
    for blank in ["", "   "] {
        let stderr = stderr_with_log_value(blank);
        assert!(
            stderr.contains("no subcommand given"),
            "NODAL_LOG={blank:?} silenced -vv output: {stderr:?}"
        );
    }
}

#[test]
fn a_set_log_value_still_wins() {
    let stderr = stderr_with_log_value("error");
    assert!(stderr.is_empty(), "NODAL_LOG=error should have silenced the debug line: {stderr:?}");
}
