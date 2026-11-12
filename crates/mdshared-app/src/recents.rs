//! The recent-files list.

use crate::paths::same_path;
use std::path::{Path, PathBuf};

pub const MAX_RECENT: usize = 15;

/// Most recent first, without duplicates, at most `MAX_RECENT`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recents {
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// Menu text: the filename, marked when the file is gone.
    pub label: String,
    pub missing: bool,
}

impl Recents {
    pub fn add(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.paths.retain(|p| !same_path(p, &path));
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, path: &Path) {
        self.paths.retain(|p| !same_path(p, path));
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Checks the disk for each entry, so call it when a menu or dialog opens, not on startup.
    pub fn entries(&self) -> Vec<RecentEntry> {
        self.paths
            .iter()
            .map(|p| {
                let missing = !p.exists();
                let name = p.file_name().map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                let label = if missing {
                    format!("{name} (missing)")
                } else {
                    name
                };
                RecentEntry {
                    path: p.clone(),
                    label,
                    missing,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recents_cap_order_and_dedupe() {
        let mut r = Recents::default();
        for i in 0..20 {
            r.add(format!("/f{i}.md"));
        }
        assert_eq!(r.paths().len(), MAX_RECENT);
        assert_eq!(r.paths()[0], Path::new("/f19.md"));
        r.add("/f10.md");
        assert_eq!(r.paths()[0], Path::new("/f10.md"));
        assert_eq!(r.paths().len(), MAX_RECENT);
        r.remove(Path::new("/f10.md"));
        assert_eq!(r.paths()[0], Path::new("/f19.md"));
        r.clear();
        assert!(r.is_empty());
    }

    #[test]
    fn recents_mark_missing_files() {
        let dir = std::env::temp_dir().join(format!("mdshared-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.md");
        std::fs::write(&real, "x").unwrap();
        let mut r = Recents::default();
        r.add(dir.join("gone.md"));
        r.add(&real);
        let e = r.entries();
        assert_eq!((e[0].label.as_str(), e[0].missing), ("real.md", false));
        assert_eq!(
            (e[1].label.as_str(), e[1].missing),
            ("gone.md (missing)", true)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
