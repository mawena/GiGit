//! Thin wrapper around the `git` command line.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("impossible de lancer git : {0}")]
    Spawn(#[from] std::io::Error),
    #[error("{}", first_line(stderr))]
    Failed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
}

fn first_line(s: &str) -> &str {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("git a échoué")
}

#[derive(Debug, Clone, Default)]
pub struct RunOpts {
    pub background: bool,
    pub read_only: bool,
    pub stdin: Option<Vec<u8>>,
}

fn build_command(repo: &Path, args: &[&str], opts: &RunOpts) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo)
        .args(["-c", "core.quotepath=off", "-c", "color.ui=never"])
        .args(args)
        // No askpass bridge yet: a credential prompt must fail fast, never hang.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if opts.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if opts.read_only {
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
    }
    // Detach from any controlling terminal so ssh cannot prompt on a tty.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd
}

pub fn run(repo: &Path, args: &[&str], opts: &RunOpts) -> Result<GitOutput, GitError> {
    let mut child = build_command(repo, args, opts).spawn()?;
    if let Some(data) = &opts.stdin {
        let mut stdin = child.stdin.take().expect("stdin piped");
        let data = data.clone();
        // Write on a thread so a large stdin cannot deadlock against a full stdout pipe.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let out = child.wait_with_output()?;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok(GitOutput { stdout: out.stdout, stderr })
    } else {
        Err(GitError::Failed {
            command: format!("git {}", args.join(" ")),
            code: out.status.code(),
            stderr,
        })
    }
}

pub fn run_str(repo: &Path, args: &[&str], opts: &RunOpts) -> Result<String, GitError> {
    run(repo, args, opts).map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    pub fn git(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(repo).output().unwrap();
        assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn temp_repo() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        git(&path, &["init", "-q", "-b", "main"]);
        git(&path, &["config", "user.name", "Test"]);
        git(&path, &["config", "user.email", "test@example.com"]);
        git(&path, &["config", "commit.gpgsign", "false"]);
        (dir, path)
    }

    pub fn write(repo: &Path, rel: &str, content: &str) {
        let p = repo.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, content).unwrap();
    }

    pub fn commit_all(repo: &Path, msg: &str) {
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", msg]);
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use std::ffi::OsStr;
    use std::time::{Duration, Instant};

    #[test]
    fn run_ok_returns_stdout() {
        let (_d, repo) = temp_repo();
        let out = run_str(&repo, &["rev-parse", "--is-inside-work-tree"], &RunOpts::default()).unwrap();
        assert_eq!(out, "true\n");
    }

    #[test]
    fn run_failure_is_failed_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = run(dir.path(), &["rev-parse", "HEAD"], &RunOpts::default()).unwrap_err();
        match err {
            GitError::Failed { command, code, stderr } => {
                assert_eq!(code, Some(128));
                assert!(stderr.starts_with("fatal:"), "{stderr}");
                assert_eq!(command, "git rev-parse HEAD");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    fn env_of(cmd: &Command, key: &str) -> Option<String> {
        cmd.get_envs()
            .find(|(k, _)| *k == OsStr::new(key))
            .and_then(|(_, v)| v.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn command_env_flags() {
        let p = Path::new("/");
        let plain = build_command(p, &["status"], &RunOpts::default());
        assert_eq!(env_of(&plain, "GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
        assert_eq!(env_of(&plain, "GIT_OPTIONAL_LOCKS"), None);
        let ro = build_command(p, &["status"], &RunOpts { read_only: true, ..Default::default() });
        assert_eq!(env_of(&ro, "GIT_OPTIONAL_LOCKS").as_deref(), Some("0"));
    }

    #[test]
    fn stdin_is_piped() {
        let (_d, repo) = temp_repo();
        let opts = RunOpts { stdin: Some(b"hello\n".to_vec()), ..Default::default() };
        let out = run_str(&repo, &["hash-object", "--stdin"], &opts).unwrap();
        assert_eq!(out.trim(), "ce013625030ba8dba906f756967f9e9ca394464a");
    }

    #[test]
    fn never_hangs_on_credentials() {
        let (_d, repo) = temp_repo();
        let start = Instant::now();
        let r = run(&repo, &["ls-remote", "https://127.0.0.1:9/x.git"], &RunOpts::default());
        assert!(matches!(r, Err(GitError::Failed { .. })));
        assert!(start.elapsed() < Duration::from_secs(10));
    }
}
pub mod status;
pub mod log;
