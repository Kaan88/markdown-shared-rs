//! Application plumbing shared by the Markdown viewer and editor: path handling, reading files,
//! the recent-files list, the key=value settings file layer, one-window-per-user hand-over and
//! file watching. No window system and no Markdown.

pub mod files;
pub mod paths;
pub mod recents;
pub mod settings;
pub mod single;
pub mod watch;
