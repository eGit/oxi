//! The app as a client of an oxi server on another machine, reached over SSH (like VS Code's
//! Remote SSH): the server runs the agents in its folders and keeps the chats; the app shows
//! and drives them. See [`protocol`] for what goes over the wire on top of `oxi headless`.
//!
//! A remote workspace's root is `ssh://<host><absolute path>`, and its chats' session files
//! are `ssh://<host><path of the file on the host>`. Neither resolves to anything on this
//! machine, so code that treats them as local paths finds nothing instead of the wrong files.

pub mod client;
pub mod protocol;
pub mod service;

const SCHEME: &str = "ssh://";

/// `(host, path on the host)` of a remote workspace root or chat file.
pub fn split(location: &str) -> Option<(&str, &str)> {
    // `Path` keeps the double slash, but code that rebuilds a path from its components
    // collapses it.
    let rest = location
        .strip_prefix(SCHEME)
        .or_else(|| location.strip_prefix("ssh:/"))?;
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    (!host.is_empty()).then_some((host, path))
}

pub fn is_remote(location: &str) -> bool {
    split(location).is_some()
}

pub fn host_of(location: &str) -> Option<&str> {
    split(location).map(|(host, _)| host)
}

/// The path on the host; a local location is returned as is.
pub fn path_of(location: &str) -> &str {
    split(location).map_or(location, |(_, path)| path)
}

pub fn location(host: &str, path: &str) -> String {
    if path.starts_with('/') {
        format!("{SCHEME}{host}{path}")
    } else {
        format!("{SCHEME}{host}/{path}")
    }
}

/// Host aliases from `~/.ssh/config`, for the connect dialog.
pub fn ssh_config_hosts() -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(home.join(".ssh/config")) else {
        return Vec::new();
    };
    let mut hosts = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("Host ")
            .or_else(|| line.strip_prefix("Host\t"))
        else {
            continue;
        };
        for host in rest.split_whitespace() {
            if !host.contains(['*', '?', '!']) && !hosts.iter().any(|h| h == host) {
                hosts.push(host.to_string());
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_split_into_host_and_path() {
        let root = location("mini", "/Users/me/p");
        assert_eq!(root, "ssh://mini/Users/me/p");
        assert_eq!(split(&root), Some(("mini", "/Users/me/p")));
        assert_eq!(path_of(&root), "/Users/me/p");
        assert_eq!(path_of("/local/p"), "/local/p");
        assert!(!is_remote("/local/p"));
        assert_eq!(split("ssh:///x"), None);
        assert_eq!(split("ssh:/mini/a/b"), Some(("mini", "/a/b")));
        let joined = std::path::Path::new(&root).join("src");
        assert_eq!(
            split(joined.to_str().unwrap()),
            Some(("mini", "/Users/me/p/src"))
        );
        assert_eq!(split("ssh://hostonly"), None);
    }
}
