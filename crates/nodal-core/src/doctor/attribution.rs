//! Whose a thing is, when its name is the only evidence there is.
//!
//! A path answers "whose is this" well. A Docker resource often has no path to answer
//! with: a stopped container mounts nothing, a volume nothing refers to has no mount
//! and no label, and both of them still carry a project's name in the name a person or
//! a compose file gave them. `<project>_db` and `supabase_db_<project>` are the two
//! shapes a measured machine held, sixteen rows and 740 MB of them, and every one was
//! filed under the project the command was run in while the section for other projects
//! said nothing was there.
//!
//! So a name is evidence, and this module says how much. It compares a resource's name
//! with the project names doctor can already know, and it answers only when the
//! comparison is positive.
//!
//! ## What doctor can know a project is called
//!
//! Two sources, both of them already read ([`super::Scope`]):
//!
//! - every project the registry holds, by the name the registry gives it;
//! - the directory name of every checkout doctor surveys, which is the surveyed root
//!   and the root of each other project.
//!
//! Nothing is walked and nothing new is read to build this. A machine whose registry is
//! empty knows one name, its own checkout's, and attributes nothing to anybody else.
//!
//! ## What counts as a match
//!
//! A name is split at every character that is not a letter or a digit, so
//! `supabase_db_acme-web` is `supabase`, `db`, `acme`, `web`. A project matches when its
//! own tokens appear in that list as a run, in order. `acme-web` matches those four
//! tokens; `acme` alone matches them too; `webacme` matches nothing, because half a
//! token is not a name.
//!
//! The longest match wins, so a machine holding both `acme` and `acme-web` attributes
//! `supabase_db_acme-web` to `acme-web`. **A tie goes to this project.** Two projects
//! whose names match a resource equally well is not evidence that the resource is
//! another project's, and the second section may hold only things that are.
//!
//! ## What this never does
//!
//! It never moves a row that a label or a mount already placed. A container Nodal
//! started names the materialisation it serves, and a container that mounts a directory
//! is standing in that directory; both are better evidence than a name and both are
//! read first ([`super::containers`]).
//!
//! It never attributes on no match. "I cannot say whose this is" stays in the first
//! section, which is the rule the whole second section is built on.

use std::path::Path;

use crate::doctor::{Scope, Section};

/// The project names doctor can know, and the section each of them stands for.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// One candidate per name: the name split into tokens, and whose name it is.
    candidates: Vec<(Vec<String>, Section)>,
}

impl Names {
    /// The names of every project doctor knows about, from what it has already read.
    #[must_use]
    pub fn of(scope: &Scope) -> Self {
        let mut names = Self::default();
        if let Some(root) = &scope.root {
            names.add(directory_of(root), Section::Here);
        }
        for known in &scope.projects {
            let section = if scope.root.as_ref() == Some(&known.root) {
                Section::Here
            } else {
                Section::Elsewhere
            };
            names.add(Some(known.project.name.to_string()), section);
            names.add(directory_of(&known.root), section);
        }
        names
    }

    /// Which section a resource's name is evidence for, `None` when it is evidence for
    /// nothing.
    #[must_use]
    pub fn section(&self, resource: &str) -> Option<Section> {
        let tokens = tokens(resource);
        let mut best: Option<(usize, Section)> = None;
        for (name, section) in &self.candidates {
            if !runs_through(&tokens, name) {
                continue;
            }
            best = Some(match best {
                Some((length, held)) if length > name.len() => (length, held),
                // A tie is not evidence that a thing is another project's, so the two
                // sections settle it rather than the order the candidates were built in.
                Some((length, Section::Here)) if length == name.len() => (length, Section::Here),
                _ => (name.len(), *section),
            });
        }
        best.map(|(_, section)| section)
    }

    /// Record one name, when there is a name to record.
    fn add(&mut self, name: Option<String>, section: Section) {
        let Some(name) = name else { return };
        let tokens = tokens(&name);
        if tokens.is_empty() {
            return;
        }
        self.candidates.push((tokens, section));
    }
}

/// The name of the directory a root sits at.
fn directory_of(root: &Path) -> Option<String> {
    root.file_name().and_then(std::ffi::OsStr::to_str).map(ToOwned::to_owned)
}

