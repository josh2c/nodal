//! The project: one Git repository Nodal manages units for.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::fingerprint::Digest;
use crate::model::ids::ProjectId;
use crate::model::scalar::{self, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// A project's display name, taken from its directory or its remote.
    ProjectName, kind = "project name", shape = scalar::LINE
}

string_newtype! {
    /// What repository a project is, in the one spelling every clone of it agrees on.
    ///
    /// It is the normalised form of a remote URL and never the URL as somebody typed
    /// it: `git@github.com:josh2c/nodal.git` and `https://github.com/josh2c/nodal`
    /// are one repository, so they are one of these
    /// ([`crate::git::remote::identity`]).
    RemoteUrl, kind = "remote url", shape = scalar::LINE
}

/// A repository Nodal knows about, and the recipe it was last read with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    /// Identity of the project.
    pub id: ProjectId,
    /// Absolute path of the repository root on this host.
    ///
    /// It is where the project was first seen. On a host two people share, each of them
    /// has a clone of their own, and the row records the first: identity comes from
    /// [`Project::remote_url`], so a second clone joins the project rather than making
    /// one. Every operation acts on the checkout the person is actually in
    /// (`lifecycle::ops::new::ensure_project`).
    pub root: PathBuf,
    /// Which repository this is, when it has a remote called `origin`.
    ///
    /// This is the project's identity where it has one, and the checkout path is the
    /// identity where it has none. A repository that has never been pushed is an
    /// ordinary project and carries `None`.
    pub remote_url: Option<RemoteUrl>,
    /// Display name; the handle a user sees in `nodal ls`.
    pub name: ProjectName,
    /// Digest of the effective recipe, so a changed `nodal.toml` is visible without
    /// re-reading the file.
    pub recipe_hash: Digest,
    /// When Nodal first saw this project.
    pub created_at: Timestamp,
}
