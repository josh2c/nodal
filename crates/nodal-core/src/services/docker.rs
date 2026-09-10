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
//! The same rule holds for what a machine has left behind. [`leftovers`] reads the
//! containers that have exited and the volumes nothing refers to, so `nodal doctor` can
//! report them with a size. [`leftovers`] only reads: `docker ps`, `docker inspect` and
//! `docker system df` are the three commands it runs, and each one only answers.
//! [`remove`] is the module's one destructive call, it belongs to `nodal reclaim`, and
//! doctor never reaches it.
//!
//! Two labels are the contract a container carries: [`UNIT_LABEL`] and [`ENV_LABEL`].
//! Every container Nodal starts carries both, which is what makes attribution certain
//! rather than inferred. A container Nodal did not start carries neither, and is
//! attributed by the home it mounts, if it mounts one.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

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
        // The standard output stays empty, and deliberately so: `docker inspect` writes
        // every container's environment there, and the reason below is the whole of
        // what may be said about this document.
        output: Box::new(crate::error::Streams {
            stdout: String::new(),
            stderr: format!("a container document could not be read at column {}", error.column()),
        }),
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

/// One container that has exited, reduced to what a report of it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exited {
    /// The name Docker shows, without the leading slash.
    pub name: String,
    /// The image it was started from.
    pub image: String,
    /// When it stopped, as Docker writes the instant.
    pub finished_at: String,
    /// The bytes of its writable layer. This is what the container itself holds; the
    /// image under it is shared with every other container started from it.
    pub bytes: u64,
    /// Its labels, by name, which is how a container Nodal started names its unit.
    pub labels: BTreeMap<String, String>,
    /// The host paths it mounts, which is how a container Nodal did not start is
    /// attributed to the tree it was working on.
    pub mounts: Vec<PathBuf>,
}

impl Exited {
    /// One label, when the container carries it.
    #[must_use]
    pub fn label(&self, name: &str) -> Option<&str> {
        self.labels.get(name).map(String::as_str)
    }
}

/// One volume no container refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// The volume's name.
    pub name: String,
    /// What it holds, as `docker system df` counts it, or `None` when Docker did not
    /// say. Docker reports a size only when it was asked for the verbose table, and it
    /// reports it as text; a figure that cannot be read is left out rather than guessed.
    pub bytes: Option<u64>,
}

/// What a machine has left behind, as far as Docker is concerned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Leftovers {
    /// Every container that has exited, in the order Docker listed them.
    pub exited: Vec<Exited>,
    /// Every volume no container refers to, in the order Docker listed them.
    pub dangling: Vec<Volume>,
}

/// What a look at this machine's leftovers produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sweep {
    /// The daemon answered.
    Ran(Leftovers),
    /// The daemon could not be reached, and this is what it said.
    Unavailable {
        /// One line, as the tool reported it, for a note a person reads.
        why: String,
    },
}

/// Every exited container and unreferenced volume on this machine, or why there is no
/// answer.
///
/// This function removes nothing. It is the read `nodal doctor` is built on, and doctor
/// reports rather than acts.
///
/// # Errors
/// [`Error::Tool`] when the daemon answered and then wrote a document that is not the
/// one `docker inspect` documents.
pub fn leftovers(docker: &dyn Docker) -> Result<Sweep> {
    let listed = match run(
        docker,
        &["ps", "--all", "--filter", "status=exited", "--quiet", "--no-trunc"],
    )? {
        Ok(output) => output,
        Err(why) => return Ok(Sweep::Unavailable { why }),
    };
    let ids: Vec<&str> = listed.stdout.lines().map(str::trim).filter(|id| !id.is_empty()).collect();
    let mut exited = Vec::new();
    for batch in ids.chunks(BATCH) {
        let mut args = vec!["inspect", "--size", "--format", "{{json .}}"];
        args.extend(batch.iter().copied());
        match run(docker, &args)? {
            Ok(output) => exited.extend(exited_in(&output.stdout)?),
            Err(why) => return Ok(Sweep::Unavailable { why }),
        }
    }
    let dangling = match run(docker, &["system", "df", "--verbose", "--format", "{{json .}}"])? {
        Ok(output) => dangling_in(&output.stdout),
        Err(why) => return Ok(Sweep::Unavailable { why }),
    };
    Ok(Sweep::Ran(Leftovers { exited, dangling }))
}

