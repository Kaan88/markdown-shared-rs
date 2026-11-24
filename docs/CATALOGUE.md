# Catalogue: what the crates hold

Every public module of the workspace, with what it is for and where to start reading. Look here
before writing a piece of an application: most of what a window needs exists. Each module's own
`//!` comment is the full account; this page is the map. **Add a line here with every module
you add** ([NEW-MODULE.md](NEW-MODULE.md)).

Conventions that hold throughout: logical pixels; the application owns the window loop, the
events and the hits; a model answers questions and takes events, a `draw_*` function draws and
changes nothing; a function that ends in `_animated` or `_presence` is the same drawing with
motion ([ANIMATION.md](ANIMATION.md)).

## Painting (`mdshared-ui`, `mdshared-text`)

| Module | For | Start at |
|---|---|---|
| `mdshared_text::bitmap` | The framebuffer every frame is drawn into: opaque BGRA bytes. PNG in and out | `Bitmap::new`, `clear`, `rrect`, `line`, `image`, `write_png`; `decode_png_rgba` |
| `mdshared_text::image` | Pictures from files: PNG and JPEG (zune-jpeg) into RGBA under limits a hostile file cannot pass, the EXIF turn applied, an area-average downscale (provisional) | `decode(bytes, Limits::DEFAULT) -> Result<Image, Error>`, `dimensions`, `sniff`, `Image::scaled_to_fit`, `scaled`, `pixel`; `encode_png`; `exif_orientation`, `orient` |
| `mdshared_ui::gfx` | The painter: boxes, frames, text, icons, clip, offset, group opacity. Everything else draws through it | `Gfx::new` (one per window), `Painter::new(bmp, gfx, scale)`; `fill`, `rbox`, `rbox_a`, `frame`, `shadow`, `polygon`, `blit`; `clip`/`restore`, `offset`/`restore_offset`, `snapshot`/`fade`; types `Rect`, `Rgb`, `Face`; `rgb(hex)`, `mix(a, b, t)` |
| `mdshared_ui::gfx` (text) | Interface text in three faces, measured and elided | `Painter::text`, `text_center`, `text_right`, `paragraph`, `measure`, `elide`, `caret_stops`, `underline_range`, `strikethrough_range` |
| `mdshared_ui::gfx` (shapes) | Pictures, paths, rings and arcs, waveform and meter columns, gradients | `Painter::image(rgba, w, h, dst, radius, alpha)`, `fill_path`, `stroke_path` with `gfx::Path` (`move_to`, `line_to`, `quad_to`, `curve_to`, `close`, `polygon`, `svg`), `ring`, `arc(cx, cy, r, width, from, to)`, `columns(area, &[(lo, hi)], gap)`, `gradient(r, radius, from, to, Along::X \| Y)` |
| `mdshared_ui::glyph` | The icons: SVG path data on a 16 px grid, drawn as strokes by `Painter::icon` | The constants (`FOLDER`, `SEARCH`, `PLAY`, `SORT`, ...), `ALL` |
| `mdshared_ui::theme` | The chrome colours, light and dark | `Palette` (fields `page`, `bar`, `pane`, `text`, `muted`, `faint`, `hover`, `press`, `on`, `line`, `accent`, `on_accent`, `field`, `popup`, ...), `LIGHT`, `DARK` |

## Text (`mdshared-text`, `mdshared-highlight`)

| Module | For | Start at |
|---|---|---|
| `mdshared_text::text` | Laying out and rasterizing text for a document page: parley layouts drawn with subpixel masks, a byte-capped glyph cache | `TextEngine::new`, `draw_layout`, `draw_layout_clipped`, `set_subpixel`; `Subpixel`, `Brush` |
| `mdshared_text::fonts` | One font enumeration per thread, fallback families, colour emoji runs | `shared_fonts`, `add_fallbacks`, `push_emoji_runs` |
| `mdshared_highlight` | Highlighting a whole code block by language name | `tokens`, `Token` |
| `mdshared_highlight::incremental` | Highlighting line by line for an editor, with checkpoints and limits | `LineHighlighter`, `Language::for_path` / `by_token`, `Limits`, `Span` |
| `mdshared_highlight::scan`, `sfc` | The hand-written scanners behind the above (Kotlin, Swift, PowerShell, TypeScript, Vue, Svelte, ...) | `scan::Lang` |

