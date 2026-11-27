//! Application plumbing shared by the Markdown viewer and editor: path handling, reading files,
//! creating a new file without replacing one, the recent-files list, the place left in each
//! file, the key=value settings file layer, one-window-per-user hand-over (through a loopback port,
//! or on Windows through a named mutex and `WM_COPYDATA`), the hand-off between
//! the two applications, file watching, a worker thread's handle and a pool of worker threads
//! (`pool`), a folder's entries as a file manager's list (`dirlist`) and the watcher that says
//! what changed in one (`dirwatch`), the small texts of sizes,
//! counts, times and dates (`fmt`), updating the application
//! in place, crash reports, and the cut of a document into notes at its headings (`split`). No
//! window system and no Markdown parser.
//!
//! The default `full` feature keeps all application plumbing available. Without default features,
//! only `notes`, `wikilink` and their `files` and `paths` helpers are built.

#[cfg(all(feature = "full", not(windows)))]
pub mod appimage;
#[cfg(all(test, feature = "full", windows))]
mod appimage;
#[cfg(feature = "full")]
pub mod backdrop;
#[cfg(feature = "full")]
pub mod brief;
#[cfg(feature = "full")]
pub mod crash;
#[cfg(feature = "full")]
pub mod dirlist;
#[cfg(feature = "full")]
pub mod dirwatch;
pub mod files;
#[cfg(feature = "full")]
pub mod fmt;
#[cfg(feature = "full")]
pub mod folder_lock;
#[cfg(feature = "full")]
pub mod foldersearch;
#[cfg(feature = "full")]
pub mod fuzzy;
#[cfg(feature = "full")]
pub mod html_md;
#[cfg(feature = "full")]
pub mod job;
#[cfg(feature = "full")]
pub mod library;
#[cfg(feature = "full")]
pub mod moves;
pub mod notes;
pub mod paths;
#[cfg(feature = "full")]
pub mod places;
#[cfg(feature = "full")]
pub mod pool;
#[cfg(feature = "full")]
pub mod recents;
#[cfg(feature = "full")]
pub mod relaunch;
#[cfg(feature = "full")]
pub mod runs;
#[cfg(feature = "full")]
pub mod selfupdate;
#[cfg(feature = "full")]
pub mod settings;
#[cfg(feature = "full")]
pub mod single;
#[cfg(all(feature = "full", windows))]
pub mod single_mutex;
#[cfg(feature = "full")]
pub mod sister;
#[cfg(feature = "full")]
pub mod split;
#[cfg(feature = "full")]
pub mod update;
#[cfg(feature = "full")]
pub mod updater;
#[cfg(feature = "full")]
pub mod watch;
pub mod wikilink;
