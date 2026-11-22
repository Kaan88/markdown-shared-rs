//! Application plumbing shared by the Markdown viewer and editor: path handling, reading files,
//! creating a new file without replacing one, the recent-files list, the place left in each
//! file, the key=value settings file layer, one-window-per-user hand-over, the hand-off between
//! the two applications, file watching, a worker thread's handle, and updating the application
//! in place, crash reports, and the cut of a document into notes at its headings (`split`). No
//! window system and no Markdown parser.

#[cfg(not(windows))]
pub mod appimage;
pub mod backdrop;
pub mod brief;
pub mod crash;
pub mod files;
pub mod folder_lock;
pub mod foldersearch;
pub mod fuzzy;
pub mod job;
pub mod library;
pub mod moves;
pub mod notes;
pub mod paths;
pub mod places;
pub mod recents;
pub mod relaunch;
pub mod runs;
pub mod selfupdate;
pub mod settings;
pub mod single;
pub mod sister;
pub mod split;
pub mod update;
pub mod updater;
pub mod watch;
pub mod wikilink;