## Widgets (`mdshared-ui`)

Each is geometry plus drawing; the application records hits and handles the keyboard unless
the row says the widget holds state.

| Module | For | Start at |
|---|---|---|
| `chrome` | The title row: caption buttons, the tab strip's widths, one tab | `TITLE_H`, `draw_win_button`, `tab_widths`, `draw_tab`, `draw_new_tab_button`, `button_fill` |
| `window` | Frameless-window helpers: resize edges, title double click, identity, pinch zoom | `edge_at`, `title_double_click`, `identify`, `set_edited`, `PinchZoom` |
| `pane` | Side panes: head, head switches, a selectable row's fill, the splitter line | `draw_head`, `draw_head_switches`, `draw_row_fill`, `draw_splitter` |
| `knob` | A rotary knob and a mixer fader, both holding a `Slider`: the knob turned by vertical drag (Shift fine, no jump), bipolar for pan; the fader with dB ticks and labels ("-inf" at silence); reset for the double click (provisional) | `Knob::new(slider, default).bipolar()`, `press(r, x, y)`, `drag(x, y, fine)`, `release`, `reset`, `wheel`, `step_by`, `angle_of`; `Fader::new(slider, default)`, `drag(r, x, y, fine)`, `ticks(r, marks)`; `draw_knob`, `draw_fader`, `draw_fader_with`; `DEFAULT_TICKS` |
| `media_session` | The system's now-playing surface and media keys (feature `media-session`): SMTC on Windows, MPRIS on Linux; the player gives a `NowPlaying` and takes `Command`s after a wake. Without the feature, and on macOS, `new` is `Unsupported` (provisional; Linux unverified) | `MediaSession::new(&Identity { name, bus_name, desktop_entry }, hwnd, wake)`, `set(&NowPlaying)` (sends what `changes` found), `seeked(position)`, `commands() -> Vec<Command>`; `PlayState`, `Command::{Play, Pause, PlayPause, Stop, Next, Previous, SeekBy, SetPosition, Raise, Quit}` |
| `menu` | Popup and context menus drawn in the window, with access keys and scrolling. Holds state | `Menu::new(items, x, y, right)`, `Item::new(id, label).hint().icon().checked()`, `draw_in`, `draw_in_presence`, `pointer`, `step`, `chosen`, `access_key` |
| `menu_bar` | The native menu bar of a Mac application | `install`, `AppCommand` |
| `meter` | A level meter: the level jumps up and falls back, the loudest reading held above it and then falling, a clip mark until reset; drawn in three zones (accent, warn, danger), flat or upright, one or a stereo pair. Time is given, never read (provisional) | `LevelMeter::new().floor().release().hold().fall()`, `update(now, peak)`, `level_db`/`peak_db`, `fraction`/`peak_fraction`, `clipped`/`reset_clip`, `next_wake`, `at_rest`; `draw_meter(p, pal, r, meter, now, vertical)`, `draw_stereo`; `ZONE_WARN_DB`, `ZONE_DANGER_DB` |
| `overlay` | Toast, tooltip and link-hint bubbles; the pieces of a modal dialog | `Bubble::toast` / `tip` / `hint`, `draw`, `draw_presence`; `dialog_card`, `draw_backdrop`, `draw_dialog_card`, `draw_dialog_button`, `draw_keycaps`, `dialog_bounds` |
| `notice` | Bars across the top of the page: update, crash report, an offer with two buttons | `draw_update_bar`, `draw_crash_bar`, `draw_offer_bar`, `bar_button` |
| `cover` | Cover art: the colours a picture suggests (background, text at 4.5:1 or better, a saturated accent) and the picture drawn letterboxed, or a music note when there is none (provisional) | `colors(rgba, w, h) -> Option<CoverColors>` (once per picture, not per frame), `draw_cover(p, pal, picture, r, radius)`, `contrast`, `luminance`; `glyph::MUSIC` |
| `segments` | A segmented switch of two or three options | `switch_width`, `draw_segments`, `draw_segments_animated`, `segment_at` |
| `check` | A tick box | `Tick::new(on)`, `draw_tick`, `draw_tick_animated` |
| `slider` | A slider (linear, log or decibel scale), a seek bar, progress bars, a spinner. Holds the value | `Slider::new(min, max, value).step().vertical().scale()`, `press`/`drag`/`release`, `step_by(Step)`, `wheel`, `draw`, `draw_animated`, `Look { buffered, .. }`; `draw_progress`, `draw_progress_indeterminate`, `draw_spinner` (both on `anim::cycle`), `draw_progress_indeterminate_at`, `draw_spinner_at` (at a phase, no animator) |
| `seek` | A player's seek bar: a `Slider` over a track's length, chapter marks cut through the track, the time and chapter under the pointer as a tooltip (provisional) | `SeekBar::new(secs).chapters(vec![Chapter { at, title }])`, `slider_mut` (press, drag, wheel as a slider), `position`/`set_position`, `time_at`, `chapter_at`, `next_chapter`/`previous_chapter` (the three-second rule), `hover_label`, `marks`; `draw(.., ground)`, `draw_animated`, `hover_tip` |
| `thumbs` | Thumbnails of picture files made on a `pool` and kept: in memory (LRU with a byte cap, so drawing does no I/O) and on disk as PNG keyed by path, length, change time and size bucket (provisional) | `Thumbnails::new(workers, memory_cap, Some(DiskCache::new(dir, cap)), wake)`, `request(path, side, priority, generation) -> Lookup { Ready, Pending, Failed }`, `collect` (after a wake), `invalidate(path)`, `set_generation`; `MemoryCache`, `DiskCache::{load, store, trim}`, `make`, `bucket`, `SIZES` |
| `scroll` | The overlay scrollbar, and an offset that glides (kept as `f64`) | `scrollbar(area, content, view, offset)`, `offset_for_thumb`, `draw_scrollbar`, `draw_scrollbar_animated`, `SmoothScroll` (`scroll_by`, `jump_to`, `offset` / `offset_f64`, `current` for hit tests during a glide) |
| `textfield` | A single-line text field: text, caret, selection, IME preedit. Holds state | `TextField`, `EditKey`, `FieldColors`, `Preedit` |
| `palette` | The command palette: a text field over a ranked list. Holds state | `Palette`, `Entry::new(id, label).hint().icon().group()`, `Outcome` |
| `settings` | A settings dialog described by rows of data. Holds state | `Form::new(title, rows)`, `Row::toggle` / `choice` / `number` / `text` / `action` / `header`, `layout`, `key`, `press`, `release`, `wheel`, `Outcome`, `Value` |
| `tree` | A lazy folder tree for a files pane (model only) | `FileTree`, `Row`, `Rules` |
| `waveform` | A track's waveform: the least and the most sample of each bucket, kept at halving resolutions so any width or zoom reads the nearest level; made on the player's decode worker, filling in as it decodes; bytes for a disk cache; drawn a column a device pixel, played part in the accent (provisional) | `PeakBuilder::new(BUCKET)`, `push`, `push_interleaved`, `peaks` (so far), `finish`; `Peaks::columns(width, view)`, `to_bytes`/`from_bytes`, `samples`, `levels`; `draw(p, pal, r, peaks, view, playhead)` |
| `plan` | The plan screen of the notes library (a card with a list, a preview and options). Specific to the Markdown applications | `plan` and `plan::draw` |
| `splitter` | An area cut in two by a draggable handle, with collapse and a saved position | `Splitter::new(Hold::Ratio \| A \| B).min().snaps()`, `rects(area, axis)`, `press`/`drag`/`release`, `toggle_collapse`, `reset`, `to_setting`/`apply_setting`; `splitter::draw` |
| `table` | A table's columns: layout with flexible widths, header hits, edge drags, sort, saved widths | `Table::new(vec![Column::new(id, title, width).flex().align()])`, `layout(view_w)`, `cell(row, k)`, `hit`, `press`/`drag`/`release`, `toggle_sort`, `to_setting`/`apply_setting`; `draw_header`, `draw_cell_text` |

