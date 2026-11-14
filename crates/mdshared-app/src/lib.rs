//! Application plumbing shared by the Markdown viewer and editor: path handling, reading files,
//! the recent-files list, the place left in each file, the key=value settings file layer,
//! one-window-per-user hand-over, the hand-off between the two applications, file watching, and
//! updating the application in place. No window system and no Markdown.

#[cfg(not(windows))]
pub mod appimage;
pub mod files;
pub mod fuzzy;
pub mod notes;
pub mod paths;
pub mod places;
pub mod recents;
pub mod selfupdate;
pub mod settings;
pub mod single;
pub mod sister;
pub mod update;
pub mod updater;
pub mod watch;
pub mod wikilink;
