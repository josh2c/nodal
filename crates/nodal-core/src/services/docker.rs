//! The one place in `nodal-core` that spawns `docker`.
//!
//! Every caller builds an argument list and hands it here, so mocking the tool is a
//! single seam (`docs/code-structure.md`). Nothing here interprets a container beyond
//! decoding what `docker inspect` writes.
//!
//! A machine with no Docker, and a machine whose daemon this account may not reach, are
//! ordinary conditions rather than failures: Nodal is a tool for working on a project,
//! and a person who has no containers still wants an answer from the commands that read
//! them. So [`survey`] reports [`Survey::Unavailable`] with the reason, and the caller
//! turns that into a note. Only a daemon that answers and then writes something
//! unreadable is an error.
//!
//! Two labels are the contract a container carries: [`UNIT_LABEL`] and [`ENV_LABEL`].
//! Every container Nodal starts carries both, which is what makes attribution certain
//! rather than inferred. A container Nodal did not start carries neither, and is
//! attributed by the home it mounts, if it mounts one.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

use crate::{Error, Result};

/// The label a container carries to say which unit it belongs to.
pub const UNIT_LABEL: &str = "nodal.unit";

/// The label a container carries to say which materialisation it serves.
pub const ENV_LABEL: &str = "nodal.environment";

/// The program this module runs.
const PROGRAM: &str = "docker";

/// What one `docker` invocation produced.
#[derive(Debug, Clone)]
pub struct Output {
    /// Standard output, lossily decoded.
    pub stdout: String,
    /// Standard error, lossily decoded, trailing whitespace removed.
    pub stderr: String,
    /// Exit code, or `None` when a signal ended the process.
    pub code: Option<i32>,
}

impl Output {
    /// Whether the invocation exited zero.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// Where container facts come from.
pub trait Docker {
    /// Run `docker` with these arguments and return what it produced, whatever the exit
    /// code: a non-zero exit is an answer here, not a failure.
    ///
    /// # Errors
    /// [`Error::ToolSpawn`] when the `docker` binary could not be started.
    fn run(&self, args: &[&str]) -> Result<Output>;
}

/// The Docker of this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cli;

impl Docker for Cli {
    fn run(&self, args: &[&str]) -> Result<Output> {
        let output = Command::new(PROGRAM)
            .args(args)
            .output()
            .map_err(|source| Error::ToolSpawn { program: String::from(PROGRAM), source })?;
        Ok(Output {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_owned(),
            code: output.status.code(),
        })
    }
}

/// One running container, reduced to what attribution reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// The name Docker shows, without the leading slash.
    pub name: String,
    /// Its labels, by name.
    pub labels: BTreeMap<String, String>,
    /// The host paths it mounts, in the order Docker lists them.
    pub mounts: Vec<PathBuf>,
}

impl Container {
    /// One label, when the container carries it.
    #[must_use]
    pub fn label(&self, name: &str) -> Option<&str> {
        self.labels.get(name).map(String::as_str)
    }
}

/// What a look at this machine's containers produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Survey {
    /// The daemon answered. The containers are every one that is running.
    Ran(Vec<Container>),
    /// The daemon could not be reached, and this is what it said.
    Unavailable {
        /// One line, as the tool reported it, for a note a person reads.
        why: String,
    },
}

/// Every running container on this machine, or why there is no answer.
///
/// # Errors
/// [`Error::Tool`] when the daemon answered and then wrote a document that is not the
/// one `docker inspect` documents.
pub fn survey(docker: &dyn Docker) -> Result<Survey> {
    let listed = match run(docker, &["ps", "--quiet", "--no-trunc"])? {
        Ok(output) => output,
        Err(why) => return Ok(Survey::Unavailable { why }),
    };
    let ids: Vec<&str> = listed.stdout.lines().map(str::trim).filter(|id| !id.is_empty()).collect();
    let mut containers = Vec::new();
    // A machine can have more containers than one command line holds, and how many is
    // not Nodal's to decide, so the ids are inspected a batch at a time rather than
    // capped. Every container is still reported.
    for batch in ids.chunks(BATCH) {
        let mut args = vec!["inspect", "--format", "{{json .}}"];
        args.extend(batch.iter().copied());
        match run(docker, &args)? {
            Ok(output) => containers.extend(containers_in(&output.stdout)?),
            Err(why) => return Ok(Survey::Unavailable { why }),
        }
    }
    Ok(Survey::Ran(containers))
}

