//! Where Nodal keeps its state, and where a unit's home goes inside it.
//!
//! The home path is a contract (`docs/contracts.md`, home path policy):
//! `<nodal directory>/<project>/e/<id>/`, and the last segment is the same length for
//! every unit of a project. Equal length is the whole point of the rule. Build caches
//! and virtual environments record the absolute path they were installed at, so a home
//! whose path is a different length from the base's cannot be made valid by rewriting
//! bytes in place; keeping every home of a project the same length keeps that repair
//! available to the tasks that need it.
//!
//! Nothing here touches a disk. The functions are pure over their inputs except
//! [`directory`], which reads one environment variable, so a test can put a whole
//! Nodal state directory in a temporary place.

use std::path::{Path, PathBuf};

use crate::model::{BaseId, EnvId, ProjectName, Slug};
use crate::{Error, Result};

/// The environment variable that moves the state directory, as `NODAL_STORE` moves the
/// registry inside it.
pub const DIRECTORY_VAR: &str = "NODAL_HOME";

/// The state directory's name under the user's own directory.
const DIRECTORY_NAME: &str = ".nodal";

/// The registry file inside the state directory.
const REGISTRY_NAME: &str = "registry.db";

/// The segment that separates a project's homes from anything else it may keep.
const ENVIRONMENTS: &str = "e";

/// The segment a project's bases sit under.
///
/// A base is what homes are cloned *from*, so it is never part of what reclaiming a
/// unit takes away. Keeping the two under different segments makes that structural: a
/// walk of the homes of a project cannot reach a base, whatever it is looking for.
const BASES: &str = "b";

/// The segment a project's reclaimed homes sit under until `nodal gc` removes them.
///
/// A third segment beside the homes and the bases, for the same structural reason: a
/// walk of the live homes of a project cannot reach a trashed one, and a walk of the
/// trash cannot reach a live home. Nothing has to remember to skip anything.
const TRASH: &str = "trash";

/// How many characters of an environment's identifier name its home.
///
/// The last eight characters of a ULID are eight of its ten random ones, which is forty
/// bits: two homes of one project colliding is not something a person will meet. The
/// last, not the first, because the first ten characters are the timestamp and two
/// units created in the same millisecond share them.
pub const SEGMENT_LEN: usize = 8;

/// Where Nodal keeps its state on this machine.
///
/// # Errors
/// [`Error::NoHomeDirectory`] when neither [`DIRECTORY_VAR`] nor a home directory says
/// where it should be.
pub fn directory() -> Result<PathBuf> {
    if let Some(moved) = std::env::var_os(DIRECTORY_VAR).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(moved));
    }
    let home = user_directory().ok_or(Error::NoHomeDirectory { variable: DIRECTORY_VAR })?;
    Ok(home.join(DIRECTORY_NAME))
}

/// The person's own directory, as the operating system publishes it.
///
/// The start-up files the shell integration goes in are under it, and they are not
/// under the state directory: `NODAL_HOME` moves Nodal's state and never moves a
/// person's `~/.bashrc`.
///
/// # Errors
/// [`Error::NoHomeDirectory`] when the platform's own variable does not say where it is.
pub fn user() -> Result<PathBuf> {
    user_directory().ok_or(Error::NoHomeDirectory { variable: DIRECTORY_VAR })
}

/// The registry's default path: `registry.db` in the state directory.
///
/// # Errors
/// As [`directory`].
pub fn registry() -> Result<PathBuf> {
    Ok(directory()?.join(REGISTRY_NAME))
}

/// The directory a project's bases live in, under the state directory.
///
/// # Errors
/// As [`directory`].
pub fn bases(project: &ProjectName) -> Result<PathBuf> {
    Ok(bases_in_directory(&directory()?, project))
}

/// The same directory, under a state directory the caller names.
#[must_use]
pub fn bases_in_directory(root: &Path, project: &ProjectName) -> PathBuf {
    root.join(project_segment(project)).join(BASES)
}

/// Where one base lives. Named by the same eight characters a home is, and for the
/// same reason: every base of a project has a path of one length, so a cache that
/// recorded the path it was installed at can be repaired in place.
#[must_use]
pub fn for_base(root: &Path, project: &ProjectName, base: BaseId) -> PathBuf {
    bases_in_directory(root, project).join(base_segment(base))
}

/// The segment that names one base.
#[must_use]
pub fn base_segment(base: BaseId) -> String {
    tail(&base.to_string())
}

/// The directory a project's reclaimed homes are moved to, under the state directory.
///
/// # Errors
/// As [`directory`].
pub fn trash(project: &ProjectName) -> Result<PathBuf> {
    Ok(trash_in_directory(&directory()?, project))
}

/// The same directory, under a state directory the caller names.
#[must_use]
pub fn trash_in_directory(root: &Path, project: &ProjectName) -> PathBuf {
    root.join(project_segment(project)).join(TRASH)
}

