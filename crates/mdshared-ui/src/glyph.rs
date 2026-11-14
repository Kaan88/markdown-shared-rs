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

pub const CHECK: &str = "M3 8.5l3.2 3.2L13 5";
pub const NEW_FILE: &str =
    "M8 14.5H4a.5 .5 0 0 1-.5-.5V2a.5 .5 0 0 1 .5-.5h5L12.5 5v3M9 1.5V5h3.5M12 10.5v4M10 12.5h4";
pub const SAVE: &str = "M3 2.5h8.3l2.2 2.2V13a.5 .5 0 0 1-.5 .5H3a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM5 2.5V6h5V2.5M5 13.5v-4h6v4";
pub const SAVE_AS: &str = "M7.5 13.5H3a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5h8.3l2.2 2.2V7M5 2.5V6h5V2.5M10 14.5l.4-1.9 3.2-3.2 1.5 1.5-3.2 3.2z";
pub const CLOSE_ALL: &str = "M2 5.5l5 5M7 5.5l-5 5M9 5.5l5 5M14 5.5l-5 5";
pub const CLOSE_OTHERS: &str = "M2.5 4l4 4M6.5 4l-4 4M9.5 3.5h4a.5 .5 0 0 1 .5 .5v8a.5 .5 0 0 1-.5 .5h-8a.5 .5 0 0 1-.5-.5v-1.5";
pub const REPLACE: &str = "M2.5 5.5h10M10 3l2.5 2.5L10 8M13.5 10.5h-10M6 8l-2.5 2.5L6 13";
pub const GO_TO_LINE: &str = "M2.5 3.5h11M2.5 12.5h11M2.5 8h7.5M7.5 5.5L10 8l-2.5 2.5";
pub const VIEW: &str = "M1.5 8C3.3 5 5.5 3.5 8 3.5S12.7 5 14.5 8c-1.8 3-4 4.5-6.5 4.5S3.3 11 1.5 8zM6 8a2 2 0 1 0 4 0a2 2 0 1 0-4 0z";
pub const MACRO: &str = "M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0zM6.7 5.6l3.8 2.4-3.8 2.4z";
pub const RECORD: &str = "M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0zM6.425 8a1.575 1.575 0 1 0 3.15 0a1.575 1.575 0 1 0-3.15 0z";
pub const STOP: &str = "M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0zM6 6h4v4H6z";
pub const PLAY: &str = "M4.5 3l8.5 5-8.5 5z";
pub const PLAY_END: &str = "M3 3.5l7 4.5-7 4.5zM13 3.5v9";
pub const UNDO: &str = "M3 6.5h6.5a3.5 3.5 0 0 1 0 7H6M5.5 4L3 6.5 5.5 9";
pub const REDO: &str = "M13 6.5H6.5a3.5 3.5 0 0 0 0 7H10M10.5 4L13 6.5 10.5 9";
pub const DUPLICATE_LINE: &str = "M2.5 4.5h9M2.5 8h5M11.5 8.5v5M9 11h5";
pub const DELETE_LINE: &str = "M2.5 4.5h11M2.5 8h5M9.5 9l4 4M13.5 9l-4 4";
pub const COMMENT: &str = "M7 3L4 13M12 3L9 13";
pub const UPPERCASE: &str = "M1.5 12.5l3-9 3 9M2.6 9.5h3.8M12 12.5v-9M9.8 5.7L12 3.5l2.2 2.2";
pub const LOWERCASE: &str = "M6.8 12.5V8.2c0-1.2-.8-2-2.2-2-1 0-1.8.4-2.4 1.1M6.8 9.6H4.6c-1.2 0-2 .6-2 1.5s.7 1.5 1.8 1.5c1 0 1.9-.5 2.4-1.400M12 3.5v9M9.8 10.300L12 12.5l2.2-2.2";
pub const TRIM: &str = "M2.5 4.5h6M2.5 8h3.5M2.5 11.5h6M14 8H9.5M11.5 6l-2 2 2 2";
pub const TABS: &str = "M2.5 8h8M8 5.5L10.5 8 8 10.5M13.5 4.5v7";
pub const BOOKMARK: &str = "M4.5 2.5h7a.5 .5 0 0 1 .5 .5v10.5l-4-3-4 3V3a.5 .5 0 0 1 .5-.5z";
pub const WRAP: &str = "M2.5 4h11M2.5 8H11a2 2 0 0 1 0 4H8.5M10 10.5L8.5 12l1.5 1.5M2.5 12h3";
pub const WHITESPACE: &str = "M11.5 2.5H7a3 3 0 0 0 0 6h1.5M8.5 2.5v11M11.5 2.5v11";
pub const MINIMAP: &str = "M2 2.5h12a.5 .5 0 0 1 .5 .5v10a.5 .5 0 0 1-.5 .5H2a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM10.5 2.5v11M12 5h1M12 7h1M12 9h1";
pub const SPLIT: &str = "M2 2.5h12a.5 .5 0 0 1 .5 .5v10a.5 .5 0 0 1-.5 .5H2a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM8 2.5v11";
pub const OTHER_PANE: &str = "M2 2.5h12a.5 .5 0 0 1 .5 .5v10a.5 .5 0 0 1-.5 .5H2a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM8 2.5v11M10 8h3M11.8 6.800L13 8l-1.2 1.2";
pub const SIDEBAR: &str = "M2 2.5h12a.5 .5 0 0 1 .5 .5v10a.5 .5 0 0 1-.5 .5H2a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM5.5 2.5v11";
pub const FOLD: &str = "M4 3.5l4 3 4-3M4 12.5l4-3 4 3";
pub const FOLD_ALL: &str = "M4.5 2.5l3.5 2.7 3.5-2.700M4.5 13.5l3.5-2.7 3.5 2.700M2.5 8h11";
pub const UNFOLD: &str = "M4 6.5l4-3 4 3M4 9.5l4 3 4-3";
pub const FUNCTIONS: &str = "M5.5 2.5c-1.2 0-1.8 .600-1.8 1.800v1.900c0 .900-.5 1.5-1.4 1.8 .900 .300 1.4 .900 1.4 1.800v1.900c0 1.2 .600 1.8 1.8 1.800M10.5 2.5c1.2 0 1.8 .600 1.8 1.800v1.900c0 .900 .5 1.5 1.4 1.8-.900 .300-1.4 .900-1.4 1.800v1.900c0 1.2-.600 1.8-1.8 1.8";
pub const ZOOM_OUT: &str = "M2.5 7a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0-9 0zM10.3 10.3L14 14M5 7h4";
pub const ZOOM_RESET: &str =
    "M2.5 7a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0-9 0zM10.3 10.3L14 14M5.3 6h3.400M5.3 8h3.4";
