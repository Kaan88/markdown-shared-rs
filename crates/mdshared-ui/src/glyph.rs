//! The icons of the chrome, for `Painter::icon`: our own drawings, the same on every system.
//!
//! Each icon is SVG path data on a 16 by 16 grid, drawn as strokes [`STROKE`] wide with round
//! ends and corners; nothing is filled. A dot is a circle small enough that its stroke closes
//! over the middle. Lines sit on half units, so at 16 px they land on whole pixels.

/// The side of the grid the icons are drawn on.
pub const GRID: f32 = 16.0;
/// The stroke width on that grid.
pub const STROKE: f32 = 1.25;

pub const DOC: &str =
    "M4 1.5h5L12.5 5v9a.5 .5 0 0 1-.5 .5H4a.5 .5 0 0 1-.5-.5V2a.5 .5 0 0 1 .5-.5zM9 1.5V5h3.5";
pub const FOLDER: &str =
    "M1.5 4a1 1 0 0 1 1-1h3.2l1.5 1.5h6.3a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z";
pub const FOLDER_OPEN: &str = "M1.5 11.5V4a1 1 0 0 1 1-1h3.2l1.5 1.5h5.3a1 1 0 0 1 1 1V7M3.6 7h11.2l-1.9 5.4a.9 .9 0 0 1-.85 .6H2.4a.9 .9 0 0 1-.85-1.2z";
pub const OPEN: &str = "M8 14.5H4a.5 .5 0 0 1-.5-.5V2a.5 .5 0 0 1 .5-.5h5L12.5 5v2.5M9 1.5V5h3.5M10 14.5l4.5-4.5M11.3 10h3.2v3.2";
pub const RELOAD: &str = "M11.9 4.1A5.5 5.5 0 1 0 13.5 8M12.5 1.5v3h-3";
pub const RECENT: &str = "M4.1 4.1A5.5 5.5 0 1 1 2.5 8M3.5 1.5v3h3M8 5v3.3l2.2 1.3";
pub const OUTLINE: &str = "M5.5 4h8M5.5 8h8M5.5 12h8M2.275 4a.325 .325 0 1 0 .65 0a.325 .325 0 1 0-.65 0zM2.275 8a.325 .325 0 1 0 .65 0a.325 .325 0 1 0-.65 0zM2.275 12a.325 .325 0 1 0 .65 0a.325 .325 0 1 0-.65 0z";
pub const MINUS: &str = "M3 8h10";
pub const PLUS: &str = "M3 8h10M8 3v10";
pub const UP: &str = "M3.5 10.2L8 5.7l4.5 4.5";
pub const DOWN: &str = "M3.5 5.8L8 10.3l4.5-4.5";
pub const RIGHT: &str = "M5.8 3.5L10.3 8l-4.5 4.5";
pub const SEARCH: &str = "M2.5 7a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0-9 0zM10.3 10.3L14 14";
pub const CLOSE: &str = "M3.5 3.5l9 9M12.5 3.5l-9 9";
pub const MORE: &str = "M3.075 8a.425 .425 0 1 0 .85 0a.425 .425 0 1 0-.85 0zM7.575 8a.425 .425 0 1 0 .85 0a.425 .425 0 1 0-.85 0zM12.075 8a.425 .425 0 1 0 .85 0a.425 .425 0 1 0-.85 0z";
pub const SUN: &str = "M5.25 8a2.75 2.75 0 1 0 5.5 0a2.75 2.75 0 1 0-5.5 0zM8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.15 1.15M11.45 11.45l1.15 1.15M3.4 12.6l1.15-1.15M11.45 4.55l1.15-1.15";
pub const MOON: &str = "M13.6 9.6A6 6 0 1 1 6.4 2.4a4.7 4.7 0 0 0 7.2 7.2z";
pub const SYSTEM: &str = "M2 2.5h12a.5 .5 0 0 1 .5 .5v7.5a.5 .5 0 0 1-.5 .5H2a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM5.5 14h5M8 11v3";
pub const WARN: &str = "M8 2.2l6.2 10.9a.6 .6 0 0 1-.5 .9H2.3a.6 .6 0 0 1-.5-.9zM8 6.4v3.4M7.825 11.7a.175 .175 0 1 0 .35 0a.175 .175 0 1 0-.35 0z";
pub const IMAGE: &str = "M2.5 2.5h11a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1zM1.8 11.6l3.4-3.4 2.9 2.9 2-2 4.1 4.1M9.5 5.7a1.1 1.1 0 1 0 2.2 0a1.1 1.1 0 1 0-2.2 0z";
pub const COPY: &str = "M6 5.5h7.5a.5 .5 0 0 1 .5 .5v8a.5 .5 0 0 1-.5 .5H6a.5 .5 0 0 1-.5-.5V6a.5 .5 0 0 1 .5-.5zM3.5 10.5H2.5a.5 .5 0 0 1-.5-.5V2a.5 .5 0 0 1 .5-.5H10a.5 .5 0 0 1 .5 .5v1.5";
pub const CUT: &str = "M2.5 12a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0-3.8 0zM9.7 12a1.9 1.9 0 1 0 3.8 0a1.9 1.9 0 1 0-3.8 0zM5.5 10.4L12 1.8M10.5 10.4L4 1.8";
pub const PASTE: &str = "M5.5 3H4a.5 .5 0 0 0-.5 .5V14a.5 .5 0 0 0 .5 .5h8a.5 .5 0 0 0 .5-.5V3.5A.5 .5 0 0 0 12 3h-1.5M6 1.5h4a.5 .5 0 0 1 .5 .5v2.5h-5V2a.5 .5 0 0 1 .5-.5zM6 8.2h4M6 11h4";
pub const LINK: &str = "M8.6 4.3l.9-.9a2.5 2.5 0 0 1 3.5 3.5l-1.9 1.9a2.5 2.5 0 0 1-3.5 0M7.4 11.7l-.9 .9a2.5 2.5 0 0 1-3.5-3.5l1.9-1.9a2.5 2.5 0 0 1 3.5 0";
pub const SELECT_ALL: &str = "M2 4.2V3a1 1 0 0 1 1-1h1.2M6.9 2h2.2M11.8 2H13a1 1 0 0 1 1 1v1.2M14 6.9v2.2M14 11.8V13a1 1 0 0 1-1 1h-1.2M9.1 14H6.9M4.2 14H3a1 1 0 0 1-1-1v-1.2M2 9.1V6.9M5.5 5.5h5v5h-5z";
pub const FULLSCREEN: &str = "M2.5 6V2.5H6M10 2.5h3.5V6M13.5 10v3.5H10M6 13.5H2.5V10";
pub const BACK_TO_WINDOW: &str = "M6 2.5V6H2.5M13.5 6H10V2.5M10 13.5V10h3.5M2.5 10H6v3.5";
pub const KEYBOARD: &str = "M2.5 4h11a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM4.2 6.8h1M7.5 6.8h1M10.8 6.8h1M5 9.6h6";
pub const INFO: &str = "M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0zM8 7.3v3.9M7.825 4.9a.175 .175 0 1 0 .35 0a.175 .175 0 1 0-.35 0z";
pub const POWER: &str = "M8 1.5v6M4.6 3.7a5.5 5.5 0 1 0 6.8 0";
pub const ZOOM: &str = "M2.5 7a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0-9 0zM10.3 10.3L14 14M5 7h4M7 5v4";
pub const DELETE: &str = "M2.5 4h11M6 4V2.5a.5 .5 0 0 1 .5-.5h3a.5 .5 0 0 1 .5 .5V4M4 4l.6 9.4a.6 .6 0 0 0 .6 .6h5.6a.6 .6 0 0 0 .6-.6L12 4M6.7 6.6v4.8M9.3 6.6v4.8";
pub const OPEN_WITH: &str = "M7 3H3.5a.5 .5 0 0 0-.5 .5v9a.5 .5 0 0 0 .5 .5h9a.5 .5 0 0 0 .5-.5V9M9.6 2.5h3.9v3.9M13.5 2.5l-6 6";
pub const WIN_MIN: &str = "M3 8.5h10";
pub const WIN_MAX: &str = "M4 3h8a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z";
pub const WIN_RESTORE: &str = "M3.5 5.5h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-6a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1zM5.5 3.5v-.5a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-.5";
pub const WIN_CLOSE: &str = "M3 3l10 10M13 3L3 13";
pub const DOWNLOAD: &str = "M8 2v8.5M4.5 7L8 10.5 11.5 7M2.5 13.5h11";