## List and layout models (`mdshared-ui`)

Pure arithmetic and state; none of them draws.

| Module | For | Start at |
|---|---|---|
| `rows` | A list of rows of one height: visible range, row rectangle, row at a point, key navigation, reveal, rubber band | `Rows::new(row_h, view, count)`, `visible`, `rect`, `row_at`, `nav(cursor, Nav)`, `reveal`, `thumb`, `band` |
| `grid` | The same for an icon or thumbnail grid, in two directions | `Grid::new(cell_w, cell_h, view_w, view_h, count).stretched()`, `columns`, `visible`, `rect`, `cell_at`, `nav`, `reveal`, `band`; `band_rect` |
| `select` | Which rows are selected (click, Ctrl, Shift, keys, rubber band), and type-ahead | `Selection`: `click(i, mods)`, `context`, `nav(&rows, nav, extend)`, `move_to`, `toggle`, `select_all`, `set_band`, `remap`, `contains`, `ranges`; `TypeAhead::key(ch, now, count, cursor, label)` |
| `pins` | Pinned tabs: order, moves, which to close next | `Pinnable`, `set`, `move_tab`, `next_to_close` |
| `stale`, `mdshared_app::brief` | Offering to close tabs not used for days; leaving out tabs barely looked at | `stale::is_stale`, `UseDays`; `brief::Clock`, `left_out` |
| `keys` | Shortcut strings parsed into chords and matched against key events | `parse("Ctrl+Shift+F")`, `Chord::matches`, `KeyName`, `Mods::from_winit`, `key_name`, `label`, `for_platform` |
| `commands` | An application's command table with the user's own keys over it | `Command::new(id, name, title).keys().group()`, `Keymap::new(&TABLE)`, `lookup(key, mods)`, `hint(id)`, `bind`/`unbind`/`reset`, `conflicts`, `entries` (for the palette), `write`/`read` (settings) |
| `cf_html` | The Windows clipboard's HTML format, as pure byte arithmetic | `cf_html`, `fragment` |

