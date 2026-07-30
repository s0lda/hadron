pub mod tools;

use anyhow::{Context, Result};
use hadron_forge::file::{ExternalAccess, ExternalRoot};
use rmcp::serve_server;
use std::env;
use std::path::PathBuf;
use tools::ForgeMcpServer;

/// The stdio MCP server entrypoint, in the library rather than in a `[[bin]]`, so the
/// single installable package (`hadron`) can carry a bin target that calls it. The
/// daemon finds this server as a sibling of its own `current_exe` (`session.rs:494`),
/// so all three binaries must install into one directory.
///
/// The first positional argument is the project root the turn runs in; the second, when
/// given, overrides the nucleus directory. `--external-root <dir>` / `--external-root-rw
/// <dir>` may appear anywhere and each grants ONE directory outside the root — read-only
/// and read-write respectively. With none given the jail is exactly what it always was.
pub async fn run() -> Result<()> {
    let (positional, external) = parse_args(env::args().skip(1));

    let root_path = match positional.first() {
        Some(p) => PathBuf::from(p),
        None => env::current_dir().context("Failed to get current directory")?,
    };

    let server = match positional.get(1) {
        Some(nucleus) => ForgeMcpServer::with_nucleus(root_path, PathBuf::from(nucleus)),
        None => ForgeMcpServer::new(root_path),
    };
    let server = server.allowing_external(external);

    let running = serve_server(server, rmcp::transport::stdio()).await?;
    running.waiting().await?;
    Ok(())
}

/// Split argv into positionals and granted external roots.
///
/// A granted root that does not exist on disk is **reported and dropped**, never fatal:
/// a stale entry in `team.json` must not stop a quark's whole tool surface from booting,
/// and dropping it fails closed (the path simply stays unreachable).
fn parse_args(args: impl Iterator<Item = String>) -> (Vec<String>, Vec<ExternalRoot>) {
    let mut positional = Vec::new();
    let mut external = Vec::new();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let access = match arg.as_str() {
            "--external-root" => ExternalAccess::ReadOnly,
            "--external-root-rw" => ExternalAccess::ReadWrite,
            _ => {
                positional.push(arg);
                continue;
            }
        };
        let Some(path) = args.next() else {
            eprintln!("[forge] {arg} needs a directory — ignoring");
            continue;
        };
        match ExternalRoot::new(&path, access) {
            Ok(root) => external.push(root),
            Err(e) => eprintln!("[forge] ignoring external root {path:?}: {e}"),
        }
    }
    (positional, external)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flags_grants_nothing_and_keeps_both_positionals() {
        let (pos, ext) = parse_args(["/root".to_string(), "/nucleus".to_string()].into_iter());
        assert_eq!(pos, vec!["/root".to_string(), "/nucleus".to_string()]);
        assert!(ext.is_empty());
    }

    #[test]
    fn a_granted_root_is_parsed_with_its_access_and_positionals_survive() {
        let dir = tempfile::tempdir().unwrap();
        let rw = tempfile::tempdir().unwrap();
        let (pos, ext) = parse_args(
            [
                "/root".to_string(),
                "--external-root".to_string(),
                dir.path().display().to_string(),
                "--external-root-rw".to_string(),
                rw.path().display().to_string(),
            ]
            .into_iter(),
        );
        assert_eq!(pos, vec!["/root".to_string()]);
        assert_eq!(ext.len(), 2);
        assert_eq!(ext[0].access(), ExternalAccess::ReadOnly);
        assert_eq!(ext[1].access(), ExternalAccess::ReadWrite);
    }

    #[test]
    fn a_root_that_does_not_exist_is_dropped_not_fatal() {
        let (pos, ext) = parse_args(
            [
                "/root".to_string(),
                "--external-root".to_string(),
                "/no-such-dir-hadron".to_string(),
            ]
            .into_iter(),
        );
        assert_eq!(pos, vec!["/root".to_string()]);
        assert!(ext.is_empty(), "a missing root must fail closed, not open");
    }
}