/// A name split into the words a name is made of, in lower case.
fn tokens(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether `name` appears in `tokens` as a run, in order.
fn runs_through(tokens: &[String], name: &[String]) -> bool {
    if name.is_empty() || name.len() > tokens.len() {
        return false;
    }
    tokens.windows(name.len()).any(|window| window == name)
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::PathBuf;

    use super::{Names, Section, runs_through, tokens};
    use crate::doctor::{Known, Scope};
    use crate::model::{Digest, Project, ProjectId, ProjectName};

    /// A registry row for a project of this name, rooted at a directory of that name.
    fn project(name: &str, root: &str) -> Known {
        Known {
            project: Project {
                id: ProjectId::from_ulid(ulid::Ulid::new()),
                root: PathBuf::from(root),
                name: ProjectName::parse(name).expect("a name"),
                recipe_hash: Digest::parse("0".repeat(64)).expect("a digest"),
                created_at: crate::model::Timestamp::now(),
            },
            root: PathBuf::from(root),
        }
    }

    /// A machine surveyed in `acme`, which also holds a project called `storefront`.
    fn scope() -> Scope {
        Scope {
            root: Some(PathBuf::from("/home/dev/code/acme")),
            projects: vec![
                project("acme", "/home/dev/code/acme"),
                project("storefront", "/home/dev/code/storefront"),
            ],
            ..Scope::default()
        }
    }

    #[test]
    fn a_name_that_holds_another_projects_name_is_that_projects() {
        let names = Names::of(&scope());
        for resource in ["storefront_db", "supabase_db_storefront", "/storefront-web-1"] {
            assert_eq!(
                names.section(resource),
                Some(Section::Elsewhere),
                "{resource} names another project"
            );
        }
    }

    #[test]
    fn a_name_that_holds_this_projects_name_is_this_projects() {
        let names = Names::of(&scope());
        assert_eq!(names.section("acme_db"), Some(Section::Here));
        assert_eq!(names.section("supabase_db_acme"), Some(Section::Here));
    }

    #[test]
    fn a_name_that_matches_nothing_is_evidence_for_nothing() {
        let names = Names::of(&scope());
        for resource in ["redis", "postgres-16", "storefrontweb_db", "app_db"] {
            assert_eq!(names.section(resource), None, "{resource} names no project doctor knows");
        }
    }

    /// A machine holding `acme` and `acme-web` reads `supabase_db_acme-web` as the
    /// second project's, because the second name is the whole of what the resource is
    /// called and the first is a part of it.
    #[test]
    fn the_longest_name_wins() {
        let mut scope = scope();
        scope.projects.push(project("acme-web", "/home/dev/code/acme-web"));
        let names = Names::of(&scope);
        assert_eq!(names.section("supabase_db_acme-web"), Some(Section::Elsewhere));
        assert_eq!(names.section("acme_db"), Some(Section::Here), "this project is still here");
    }

    /// Two projects matching a name equally well is not evidence of anything, and the
    /// section that may hold only positive evidence is the second one.
    #[test]
    fn a_tie_goes_to_this_project() {
        let mut scope = scope();
        scope.projects.push(project("db", "/home/dev/code/db"));
        let names = Names::of(&scope);
        assert_eq!(names.section("acme_db"), Some(Section::Here));
    }

    #[test]
    fn a_machine_with_no_registry_still_knows_the_name_of_the_checkout_it_is_in() {
        let scope = Scope { root: Some(PathBuf::from("/home/dev/code/acme")), ..Scope::default() };
        let names = Names::of(&scope);
        assert_eq!(names.section("acme_db"), Some(Section::Here));
        assert_eq!(names.section("other_db"), None, "it knows no other project to name");
    }

    #[test]
    fn a_name_is_split_at_every_character_that_is_not_a_letter_or_a_digit() {
        assert_eq!(tokens("supabase_db_acme-web.1"), ["supabase", "db", "acme", "web", "1"]);
        assert!(tokens("///").is_empty());
    }

    #[test]
    fn half_a_token_is_not_a_name() {
        let acme = [String::from("acme")];
        assert!(runs_through(&tokens("supabase_db_acme"), &acme));
        assert!(!runs_through(&tokens("supabase_db_acmeweb"), &acme));
        assert!(!runs_through(&tokens("db"), &acme));
    }
}