/// The exited containers in the output of `docker inspect --size --format '{{json .}}'`.
///
/// # Errors
/// [`Error::Tool`] when a line is not the document `docker inspect` documents.
pub fn exited_in(text: &str) -> Result<Vec<Exited>> {
    text.lines().filter(|line| !line.trim().is_empty()).map(one_exited).collect()
}

/// One line of `docker inspect --size` output as an exited container.
fn one_exited(line: &str) -> Result<Exited> {
    let inspected: InspectedFull = serde_json::from_str(line).map_err(|error| Error::Tool {
        program: String::from(PROGRAM),
        args: vec![String::from("inspect")],
        dir: PathBuf::from("."),
        code: Some(0),
        // The standard output stays empty, and deliberately so: `docker inspect` writes
        // every container's environment there, and the reason below is the whole of
        // what may be said about this document.
        output: Box::new(crate::error::Streams {
            stdout: String::new(),
            stderr: format!("a container document could not be read at column {}", error.column()),
        }),
    })?;
    Ok(Exited {
        name: inspected.name.trim_start_matches('/').to_owned(),
        image: inspected.config.image.unwrap_or_default(),
        finished_at: inspected.state.finished_at.unwrap_or_default(),
        bytes: inspected.size_rw.unwrap_or(0),
        labels: inspected.config.labels.unwrap_or_default(),
        mounts: inspected.mounts.into_iter().filter_map(|mount| mount.source).collect(),
    })
}

/// The volumes nothing refers to, from `docker system df --verbose --format '{{json .}}'`.
///
/// Docker writes one JSON document holding a table per kind of object, and the volume
/// rows carry `Links` and a `Size` written for a person. A row whose size cannot be read
/// is still reported, without one: the name is the part a person acts on.
#[must_use]
pub fn dangling_in(text: &str) -> Vec<Volume> {
    let Some(document) = text.lines().find(|line| !line.trim().is_empty()) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<DiskUsage>(document) else {
        return Vec::new();
    };
    parsed
        .volumes
        .into_iter()
        .filter(|volume| number_of(&volume.links) == Some(0))
        .map(|volume| Volume { name: volume.name, bytes: size_of(&volume.size) })
        .collect()
}

/// A count Docker wrote either as a number or as text. The command line has written
/// both, so both are read here rather than one being assumed.
fn number_of(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.trim().parse().ok())
}

/// A size Docker wrote either as a count of bytes or as text for a person.
fn size_of(value: &Value) -> Option<u64> {
    if let Some(count) = value.as_u64() {
        return Some(count);
    }
    bytes_of(value.as_str()?)
}

/// A size as the Docker command line writes it for a person: `1.4GB`, `233.2MB`, `0B`.
///
/// Docker counts in powers of ten here, as a disk does, so the scale below does too. A
/// figure this cannot read is `None`, and the volume is reported without a size.
fn bytes_of(text: &str) -> Option<u64> {
    const SCALE: [(&str, f64); 5] =
        [("TB", 1e12), ("GB", 1e9), ("MB", 1e6), ("kB", 1e3), ("B", 1.0)];
    let text = text.trim();
    for (unit, scale) in SCALE {
        if let Some(number) = text.strip_suffix(unit)
            && let Ok(value) = number.trim().parse::<f64>()
            && value >= 0.0
        {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a size Docker printed for a person is far inside u64"
            )]
            return Some((value * scale) as u64);
        }
    }
    None
}

/// The part of `docker inspect --size` output an exited container is read from.
#[derive(Debug, Deserialize)]
struct InspectedFull {
    /// The container name, which Docker writes with a leading slash.
    #[serde(rename = "Name")]
    name: String,
    /// Where the labels and the image name are.
    #[serde(rename = "Config", default)]
    config: InspectedFullConfig,
    /// Where the instant it stopped is.
    #[serde(rename = "State", default)]
    state: InspectedState,
    /// The bytes of the writable layer, which `--size` adds.
    #[serde(rename = "SizeRw")]
    size_rw: Option<u64>,
    /// Every mount, bind and volume alike.
    #[serde(rename = "Mounts", default)]
    mounts: Vec<InspectedMount>,
}