## Animation (`mdshared-ui::anim`)

Full account: [ANIMATION.md](ANIMATION.md).

| Piece | For | Start at |
|---|---|---|
| `anim::Animator` | Named values that move to their targets, asked for while drawing; says when the next frame is needed. A name is anything that hashes, or an `Id`, which is itself (provisional) | `begin_frame(now)`, `value`, `value_from`, `toggle`, `presence`, `set`, `get`, `keep`, `forget`, `end_frame() -> Option<Instant>`, `set_system_reduced`, `set_policy`; `Id::new(name).with(part)`, `AsId` |
| `anim::Motion`, `Easing`, `Spring`, `fluent` | How a value moves; the Windows 11 timings. A delay is kept under reduced motion; a bezier only exists checked (provisional) | `Motion::tween`, `Motion::spring`, `.delayed`, `Easing::cubic_bezier(..) -> Option`, `Easing::bezier(..)` (const), `Spring::with_threshold`, `fluent::FADE` / `ENTER` / `EXIT` / `MOVE` / `PAGE` |
| `anim::Animatable` | What can move: `f32`, `f64`, a point, `Rect`, `Rgb`, `[f32; 4]`; kept as `f64`, with a settle precision per type (provisional) | `parts`, `from_parts`, `precision()`, `lerp` |
| `anim::cycle` | What repeats while shown: a running phase, and a blink that wakes the window only when it changes (provisional) | `Animator::cycle(id, period) -> f32`, `Animator::blink(id, on, off) -> bool`, `Periodic::new(period).ping_pong().still_at(phase)`, `.phase(anim, id)`, `.at(elapsed)` |
| `anim::Clock` | Where time comes from; a clock moved by hand for tests | `SystemClock`, `ManualClock` |
| `anim::Snapshot`, `PageTransition` | A saved piece of the bitmap; cross-fade, slide or push between an old page and a new one; ends when the bitmap size, scale or region changes (provisional) | `Painter::snapshot`, `PageTransition::begin` / `apply(anim, p, region)` / `apply_dev`, `PageEffect` |
| `anim::Transient`, `pop` | Things that appear and disappear, kept alive while they leave; any drawing faded and moved as one | `Transient::show` / `hide` / `frame`, `pop(p, bounds, t, shift, draw)` |