/// Where one reclaimed home is put: `<root>/<project>/trash/<id>`, named by the same
/// eight characters the live home was.
///
/// The name is the home's, not a new one, so a person who wrote the path down before
/// the reclaim can still find the directory afterwards.
#[must_use]
pub fn trashed(root: &Path, project: &ProjectName, environment: EnvId) -> PathBuf {
    trash_in_directory(root, project).join(segment(environment))
}

/// The home of one materialisation, under the state directory.
///
/// # Errors
/// As [`directory`].
pub fn for_environment(project: &ProjectName, environment: EnvId) -> Result<PathBuf> {
    Ok(in_directory(&directory()?, project, environment))
}

/// The same path, under a state directory the caller names. This is what a plan keeps,
/// so that a run rebuilt in another process places the home where the first one did.
#[must_use]
pub fn in_directory(root: &Path, project: &ProjectName, environment: EnvId) -> PathBuf {
    root.join(project_segment(project)).join(ENVIRONMENTS).join(segment(environment))
}

/// The directory segment a project's homes sit under.
///
/// A project name is a line a person reads and may hold a space or a slash, so it is
/// reduced to a slug. A name that reduces to nothing — one written in a script this
/// rule does not cover — falls back to `project`, and the identifying part of the path
/// is then the environment segment, which is unique on its own.
#[must_use]
pub fn project_segment(project: &ProjectName) -> String {
    slugify(project.as_str()).map_or_else(|| String::from("project"), |slug| slug.to_string())
}

/// The segment that names one materialisation.
#[must_use]
pub fn segment(environment: EnvId) -> String {
    tail(&environment.to_string())
}

/// The last [`SEGMENT_LEN`] characters of an identifier.
fn tail(text: &str) -> String {
    text[text.len().saturating_sub(SEGMENT_LEN)..].to_owned()
}

/// A slug for a line of text, `None` when nothing of it survives the rule.
fn slugify(text: &str) -> Option<Slug> {
    let mut slug = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            slug.extend(character.to_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    Slug::parse(slug.trim_matches('-')).ok()
}

/// The user's own directory, as the operating system publishes it.
#[cfg(unix)]
fn user_directory() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Windows publishes the same thing under another name.
#[cfg(not(unix))]
fn user_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use super::{ENVIRONMENTS, SEGMENT_LEN, in_directory, project_segment, segment, slugify};
    use crate::model::{BaseId, EnvId, ProjectName};

    fn environment(last: char) -> EnvId {
        format!("01J8Z6H000000000000000000{last}").parse().unwrap()
    }

    fn project(name: &str) -> ProjectName {
        ProjectName::parse(name).unwrap()
    }

    #[test]
    fn every_home_of_a_project_has_a_path_of_the_same_length() {
        let root = Path::new("/home/u/.nodal");
        let one = in_directory(root, &project("storefront"), environment('1'));
        let two = in_directory(root, &project("storefront"), environment('Z'));
        assert_ne!(one, two);
        assert_eq!(one.as_os_str().len(), two.as_os_str().len());
        assert!(one.ends_with(Path::new(ENVIRONMENTS).join(segment(environment('1')))));
    }

    #[test]
    fn a_home_is_named_by_the_random_end_of_the_identifier() {
        let text = environment('7').to_string();
        assert_eq!(segment(environment('7')), text[text.len() - SEGMENT_LEN..]);
    }

    #[test]
    fn a_base_is_never_inside_the_directory_homes_are_reclaimed_from() {
        let root = Path::new("/home/u/.nodal");
        let name = project("storefront");
        let base: BaseId = "01J8Z6H000000000000000000B".parse().unwrap();
        let path = super::for_base(root, &name, base);
        assert!(path.starts_with(super::bases_in_directory(root, &name)));
        assert!(!path.starts_with(in_directory(root, &name, environment('1')).parent().unwrap()));
        assert_eq!(
            path.as_os_str().len(),
            in_directory(root, &name, environment('1')).as_os_str().len(),
            "a base path and a home path are the same length"
        );
    }

    #[test]
    fn a_reclaimed_home_keeps_its_name_and_leaves_the_live_homes() {
        let root = Path::new("/home/u/.nodal");
        let name = project("storefront");
        let live = in_directory(root, &name, environment('1'));
        let gone = super::trashed(root, &name, environment('1'));
        assert_ne!(live, gone);
        assert_eq!(live.file_name(), gone.file_name());
        assert!(gone.starts_with(super::trash_in_directory(root, &name)));
        assert!(!gone.starts_with(live.parent().unwrap()));
    }

    #[test]
    fn a_project_name_becomes_one_directory_segment() {
        assert_eq!(project_segment(&project("Storefront Web")), "storefront-web");
        assert_eq!(project_segment(&project("api/v2")), "api-v2");
        assert_eq!(project_segment(&project("...")), "project");
    }

    #[test]
    fn a_slug_never_leads_or_trails_with_a_dash() {
        assert_eq!(slugify(" leading and trailing ").unwrap().as_str(), "leading-and-trailing");
        assert!(slugify("///").is_none());
    }
}
