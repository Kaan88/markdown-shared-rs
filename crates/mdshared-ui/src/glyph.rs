//! Segoe Fluent Icons code points used across the chrome, for `Painter::icon`. On other systems the same names map to Fluent UI System Icons (MIT), see assets/icons.

#[cfg(windows)]
pub const DOC: &str = "\u{E8A5}";
#[cfg(not(windows))]
pub const DOC: &str = "\u{F378}"; // document_20
#[cfg(windows)]
pub const FOLDER: &str = "\u{E8B7}";
#[cfg(not(windows))]
pub const FOLDER: &str = "\u{F418}"; // folder_20
#[cfg(windows)]
pub const FOLDER_OPEN: &str = "\u{E838}";
#[cfg(not(windows))]
pub const FOLDER_OPEN: &str = "\u{F42E}"; // folder_open_20
#[cfg(windows)]
pub const OPEN: &str = "\u{E8E5}";
#[cfg(not(windows))]
pub const OPEN: &str = "\u{E891}"; // open_folder_20
#[cfg(windows)]
pub const RELOAD: &str = "\u{E72C}";
#[cfg(not(windows))]
pub const RELOAD: &str = "\u{F190}"; // arrow_sync_20
#[cfg(windows)]
pub const RECENT: &str = "\u{E81C}";
#[cfg(not(windows))]
pub const RECENT: &str = "\u{F47E}"; // history_20
#[cfg(windows)]
pub const OUTLINE: &str = "\u{E8FD}";
#[cfg(not(windows))]
pub const OUTLINE: &str = "\u{ECD9}"; // text_bullet_list_ltr_20
#[cfg(windows)]
pub const MINUS: &str = "\u{E738}";
#[cfg(not(windows))]
pub const MINUS: &str = "\u{EBD0}"; // subtract_20
#[cfg(windows)]
pub const PLUS: &str = "\u{E710}";
#[cfg(not(windows))]
pub const PLUS: &str = "\u{F109}"; // add_20
#[cfg(windows)]
pub const UP: &str = "\u{E70E}";
#[cfg(not(windows))]
pub const UP: &str = "\u{F2B6}"; // chevron_up_20
#[cfg(windows)]
pub const DOWN: &str = "\u{E70D}";
#[cfg(not(windows))]
pub const DOWN: &str = "\u{F2A3}"; // chevron_down_20
#[cfg(windows)]
pub const RIGHT: &str = "\u{E76C}";
#[cfg(not(windows))]
pub const RIGHT: &str = "\u{F2B0}"; // chevron_right_20
#[cfg(windows)]
pub const SEARCH: &str = "\u{E721}";
#[cfg(not(windows))]
pub const SEARCH: &str = "\u{F68F}"; // search_20
#[cfg(windows)]
pub const CLOSE: &str = "\u{E711}";
#[cfg(not(windows))]
pub const CLOSE: &str = "\u{F369}"; // dismiss_20
#[cfg(windows)]
pub const MORE: &str = "\u{E712}";
#[cfg(not(windows))]
pub const MORE: &str = "\u{E824}"; // more_horizontal_20
#[cfg(windows)]
pub const SUN: &str = "\u{E706}";
#[cfg(not(windows))]
pub const SUN: &str = "\u{F8A1}"; // weather_sunny_20
#[cfg(windows)]
pub const MOON: &str = "\u{E708}";
#[cfg(not(windows))]
pub const MOON: &str = "\u{F87D}"; // weather_moon_20
#[cfg(windows)]
pub const SYSTEM: &str = "\u{E7F4}";
#[cfg(not(windows))]
pub const SYSTEM: &str = "\u{F359}"; // desktop_20
#[cfg(windows)]
pub const WARN: &str = "\u{E7BA}";
#[cfg(not(windows))]
pub const WARN: &str = "\u{F869}"; // warning_20
#[cfg(windows)]
pub const IMAGE: &str = "\u{E8B9}";
#[cfg(not(windows))]
pub const IMAGE: &str = "\u{F488}"; // image_20
#[cfg(windows)]
pub const COPY: &str = "\u{E8C8}";
#[cfg(not(windows))]
pub const COPY: &str = "\u{F32B}"; // copy_20
#[cfg(windows)]
pub const CUT: &str = "\u{E8C6}";
#[cfg(not(windows))]
pub const CUT: &str = "\u{F33A}"; // cut_20
#[cfg(windows)]
pub const PASTE: &str = "\u{E77F}";
#[cfg(not(windows))]
pub const PASTE: &str = "\u{F2D5}"; // clipboard_paste_20
#[cfg(windows)]
pub const LINK: &str = "\u{E71B}";
#[cfg(not(windows))]
pub const LINK: &str = "\u{F4E4}"; // link_20
#[cfg(windows)]
pub const SELECT_ALL: &str = "\u{E8B3}";
#[cfg(not(windows))]
pub const SELECT_ALL: &str = "\u{EA86}"; // select_all_on_20
#[cfg(windows)]
pub const FULLSCREEN: &str = "\u{E740}";
#[cfg(not(windows))]
pub const FULLSCREEN: &str = "\u{E684}"; // full_screen_maximize_20
#[cfg(windows)]
pub const BACK_TO_WINDOW: &str = "\u{E73F}";
#[cfg(not(windows))]
pub const BACK_TO_WINDOW: &str = "\u{E687}"; // full_screen_minimize_20
#[cfg(windows)]
pub const KEYBOARD: &str = "\u{E765}";
#[cfg(not(windows))]
pub const KEYBOARD: &str = "\u{F4B8}"; // keyboard_20
#[cfg(windows)]
pub const INFO: &str = "\u{E946}";
#[cfg(not(windows))]
pub const INFO: &str = "\u{F4A3}"; // info_20
#[cfg(windows)]
pub const POWER: &str = "\u{E7E8}";
#[cfg(not(windows))]
pub const POWER: &str = "\u{F60E}"; // power_20
#[cfg(windows)]
pub const ZOOM: &str = "\u{E71E}";
#[cfg(not(windows))]
pub const ZOOM: &str = "\u{F8C4}"; // zoom_in_20
#[cfg(windows)]
pub const DELETE: &str = "\u{E74D}";
#[cfg(not(windows))]
pub const DELETE: &str = "\u{F34C}"; // delete_20
#[cfg(windows)]
pub const OPEN_WITH: &str = "\u{E8A7}";
#[cfg(not(windows))]
pub const OPEN_WITH: &str = "\u{F8BD}"; // window_new_20
#[cfg(windows)]
pub const WIN_MIN: &str = "\u{E921}";
#[cfg(not(windows))]
pub const WIN_MIN: &str = "\u{EBD0}"; // subtract_20
#[cfg(windows)]
pub const WIN_MAX: &str = "\u{E922}";
#[cfg(not(windows))]
pub const WIN_MAX: &str = "\u{E7EB}"; // maximize_20
#[cfg(windows)]
pub const WIN_RESTORE: &str = "\u{E923}";
#[cfg(not(windows))]
pub const WIN_RESTORE: &str = "\u{EB96}"; // square_multiple_20
#[cfg(windows)]
pub const WIN_CLOSE: &str = "\u{E8BB}";
#[cfg(not(windows))]
pub const WIN_CLOSE: &str = "\u{F369}"; // dismiss_20
#[cfg(windows)]
pub const DOWNLOAD: &str = "\u{E896}";
#[cfg(not(windows))]
pub const DOWNLOAD: &str = "\u{F150}"; // arrow_download_20
