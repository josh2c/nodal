//! Whether a unit's commits exist anywhere but this machine, and which repository a
//! checkout is a clone of.
//!
//! `nodal reclaim` refuses to remove work that exists only on this machine; [`containment`]
//! is the Git half of that answer.
//!
//! [`identity`] is the other question this module answers, and it is asked of a string
//! rather than of a repository. Two engineers clone one repository into two directories
//! and Nodal has to see one project, so the two remotes have to reduce to one spelling.
//! Nothing here reaches a network: the URL comes from `git remote get-url`, which reads
//! the repository's own configuration file.

use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::Result;

/// How much of a revision the remotes already have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Containment {
    /// The remotes configured in this repository.
    pub remotes: Vec<String>,
    /// Commits reachable from the revision and from no remote-tracking ref, newest first.
    pub unpushed: Vec<Oid>,
}

impl Containment {
    /// Whether every commit of the revision is on at least one remote.
    #[must_use]
    pub fn is_contained(&self) -> bool {
        self.unpushed.is_empty()
    }
}

/// Compute containment for `rev` against every remote-tracking ref.
///
/// # Errors
/// [`Error::Git`](crate::Error::Git) when the revision is unknown, [`Error::GitOid`](crate::Error::GitOid)
/// on unreadable output.
pub(super) fn containment(repo: &Path, rev: &str) -> Result<Containment> {
    let remotes = names(repo)?;
    let listed = cmd::run_ok(repo, &["rev-list", rev, "--not", "--remotes"])?;
    let unpushed = listed.lines()?.iter().map(|line| Oid::parse(line)).collect::<Result<_>>()?;
    Ok(Containment { remotes, unpushed })
}

/// Every remote this repository names, in the order `git remote` lists them.
///
/// # Errors
/// [`Error::Git`](crate::Error::Git) when `git remote` failed.
pub(super) fn names(repo: &Path) -> Result<Vec<String>> {
    Ok(cmd::run_ok(repo, &["remote"])?.lines()?.iter().map(|name| (*name).to_owned()).collect())
}

/// The URL a remote fetches from, `None` when the repository has no such remote.
///
/// # Errors
/// [`Error::GitSpawn`](crate::Error::GitSpawn) when `git` could not be started,
/// [`Error::GitEncoding`](crate::Error::GitEncoding) when the URL is not UTF-8.
pub(super) fn url(repo: &Path, name: &str) -> Result<Option<String>> {
    let output = cmd::run(repo, &["remote", "get-url", "--", name])?;
    if !output.ok() {
        return Ok(None);
    }
    Ok(Some(output.text()?.to_owned()))
}

/// The port a URL may carry, kept because two servers can share a host name.
const PORT: char = ':';

/// What repository a remote URL names, in the spelling every clone of it shares.
///
/// The forms Git accepts are three, and all three reduce to `host/path`:
///
/// | typed | reduced to |
/// |---|---|
/// | `https://github.com/josh2c/nodal.git` | `github.com/josh2c/nodal` |
/// | `git@github.com:josh2c/nodal.git` | `github.com/josh2c/nodal` |
/// | `ssh://git@github.com:22/josh2c/nodal` | `github.com:22/josh2c/nodal` |
///
/// Four things are dropped and one is kept. The scheme goes, because it is how a person
/// reaches the repository and not which repository it is. The user before the host goes,
/// for the same reason: `git@` and `josh@` are two accounts on one server. A trailing
/// `.git` goes, and so do trailing slashes. The port stays, because two servers can
/// answer on one host name and they are not one repository.
///
/// The host is lowercased and the path is not. Host names are case-insensitive by the
/// standard; a path's case is the server's business, and folding it would merge two
/// repositories on a host that keeps them apart.
///
/// A remote that names a directory rather than a server — a bare repository two people
/// share, which is exactly what a host like this often has — reduces to that directory's
/// path. `None` where there is nothing to reduce, which is what an empty remote is.
#[must_use]
pub fn identity(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (host, path) = split(trimmed);
    let path = tidy(path);
    if host.is_empty() {
        return (!path.is_empty()).then(|| format!("/{path}"));
    }
    Some(if path.is_empty() { host } else { format!("{host}/{path}") })
}