## Platform (`mdshared-ui`)

The only modules that call the system. Each fails soft where the system lacks the feature.

| Module | For | Start at |
|---|---|---|
| `platform` | Clipboard (text and HTML), opening URLs and files, the file manager, file dialogs, system theme, reduced motion, double-click and caret times, display scale, DWM window styling and backdrop | `clipboard_text`, `set_clipboard_text`, `clipboard_html`, `open_url`, `open_file`, `reveal_in_file_manager`, `pick_files_async`, `pick_folder_async`, `pick_save_async`, `system_is_dark`, `motion_reduced` and `watch_motion_reduced(wake)` (never blocks; Linux reads the portal on a helper thread), `double_click_time`, `caret_blink_time`, `system_scale`, `style_window`, `set_backdrop` |
| `platform::recycle` (feature `recycle`) | Moving files to the Recycle Bin or Trash | `recycle`, `recycle_refusal`, `open_recycle_bin` |
| `corners`, `shadow`, `blur`, `frost` | Rounded window corners; the drop shadow of an undecorated window (Linux routes in LINUX.md); blur behind the window on KDE; a system backdrop behind the chrome | `corners::set_window_corners`, `shadow::Shadow::attach`, `blur::set`, `frost::Frost` |
| `snap` | The Windows 11 snap-layouts flyout over a hand-drawn maximize button | `set_maximize_button_rect` |
| `place` | Putting a window on a half of the screen (the two Markdown applications side by side) | `halves`, `work_area`, `set` |
| `desktop_entry` | The desktop entry and icon of an AppImage | `ensure`, `Spec` |
| `open_documents` | Files macOS asks a running application to open | `on_open` |

## Application plumbing (`mdshared-app`, `mdshared-fetch`)

No window system. All of it is usable from a test with a temporary folder.

