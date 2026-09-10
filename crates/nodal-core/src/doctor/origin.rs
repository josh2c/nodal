//! The URL of `origin`, reduced to a name two clones of one remote share.
//!
//! SSH and HTTPS forms of one host path are the same remote. A person cleaning a disk
//! groups by that, not by which scheme a clone happened to record.

/// The grouping name for an `origin` URL.
#[must_use]
pub fn normalize(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(path) = trimmed.strip_prefix("file://") {
        return strip_git(path).to_owned();
    }
    if let Some(rest) = scheme_rest(trimmed) {
        return host_path(rest);
    }
    if let Some(name) = scp(trimmed) {
        return name;
    }
    strip_git(trimmed).to_owned()
}

/// The rest of a URL after `scheme://`, when it has one.
fn scheme_rest(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    if !scheme.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(rest)
}

/// `user@host:path` as `host/path`.
fn scp(url: &str) -> Option<String> {
    let rest = url.rsplit_once('@').map_or(url, |(_, hostpath)| hostpath);
    let (host, path) = rest.split_once(':')?;
    if host.is_empty() || host.contains('/') || path.is_empty() {
        return None;
    }
    Some(format!("{}/{}", host.to_ascii_lowercase(), strip_git(path)))
}

/// `host[:port]/path` with userinfo stripped, as `host/path`.
fn host_path(rest: &str) -> String {
    let rest = rest.rsplit_once('@').map_or(rest, |(_, hostpath)| hostpath);
    let (hostport, path) = rest.split_once('/').map_or((rest, ""), |(host, path)| (host, path));
    let host = host_of(hostport);
    let path = strip_git(path);
    if path.is_empty() { host } else { format!("{host}/{path}") }
}

/// The host of `host` or `host:port`, with default ports dropped.
fn host_of(hostport: &str) -> String {
    let (host, port) = match hostport.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|ch| ch.is_ascii_digit()) => (host, Some(port)),
        _ => (hostport, None),
    };
    match port {
        Some("22" | "80" | "443") | None => host.to_ascii_lowercase(),
        Some(port) => format!("{}:{port}", host.to_ascii_lowercase()),
    }
}

/// Trailing slashes and a trailing `.git` taken off.
fn strip_git(value: &str) -> &str {
    let value = value.trim_end_matches('/');
    value.strip_suffix(".git").map_or(value, |stripped| stripped.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn ssh_and_https_forms_of_one_remote_are_the_same_name() {
        let names = [
            "git@github.com:josh2c/nodal.git",
            "ssh://git@github.com/josh2c/nodal.git",
            "https://github.com/josh2c/nodal.git",
            "https://github.com/josh2c/nodal",
            "https://GitHub.com/josh2c/nodal.git/",
        ];
        for url in names {
            assert_eq!(normalize(url), "github.com/josh2c/nodal", "{url}");
        }
    }

    #[test]
    fn a_local_path_remote_keeps_its_path() {
        assert_eq!(normalize("/tmp/remote.git"), "/tmp/remote");
        assert_eq!(normalize("file:///tmp/remote.git"), "/tmp/remote");
    }
}
