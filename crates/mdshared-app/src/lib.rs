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

#[cfg(not(windows))]
pub mod appimage;
pub mod backdrop;
pub mod brief;
pub mod crash;
pub mod dirlist;
pub mod dirwatch;
pub mod files;
pub mod fmt;
pub mod folder_lock;
pub mod foldersearch;
pub mod fuzzy;
pub mod html_md;
pub mod job;
pub mod library;
pub mod moves;
pub mod notes;
pub mod paths;
pub mod places;
pub mod pool;
pub mod recents;
pub mod relaunch;
pub mod runs;
pub mod selfupdate;
pub mod settings;
pub mod single;
#[cfg(windows)]
pub mod single_mutex;
pub mod sister;
pub mod split;
pub mod update;
pub mod updater;
pub mod watch;
pub mod wikilink;