/// What removing a unit's containers did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Removed {
    /// The containers that are no longer there, by the name Docker shows.
    pub containers: Vec<String>,
    /// Why fewer went than were asked for, when there is a reason worth printing. A
    /// machine with no daemon is one of these, and never a failure: a unit whose
    /// containers cannot be reached is still a unit whose home can be reclaimed.
    pub why: Option<String>,
}

/// Remove containers by name, stopping them first.
///
/// `docker rm --force` is one call rather than a stop and then a remove, because the
/// two-call form has a window in between: a container that exits on its own between
/// them makes the second call fail on a container that is already the state that was
/// wanted. Removing one that is not there is not a failure either — the name is simply
/// not in the answer.
///
/// # Errors
/// [`Error::Tool`] only for a failure that is not the daemon being unreachable, which
/// is reported as [`Removed::why`].
pub fn remove(docker: &dyn Docker, names: &[String]) -> Result<Removed> {
    if names.is_empty() {
        return Ok(Removed::default());
    }
    let mut removed = Removed::default();
    for batch in names.chunks(BATCH) {
        let mut args = vec!["rm", "--force", "--volumes"];
        args.extend(batch.iter().map(String::as_str));
        match run(docker, &args)? {
            Ok(output) => removed.containers.extend(
                output
                    .stdout
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned),
            ),
            Err(why) => {
                removed.why = Some(why);
                return Ok(removed);
            }
        }
    }
    Ok(removed)
}

/// How many containers one `docker inspect` is asked about. An identifier is 64
/// characters, so a batch is about 16 kB of arguments whatever the machine is running.
const BATCH: usize = 200;

/// One invocation: its output when it worked, and the reason when it did not.
///
/// A binary that is not installed and a daemon that refuses are one answer here, so the
/// caller has one thing to say rather than two.
fn run(docker: &dyn Docker, args: &[&str]) -> Result<std::result::Result<Output, String>> {
    match docker.run(args) {
        Err(Error::ToolSpawn { .. }) => Ok(Err(String::from("docker is not installed"))),
        Err(other) => Err(other),
        Ok(output) if output.ok() => Ok(Ok(output)),
        Ok(output) => Ok(Err(reason(&output))),
    }
}

/// Why an invocation that ran did not work, as one line.
fn reason(output: &Output) -> String {
    let first = output.stderr.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        String::from("docker exited without an answer")
    } else {
        first.to_owned()
    }
}

/// The containers in the output of `docker inspect --format '{{json .}}'`, which writes
/// one JSON document per line.
///
/// Kept apart from the running so that the decoding is tested on every host, not only on
/// one that has a daemon.
///
/// # Errors
/// [`Error::Tool`] when a line is not the document `docker inspect` documents.
pub fn containers_in(text: &str) -> Result<Vec<Container>> {
    text.lines().filter(|line| !line.trim().is_empty()).map(one).collect()
}

/// One line of `docker inspect` output as a container.
///
/// A document that cannot be read is reported by its position and by nothing else. The
/// full output of `docker inspect` holds every container's environment, so neither the
/// line nor a decoder's message about it may reach an error, a log or a screen.
fn one(line: &str) -> Result<Container> {
    let inspected: Inspected = serde_json::from_str(line).map_err(|error| Error::Tool {
        program: String::from(PROGRAM),
        args: vec![String::from("inspect")],
        dir: PathBuf::from("."),
        code: Some(0),
        stderr: format!("a container document could not be read at column {}", error.column()),
    })?;
    Ok(Container {
        name: inspected.name.trim_start_matches('/').to_owned(),
        labels: inspected.config.labels.unwrap_or_default(),
        mounts: inspected.mounts.into_iter().filter_map(|mount| mount.source).collect(),
    })
}

/// The part of `docker inspect` output this module reads.
#[derive(Debug, Deserialize)]
struct Inspected {
    /// The container name, which Docker writes with a leading slash.
    #[serde(rename = "Name")]
    name: String,
    /// Where the labels are.
    #[serde(rename = "Config", default)]
    config: InspectedConfig,
    /// Every mount, bind and volume alike.
    #[serde(rename = "Mounts", default)]
    mounts: Vec<InspectedMount>,
}