/// The configuration of an inspected container, with the image it was started from.
#[derive(Debug, Default, Deserialize)]
struct InspectedFullConfig {
    /// The labels, which Docker writes as null when there are none.
    #[serde(rename = "Labels")]
    labels: Option<BTreeMap<String, String>>,
    /// The image reference.
    #[serde(rename = "Image")]
    image: Option<String>,
}

/// The state of an inspected container.
#[derive(Debug, Default, Deserialize)]
struct InspectedState {
    /// When it stopped, as Docker writes the instant.
    #[serde(rename = "FinishedAt")]
    finished_at: Option<String>,
}

/// The part of `docker system df --verbose` output volumes are read from.
#[derive(Debug, Deserialize)]
struct DiskUsage {
    /// The volume rows.
    #[serde(rename = "Volumes", default)]
    volumes: Vec<DiskUsageVolume>,
}

/// One volume row of `docker system df --verbose`.
#[derive(Debug, Deserialize)]
struct DiskUsageVolume {
    /// The volume's name.
    #[serde(rename = "Name")]
    name: String,
    /// How many containers refer to it. Zero means nothing does.
    #[serde(rename = "Links", default)]
    links: Value,
    /// What it holds, as a count of bytes or as text for a person.
    #[serde(rename = "Size", default)]
    size: Value,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{Container, Docker, Output, Survey, Sweep, UNIT_LABEL, containers_in, survey};
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

    /// One exited container as `docker inspect --size` writes it, and one volume table
    /// as `docker system df --verbose` writes it.
    const INSPECTED_EXITED: &str = concat!(
        r#"{"Name":"/acme-replay-db","Config":{"Image":"postgres:16","Labels":{"nodal.environment":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}},"#,
        r#""State":{"FinishedAt":"2026-08-10T09:00:00Z"},"SizeRw":120000000,"SizeRootFs":900000000,"#,
        r#""Mounts":[{"Source":"/home/j/code/acme"}]}"#,
        "\n"
    );

    const DISK_USAGE: &str = concat!(
        r#"{"Images":[],"Containers":[],"Volumes":[{"Name":"acme_pgdata","Links":0,"Size":"1.4GB"},"#,
        r#"{"Name":"live_pgdata","Links":2,"Size":"800MB"},"#,
        r#"{"Name":"counted","Links":"0","Size":512}]}"#,
        "\n"
    );

    #[test]
    fn an_exited_container_carries_its_own_layer_and_not_its_images() {
        let exited = super::exited_in(INSPECTED_EXITED).unwrap();
        assert_eq!(exited.len(), 1);
        assert_eq!(exited[0].name, "acme-replay-db");
        assert_eq!(exited[0].image, "postgres:16");
        assert_eq!(exited[0].bytes, 120_000_000, "the writable layer, not the image under it");
        assert_eq!(exited[0].finished_at, "2026-08-10T09:00:00Z");
        assert_eq!(exited[0].label(super::ENV_LABEL), Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert_eq!(exited[0].mounts, vec![PathBuf::from("/home/j/code/acme")]);
    }

    #[test]
    fn only_a_volume_nothing_refers_to_is_a_leftover() {
        let dangling = super::dangling_in(DISK_USAGE);
        let names: Vec<&str> = dangling.iter().map(|volume| volume.name.as_str()).collect();
        assert_eq!(names, ["acme_pgdata", "counted"]);
        assert_eq!(dangling[0].bytes, Some(1_400_000_000), "docker's own figure, read as bytes");
        assert_eq!(dangling[1].bytes, Some(512), "a count of bytes is read as one");
    }

    #[test]
    fn a_table_that_cannot_be_read_is_no_volumes_rather_than_a_failure() {
        assert!(super::dangling_in("not json\n").is_empty());
        assert!(super::dangling_in("").is_empty());
    }

    #[test]
    fn a_daemon_that_is_not_there_is_a_reason_for_the_leftovers_too() {
        let Sweep::Unavailable { why } = super::leftovers(&Missing).unwrap() else {
            panic!("a missing docker was reported as an answer");
        };
        assert_eq!(why, "docker is not installed");
    }

    /// The type is public, so a caller can build one; this keeps that honest.
    #[test]
    fn a_container_without_the_label_says_so() {
        let container =
            Container { name: String::from("x"), labels: BTreeMap::new(), mounts: Vec::new() };
        assert_eq!(container.label(UNIT_LABEL), None);
    }
}