/// The host and the path of a remote, in whichever of Git's three spellings it is.
fn split(url: &str) -> (String, &str) {
    if let Some(rest) = url.split_once("://").map(|(_, rest)| rest) {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        return (host_of(authority), path);
    }
    if let Some((authority, path)) = scp_like(url) {
        return (host_of(authority), path);
    }
    (String::new(), url)
}

/// The `user@host:path` form, and `None` for anything that is a path with a colon in it.
///
/// A Windows drive letter and an absolute path that happens to hold a colon are the two
/// ways this form is mistaken for one. Git reads the form only when the colon comes
/// before the first slash, and so does this.
fn scp_like(url: &str) -> Option<(&str, &str)> {
    let colon = url.find(PORT)?;
    let slash = url.find('/');
    if slash.is_some_and(|slash| slash < colon) {
        return None;
    }
    Some((&url[..colon], &url[colon + 1..]))
}

/// The host in an authority, lowercased, with any user before it dropped.
fn host_of(authority: &str) -> String {
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    host.to_lowercase()
}

/// A path with its trailing slashes and its trailing `.git` taken off.
fn tidy(path: &str) -> String {
    let path = path.trim_matches('/');
    path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    use super::identity;

    /// The two spellings of one repository that two engineers on one host will type.
    /// This is the whole point of the function: they are one project or they are two.
    #[test]
    fn ssh_and_https_spellings_of_one_repository_reduce_to_one() {
        let over_ssh = identity("git@github.com:josh2c/nodal.git");
        let over_https = identity("https://github.com/josh2c/nodal");
        assert_eq!(over_ssh.as_deref(), Some("github.com/josh2c/nodal"));
        assert_eq!(over_ssh, over_https);
    }

    /// The account a person reaches the server through is not which repository it is.
    #[test]
    fn the_user_before_the_host_is_not_part_of_the_identity() {
        assert_eq!(identity("ssh://git@example.com/a/b"), identity("ssh://jo@example.com/a/b"));
        assert_eq!(identity("git@example.com:a/b"), identity("jo@example.com:a/b"));
    }

    /// Two servers can answer on one host name, and they are not one repository.
    #[test]
    fn a_port_is_kept() {
        assert_eq!(
            identity("ssh://git@example.com:2222/a/b").as_deref(),
            Some("example.com:2222/a/b")
        );
        assert_ne!(identity("ssh://example.com:2222/a/b"), identity("ssh://example.com/a/b"));
    }

    /// Host names are case-insensitive and paths are not.
    #[test]
    fn the_host_folds_and_the_path_does_not() {
        assert_eq!(
            identity("https://GitHub.COM/josh2c/nodal").as_deref(),
            Some("github.com/josh2c/nodal")
        );
        assert_ne!(
            identity("https://github.com/Josh2c/Nodal"),
            identity("https://github.com/josh2c/nodal")
        );
    }

    /// A bare repository in a directory two people share is a remote like any other.
    #[test]
    fn a_local_directory_is_its_own_identity() {
        assert_eq!(identity("/srv/git/nodal.git").as_deref(), Some("/srv/git/nodal"));
        assert_eq!(identity("file:///srv/git/nodal.git").as_deref(), Some("/srv/git/nodal"));
    }

    /// An absolute path is not a `user@host:path`, whatever colons it holds.
    #[test]
    fn a_path_with_a_colon_in_it_is_still_a_path() {
        assert_eq!(identity("/srv/git/odd:name.git").as_deref(), Some("/srv/git/odd:name"));
    }

    /// Nothing to reduce is nothing, and never an empty identity two rows could share.
    #[test]
    fn an_empty_remote_has_no_identity() {
        assert_eq!(identity(""), None);
        assert_eq!(identity("   "), None);
    }
}