pub const PAGE_NARROW: &str = "M5.5 2.5h5v11h-5zM7.3 6h1.400M7.3 8.5h1.4";
pub const PAGE_WIDE: &str = "M3.5 2.5h9v11h-9zM5.5 6h5M5.5 8.5h5";
pub const PAGE_FULL: &str = "M1.5 2.5h13v11h-13zM4 6h8M4 8.5h8";
pub const GLOBE: &str = "M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0zM2 8h12M8 2C6 3.6 5 5.6 5 8s1 4.4 3 6c2-1.6 3-3.6 3-6s-1-4.4-3-6z";
pub const TASK: &str = "M3 2.5h10a.5 .5 0 0 1 .5 .5v10a.5 .5 0 0 1-.5 .5H3a.5 .5 0 0 1-.5-.5V3a.5 .5 0 0 1 .5-.5zM5 8.200l2.2 2.200L11 6";
pub const ANIMATION: &str = "M2.5 3.5h11a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1zM6.7 5.900l3.6 2.1-3.6 2.100z";
pub const SLIDERS: &str = "M2.5 4.5h2M8 4.5h5.5M2.5 11.5h5.5M11.5 11.5h2M4.75 4.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0zM8.25 11.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0z";
pub const FIND_TABS: &str =
    "M2 4h5M2 8h3M2 12h5M7.75 8.5a2.75 2.75 0 1 0 5.5 0a2.75 2.75 0 1 0-5.5 0zM12.5 10.5l2 2";
pub const CASE: &str = "M1.5 12l2.7-8 2.7 8M2.5 9.400h3.400M13.5 12V8.5c0-1-.700-1.7-1.9-1.7-.900 0-1.5 .300-2 .900M13.5 9.800h-1.900c-1 0-1.7 .5-1.7 1.200s.600 1.2 1.5 1.2 1.7-.400 2.1-1.1";
pub const WORD: &str =
    "M3.5 5.5v-2M12.5 5.5v-2M3.5 3.5h9M2.5 8.5h2M6 8.5h4M11.5 8.5h2M3.5 11v2M12.5 11v2M3.5 13h9";
