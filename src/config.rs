//! Persistent settings and the list of repositories.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{GitError, RunOpts, run, run_str};
pub use crate::i18n::LangChoice;
pub use crate::theme::ThemeChoice;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub repos: Vec<PathBuf>,
    pub last_open: Option<PathBuf>,
    pub language: LangChoice,
    pub theme: ThemeChoice,
    pub font_size: f32,
    /// 0 disables automatic fetching.
    pub autofetch_minutes: u32,
    /// Show authors' avatars (Gravatar / GitHub), as ungit does.
    pub avatars: bool,
    pub window: Option<[f32; 2]>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            repos: Vec::new(),
            last_open: None,
            language: LangChoice::default(),
            theme: ThemeChoice::default(),
            font_size: 14.0,
            autofetch_minutes: 5,
            avatars: true,
            window: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Config {
        std::fs::read_to_string(path).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    /// Writes atomically (temporary file + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    /// Adds a repository (canonicalised). Returns false if it was already listed.
    pub fn add_repo(&mut self, p: PathBuf) -> bool {
        let p = p.canonicalize().unwrap_or(p);
        if self.repos.contains(&p) {
            return false;
        }
        self.repos.push(p);
        true
    }

    /// Removes a repository from the list. Never touches the disk.
    pub fn remove_repo(&mut self, p: &Path) {
        self.repos.retain(|r| r != p);
        if self.last_open.as_deref() == Some(p) {
            self.last_open = None;
        }
    }

    pub fn move_repo(&mut self, from: usize, to: usize) {
        if from >= self.repos.len() {
            return;
        }
        let r = self.repos.remove(from);
        self.repos.insert(to.min(self.repos.len()), r);
    }
}

pub fn config_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "gigit")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("gigit.toml"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoProbe {
    Ok { root: PathBuf },
    NotARepo,
    Missing,
}

pub fn probe(p: &Path) -> RepoProbe {
    if !p.is_dir() {
        return RepoProbe::Missing;
    }
    let opts = RunOpts { background: true, read_only: true, ..Default::default() };
    match run_str(p, &["rev-parse", "--show-toplevel"], &opts) {
        Ok(root) => {
            let root = PathBuf::from(root.trim_end_matches('\n'));
            RepoProbe::Ok { root: root.canonicalize().unwrap_or(root) }
        }
        Err(_) => RepoProbe::NotARepo,
    }
}

pub fn init_repo(p: &Path) -> Result<(), GitError> {
    run(p, &["init", "-q"], &RunOpts::default()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;
    use tempfile::TempDir;

    #[test]
    fn default_values() {
        let c = Config::default();
        assert_eq!(c.font_size, 14.0);
        assert_eq!(c.autofetch_minutes, 5);
        assert_eq!(c.theme, ThemeChoice::Ardoise);
        assert!(c.avatars);
        assert_eq!(c.language, LangChoice::System);
        assert!(c.repos.is_empty());
    }

    #[test]
    fn roundtrip_toml() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("sub/config.toml");
        let c = Config {
            repos: vec!["/a".into(), "/b c".into()],
            last_open: Some("/b c".into()),
            language: LangChoice::Fr,
            theme: ThemeChoice::Nuit,
            font_size: 16.0,
            autofetch_minutes: 0,
            avatars: false,
            window: Some([1000.0, 700.0]),
        };
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path), c);
    }

    #[test]
    fn load_missing_or_invalid_file_gives_default() {
        let d = TempDir::new().unwrap();
        assert_eq!(Config::load(&d.path().join("none.toml")), Config::default());
        let bad = d.path().join("bad.toml");
        std::fs::write(&bad, "repos = 12 [[[").unwrap();
        assert_eq!(Config::load(&bad), Config::default());
    }

    #[test]
    fn load_partial_file_keeps_defaults() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("c.toml");
        std::fs::write(&p, "theme = \"clair\"\n").unwrap();
        let c = Config::load(&p);
        assert_eq!(c.theme, ThemeChoice::Clair);
        assert_eq!(c.font_size, 14.0);
    }

    #[test]
    fn add_repo_dedups_canonical() {
        let d = TempDir::new().unwrap();
        let repo = d.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let mut c = Config::default();
        assert!(c.add_repo(repo.clone()));
        assert!(!c.add_repo(d.path().join("./repo/../repo")));
        assert_eq!(c.repos, vec![repo.canonicalize().unwrap()]);
    }

    #[test]
    fn remove_repo_clears_last_open_and_keeps_disk() {
        let d = TempDir::new().unwrap();
        let repo = d.path().canonicalize().unwrap();
        let mut c = Config::default();
        c.add_repo(repo.clone());
        c.last_open = Some(repo.clone());
        c.remove_repo(&repo);
        assert!(c.repos.is_empty());
        assert_eq!(c.last_open, None);
        assert!(repo.exists());
    }

    #[test]
    fn move_repo_reorders() {
        let mut c = Config { repos: vec!["/a".into(), "/b".into(), "/c".into()], ..Default::default() };
        c.move_repo(0, 2);
        assert_eq!(c.repos, vec![PathBuf::from("/b"), "/c".into(), "/a".into()]);
        c.move_repo(2, 0);
        assert_eq!(c.repos, vec![PathBuf::from("/a"), "/b".into(), "/c".into()]);
        c.move_repo(5, 0); // out of range: ignored
        assert_eq!(c.repos.len(), 3);
    }

    #[test]
    fn probe_missing_dir() {
        assert_eq!(probe(Path::new("/definitely/not/here")), RepoProbe::Missing);
    }

    #[test]
    fn probe_not_a_repo() {
        let d = TempDir::new().unwrap();
        assert_eq!(probe(d.path()), RepoProbe::NotARepo);
    }

    #[test]
    fn probe_subdir_returns_root() {
        let (_d, repo) = temp_repo();
        std::fs::create_dir(repo.join("sub")).unwrap();
        let root = repo.canonicalize().unwrap();
        assert_eq!(probe(&repo.join("sub")), RepoProbe::Ok { root });
    }

    #[test]
    fn init_repo_makes_repo() {
        let d = TempDir::new().unwrap();
        init_repo(d.path()).unwrap();
        assert!(matches!(probe(d.path()), RepoProbe::Ok { .. }));
    }
}
