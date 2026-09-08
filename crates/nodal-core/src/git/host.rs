//! The web host a remote names, and the page it serves for opening a change.
//!
//! `nodal done` pushes a branch and then prints where a person opens the change. It
//! never opens one itself (`docs/contracts.md`), so what it needs of a remote URL is
//! one page a browser can be pointed at.
//!
//! The hosts are a table rather than a chain of conditions, and the table is short on
//! purpose: a host that is not in it gets no URL and says so. Guessing a compare path
//! for an unknown host would print a link that 404s, which is worse than printing
//! nothing, because a person cannot tell a wrong guess from a broken remote.
//!
//! Nothing here reaches the network. A remote URL is text the repository already holds.
//!
//! **Credentials never come out of here.** An HTTPS remote may carry a user name and a
//! token in front of the host, and this module drops that part before it builds
//! anything a report can print.

use std::fmt::Write as _;

/// The page each known host serves for opening a change from a branch, by host name.
///
/// `{base}` is the repository's web address and `{branch}` the branch, encoded. Exact
/// host names only: a self-hosted install answers on a name this table cannot know.
const PAGES: [(&str, &str); 3] = [
    ("github.com", "{base}/compare/{branch}?expand=1"),
    ("gitlab.com", "{base}/-/compare/{branch}"),
    ("bitbucket.org", "{base}/branch/{branch}"),
];

/// The bytes a branch may carry into a URL path unencoded.
const UNRESERVED: &[u8] = b"-._~/";

/// Where a remote points, as a browser would reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Web {
    /// The host the remote names, for example `github.com`.
    pub host: String,
    /// The repository's web address, with no credentials and no `.git`.
    pub base: String,
}

/// The web address a remote URL names, `None` for a path or a host with no name.
///
/// Both forms Git accepts are read: a URL with a scheme, and the `host:path` form an
/// SSH remote is usually written in.
#[must_use]
pub fn web(remote: &str) -> Option<Web> {
    let location = parse(remote)?;
    let path = location.path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if location.host.is_empty() || path.is_empty() {
        return None;
    }
    Some(Web { host: location.host.clone(), base: format!("https://{}/{path}", location.host) })
}

/// The page a person opens the change on, `None` when the host is not one this build
/// knows a compare page for.
#[must_use]
pub fn compare(remote: &str, branch: &str) -> Option<String> {
    let web = web(remote)?;
    let (_, template) = PAGES.iter().find(|(host, _)| *host == web.host)?;
    Some(template.replace("{base}", &web.base).replace("{branch}", &encoded(branch)))
}

/// A remote URL split into the two parts a web address is built from.
struct Location {
    /// The host, without credentials or port.
    host: String,
    /// Everything after the host.
    path: String,
}

/// Read a remote URL in either of the two forms Git accepts.
fn parse(remote: &str) -> Option<Location> {
    match remote.split_once("://") {
        Some((scheme, rest)) => authority(scheme, rest),
        None => scp(remote),
    }
}

/// `scheme://[user[:secret]@]host[:port]/path`, for every scheme but `file`.
fn authority(scheme: &str, rest: &str) -> Option<Location> {
    if scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    Some(Location { host: hostname(authority)?, path: path.to_owned() })
}

/// `[user@]host:path`, the form an SSH remote is usually written in.
///
/// A Windows drive letter and a relative path both look like this, so the part before
/// the colon must be a host: it may not be empty, hold a `/`, or be one character.
fn scp(remote: &str) -> Option<Location> {
    let (authority, path) = remote.split_once(':')?;
    if authority.contains('/') || authority.len() < 2 {
        return None;
    }
    Some(Location { host: hostname(authority)?, path: path.to_owned() })
}

/// The host out of an authority: credentials dropped, port dropped, lower-cased.
fn hostname(authority: &str) -> Option<String> {
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = host.split_once(':').map_or(host, |(host, _)| host);
    if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
}

/// A branch as one path segment sequence of a URL.
///
/// A branch name may hold `#`, `%` or `&`, each of which means something else in a URL,
/// so everything but the unreserved set is percent-encoded. `/` is kept, because a
/// branch's slashes are path separators on every host in the table.
fn encoded(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for byte in branch.bytes() {
        if byte.is_ascii_alphanumeric() || UNRESERVED.contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "tests fail by panicking")]

    use super::{compare, encoded, web};

    #[test]
    fn both_forms_of_a_github_remote_name_one_repository() {
        let expected = "https://github.com/team/project";
        for remote in [
            "https://github.com/team/project.git",
            "git@github.com:team/project.git",
            "ssh://git@github.com/team/project",
            "https://github.com/team/project/",
        ] {
            assert_eq!(web(remote).map(|found| found.base), Some(expected.to_owned()), "{remote}");
        }
    }

    #[test]
    fn a_token_in_a_remote_url_never_reaches_the_page_that_is_printed() {
        let remote = "https://someone:a-token-nobody-should-see@github.com/team/project.git";
        let page = compare(remote, "nodal/fix").expect("github is a host with a compare page");
        assert!(!page.contains("a-token-nobody-should-see"), "{page}");
        assert_eq!(page, "https://github.com/team/project/compare/nodal/fix?expand=1");
    }

    #[test]
    fn each_host_serves_its_own_page_and_an_unknown_one_serves_none() {
        assert_eq!(
            compare("git@gitlab.com:team/app.git", "topic"),
            Some(String::from("https://gitlab.com/team/app/-/compare/topic"))
        );
        assert_eq!(
            compare("git@bitbucket.org:team/app.git", "topic"),
            Some(String::from("https://bitbucket.org/team/app/branch/topic"))
        );
        assert_eq!(compare("git@git.example.invalid:team/app.git", "topic"), None);
    }

    #[test]
    fn a_remote_that_is_a_path_on_this_machine_has_no_web_address() {
        for remote in ["/srv/git/app.git", "../mirror", "file:///srv/git/app.git", "C:/repos/app"] {
            assert_eq!(web(remote), None, "{remote}");
        }
    }

    #[test]
    fn what_a_branch_may_hold_and_a_url_may_not_is_encoded() {
        assert_eq!(encoded("nodal/fix-worker-import"), "nodal/fix-worker-import");
        assert_eq!(encoded("fix#42"), "fix%2342");
    }
}