pub const REGEX: &str = "M10.5 2.5v6M7.9 4l5.2 3M13.1 4L7.9 7M3.525 12a0.475 0.475 0 1 0 0.95 0a0.475 0.475 0 1 0-0.95 0z";
pub const EOL: &str = "M12.5 3v5a1.5 1.5 0 0 1-1.5 1.5H4M6.5 7L4 9.5 6.5 12";
pub const ENCODING: &str = "M4.5 3.5L2 8l2.5 4.5M11.5 3.5L14 8l-2.5 4.5M6.5 10.5l1.5-5 1.5 5M7 9h2";
pub const BACK: &str = "M13 8H3M7 4L3 8l4 4";
pub const FORWARD: &str = "M3 8h10M9 4l4 4-4 4";
pub const EDIT: &str = "M2.5 13.5l.700-3 7.5-7.5a1.3 1.3 0 0 1 1.8 0l.5 .5a1.3 1.3 0 0 1 0 1.800L5.5 12.800zM9.7 4l2.3 2.3";

pub const SORT: &str = "M4 3v10M2 11l2 2 2-2M8.5 4h3M8.5 8h4.5M8.5 12h6";
pub const SORT_DESC: &str = "M4 13V3M2 5l2-2 2 2M8.5 4h6M8.5 8h4.5M8.5 12h3";
pub const SORT_CASE: &str = "M4 3v10M2 11l2 2 2-2M8.5 12.5l2.7-8.5 2.7 8.5M9.5 9.5h3.4";
pub const REVERSE: &str = "M5 13V3M3 5l2-2 2 2M11 3v10M9 11l2 2 2-2";
pub const DEDUPE: &str = "M2.5 4h9M2.5 7.5h9M2.5 11.5h5M10.5 10l3 3M13.5 10l-3 3";
pub const JOIN: &str = "M2.5 4.5h5.5M2.5 11.5h5.5M12 4.5v2.5a1 1 0 0 1-1 1H7M9 6L7 8l2 2";

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
    ("CHECK", CHECK),
    ("NEW_FILE", NEW_FILE),
    ("SAVE", SAVE),
    ("SAVE_AS", SAVE_AS),
    ("CLOSE_ALL", CLOSE_ALL),
    ("CLOSE_OTHERS", CLOSE_OTHERS),
    ("REPLACE", REPLACE),
    ("GO_TO_LINE", GO_TO_LINE),
    ("VIEW", VIEW),
    ("MACRO", MACRO),
    ("RECORD", RECORD),
    ("STOP", STOP),
    ("PLAY", PLAY),
    ("PLAY_END", PLAY_END),
    ("UNDO", UNDO),
    ("REDO", REDO),
    ("DUPLICATE_LINE", DUPLICATE_LINE),
    ("DELETE_LINE", DELETE_LINE),
    ("COMMENT", COMMENT),
    ("UPPERCASE", UPPERCASE),
    ("LOWERCASE", LOWERCASE),
    ("TRIM", TRIM),
    ("TABS", TABS),
    ("BOOKMARK", BOOKMARK),
    ("WRAP", WRAP),
    ("WHITESPACE", WHITESPACE),
    ("MINIMAP", MINIMAP),
    ("SPLIT", SPLIT),
    ("OTHER_PANE", OTHER_PANE),
    ("SIDEBAR", SIDEBAR),
    ("FOLD", FOLD),
    ("FOLD_ALL", FOLD_ALL),
    ("UNFOLD", UNFOLD),
    ("FUNCTIONS", FUNCTIONS),
    ("ZOOM_OUT", ZOOM_OUT),
    ("ZOOM_RESET", ZOOM_RESET),
    ("PAGE_NARROW", PAGE_NARROW),
    ("PAGE_WIDE", PAGE_WIDE),
    ("PAGE_FULL", PAGE_FULL),
    ("GLOBE", GLOBE),
    ("TASK", TASK),
    ("ANIMATION", ANIMATION),
    ("SLIDERS", SLIDERS),
    ("FIND_TABS", FIND_TABS),
    ("CASE", CASE),
    ("WORD", WORD),
    ("REGEX", REGEX),
    ("EOL", EOL),
    ("ENCODING", ENCODING),
    ("BACK", BACK),
    ("FORWARD", FORWARD),
    ("EDIT", EDIT),
    ("SORT", SORT),
    ("SORT_DESC", SORT_DESC),
    ("SORT_CASE", SORT_CASE),
    ("REVERSE", REVERSE),
    ("DEDUPE", DEDUPE),
    ("JOIN", JOIN),
];