/// Every icon with its name, for the tests and the specimen sheet.
pub const ALL: &[(&str, &str)] = &[
    ("DOC", DOC),
    ("FOLDER", FOLDER),
    ("FOLDER_OPEN", FOLDER_OPEN),
    ("OPEN", OPEN),
    ("RELOAD", RELOAD),
    ("RECENT", RECENT),
    ("OUTLINE", OUTLINE),
    ("MINUS", MINUS),
    ("PLUS", PLUS),
    ("UP", UP),
    ("DOWN", DOWN),
    ("RIGHT", RIGHT),
    ("SEARCH", SEARCH),
    ("CLOSE", CLOSE),
    ("MORE", MORE),
    ("SUN", SUN),
    ("MOON", MOON),
    ("SYSTEM", SYSTEM),
    ("WARN", WARN),
    ("IMAGE", IMAGE),
    ("COPY", COPY),
    ("CUT", CUT),
    ("PASTE", PASTE),
    ("LINK", LINK),
    ("SELECT_ALL", SELECT_ALL),
    ("FULLSCREEN", FULLSCREEN),
    ("BACK_TO_WINDOW", BACK_TO_WINDOW),
    ("KEYBOARD", KEYBOARD),
    ("INFO", INFO),
    ("POWER", POWER),
    ("ZOOM", ZOOM),
    ("DELETE", DELETE),
    ("OPEN_WITH", OPEN_WITH),
    ("WIN_MIN", WIN_MIN),
    ("WIN_MAX", WIN_MAX),
    ("WIN_RESTORE", WIN_RESTORE),
    ("WIN_CLOSE", WIN_CLOSE),
    ("DOWNLOAD", DOWNLOAD),
];