/// The configuration of an inspected container.
#[derive(Debug, Default, Deserialize)]
struct InspectedConfig {
    /// The labels, which Docker writes as null when there are none.
    #[serde(rename = "Labels")]
    labels: Option<BTreeMap<String, String>>,
}

/// One mount of an inspected container.
#[derive(Debug, Deserialize)]
struct InspectedMount {
    /// The host path, which a named volume does not have.
    #[serde(rename = "Source")]
    source: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{Container, Docker, Output, Survey, UNIT_LABEL, containers_in, survey};
    use crate::{Error, Result};

    /// Two containers as `docker inspect --format '{{json .}}'` writes them: one Nodal
    /// started, one a person did, which mounts a home and carries no label.
    const INSPECTED: &str = concat!(
        r#"{"Name":"/nodal-worker-import-db","Config":{"Labels":{"nodal.unit":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}},"Mounts":[{"Source":null,"Name":"pgdata"}]}"#,
        "\n",
        r#"{"Name":"/redis","Config":{"Labels":null},"Mounts":[{"Source":"/home/j/.nodal/p/e/01"}]}"#,
        "\n",
    );

    /// A Docker that answers with fixed text, and records what it was asked.
    struct Fake(Vec<Output>);

    impl Docker for Fake {
        fn run(&self, _args: &[&str]) -> Result<Output> {
            Ok(self.0.first().cloned().unwrap_or(Output {
                stdout: String::new(),
                stderr: String::new(),
                code: Some(0),
            }))
        }
    }

    /// A Docker that is not installed.
    struct Missing;

    impl Docker for Missing {
        fn run(&self, _args: &[&str]) -> Result<Output> {
            Err(Error::ToolSpawn {
                program: String::from("docker"),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
        }
    }

    /// A Docker whose daemon refuses this account.
    struct Refused;

    impl Docker for Refused {
        fn run(&self, _args: &[&str]) -> Result<Output> {
            Ok(Output {
                stdout: String::new(),
                stderr: String::from(
                    "permission denied while trying to connect to the docker API\nsecond line",
                ),
                code: Some(1),
            })
        }
    }

    #[test]
    fn a_label_and_a_bind_mount_both_survive_the_decoding() {
        let containers = containers_in(INSPECTED).unwrap();
        assert_eq!(containers.len(), 2);
        assert_eq!(containers[0].name, "nodal-worker-import-db");
        assert_eq!(containers[0].label(UNIT_LABEL), Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(containers[0].mounts.is_empty(), "a named volume has no host path");
        assert_eq!(containers[1].label(UNIT_LABEL), None);
        assert_eq!(containers[1].mounts, vec![PathBuf::from("/home/j/.nodal/p/e/01")]);
    }

    #[test]
    fn a_document_that_is_not_one_is_an_error_rather_than_a_container() {
        assert!(containers_in("not json\n").is_err());
    }

    #[test]
    fn a_machine_with_no_containers_answers_with_none_of_them() {
        let empty =
            Fake(vec![Output { stdout: String::new(), stderr: String::new(), code: Some(0) }]);
        assert_eq!(survey(&empty).unwrap(), Survey::Ran(Vec::new()));
    }

    #[test]
    fn a_docker_that_is_not_there_is_a_reason_rather_than_a_failure() {
        let Survey::Unavailable { why } = survey(&Missing).unwrap() else {
            panic!("a missing docker was reported as an answer");
        };
        assert_eq!(why, "docker is not installed");
    }

    #[test]
    fn a_daemon_that_refuses_is_reported_in_its_own_first_line() {
        let Survey::Unavailable { why } = survey(&Refused).unwrap() else {
            panic!("a refused daemon was reported as an answer");
        };
        assert_eq!(why, "permission denied while trying to connect to the docker API");
    }

    /// The type is public, so a caller can build one; this keeps that honest.
    #[test]
    fn a_container_without_the_label_says_so() {
        let container =
            Container { name: String::from("x"), labels: BTreeMap::new(), mounts: Vec::new() };
        assert_eq!(container.label(UNIT_LABEL), None);
    }
}