| Module | For | Start at |
|---|---|---|
| `fmt` | Sizes, counts, lengths of time, dates, natural order of names | `size(bytes)`, `count(n)`, `duration(secs)`, `date`, `date_time(unix_secs, offset_minutes)`, `Civil::from_unix`, `natural_cmp` |
| `settings` | The `key=value` settings file: sections, escaping, atomic save, the data folder | `default_settings_path`, `read`, `pairs`, `Writer::new(section)` with `line(key, value)` and `finish()`, `write_atomic`, `escape`, `encode_path` |
| `paths`, `files` | One identity per file; reading with change stamps and decoding; creating without replacing; replacing safely | `paths::same_path`, `canonical_or_absolute`, `tab_labels`; `files::read_stable`, `stamp`, `decode`, `create_new`, `replace` |
| `recents`, `places` | The recent-files list; where the user was in each file | `Recents`, `Places` |
| `single`, `folder_lock`, `sister` | One running window per user or per settings folder; the hand-off between the viewer and the editor ([SISTER.md](SISTER.md)) | `single::listen_waking`, `forward`; `folder_lock::claim`; `sister::find`, `open`, `Request` |
| `watch` | Telling the application that an open file changed on disk | `FileWatcher` |
| `job` | One worker thread with cancel, progress and a wake closure | `Job::start`, `Cancel`, `Progress` |
| `pool` | A bounded pool for many small pieces of work (thumbnails, tags, sizes), with keys, priorities and generations that drop stale work | `Pool::new(workers, wake)`, `submit(key, priority, generation, work)`, `set_generation`, `drain`, `Token::cancelled` |
| `fuzzy` | Fuzzy subsequence matching with scores and match positions, for pickers | `score`, `rank`, `Match` |
| `foldersearch` | Searching the text files of a folder on a worker, streaming hits | `Search`, `search_tree`, `Options`, `Hit` |
| `html_md` | HTML from the clipboard turned into Markdown | `to_markdown` |
| `wikilink`, `notes`, `library`, `split`, `moves`, `runs` | The notes side of the Markdown applications: wiki-links, a notes index with backlinks, the notes library, splitting a document into notes, moving notes, and undoable runs that write files | `wikilink::scan`, `notes::NoteIndex`, `library::read`, `split::plan`, `moves::plan_group`, `runs::Run` |
| `update`, `updater`, `selfupdate`, `relaunch`, `appimage` | Updating the application in place: manifest and SHA-256, download and check, the file swap, restarting; the AppImage a process runs from | `updater::App`, `check`, `install`, `Stage`; `update::Version`, `verify_sha256_file`; `relaunch::spawn` |
| `crash` | Crash reports written at a panic or a fatal fault, offered at the next start | `install`, `pending`, `report_text` |
| `backdrop` | The "Window backdrop" setting's type and words | `Backdrop` |
| `mdshared_fetch` | Downloading untrusted `http(s)` URLs with strict limits and a disk cache; one file for an update | `Fetcher`, `State`, `fetch_file`, `Url`, `ImageRequests` |

## Terminal (`mdshared-term`)

No dependencies, no window, no process: bytes in, a grid out, events to bytes. Its crate doc
lists what a full terminal has that this does not yet (reflow, combining marks, OSC 8 and 52,
the kitty keyboard protocol, images).

| Module | For | Start at |
|---|---|---|
| `screen` | The grid a program's output is written into: cells with colours and attributes, cursor, scrollback, alternate screen, scroll regions, modes, dirty rows, replies | `Screen::new(cols, rows, scrollback)`, `feed(bytes)`, `resize`, `cell`, `line`, `row_text`, `cursor`, `modes`, `take_dirty`, `take_reply`, `take_title`, `scrollback_line` |
| `parse` | The VT byte-stream parser, apart from what the sequences mean | `Parser::feed(bytes, &mut impl Perform)`, `Params` |
| `select` | Selecting cells across scrollback and screen, by cell, word or line, as a stream or a block | `Selection::start(&screen, point, kind, clicks)`, `extend`, `contains`, `text` |
| `input` | Keys, pastes, mouse events and focus changes as the bytes a program expects | `encode_key(Key, Mods, &modes)`, `encode_paste`, `encode_mouse`, `encode_focus` |
| `width` | How many cells a character takes | `char_width`, `str_width` |

## What an application still writes itself

The window loop (winit events, softbuffer present, damage, timers), focus between widgets,
drag and drop, and everything listed in [ROADMAP.md](ROADMAP.md). A typical frame:

```rust
anim.begin_frame(clock.now());
let mut p = Painter::new(&mut bitmap, &mut gfx, scale);
p.fill(window, pal.page);
// layout from models: splitter.rects(..), table.layout(..), rows.visible() ...
// draw_* calls, recording hit rectangles as you go
// overlays last: menu, bubbles, dialog
present(&bitmap);
match anim.end_frame() {
    Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
    None => event_loop.set_control_flow(ControlFlow::Wait),
}
```
