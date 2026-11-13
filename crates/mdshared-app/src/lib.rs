//! Application plumbing shared by the Markdown viewer and editor: path handling, reading files,
//! the recent-files list, the key=value settings file layer, one-window-per-user hand-over,
//! file watching, and updating the application in place. No window system and no Markdown.

pub mod files;
pub mod notes;
pub mod paths;
pub mod recents;
pub mod selfupdate;
pub mod settings;
pub mod single;
pub mod update;
pub mod updater;
pub mod watch;
pub mod wikilink;
