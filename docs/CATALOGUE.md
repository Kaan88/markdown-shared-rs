# Catalogue: what the crates hold

Every public module of the workspace, with what it is for and where to start reading. Look here
before writing a piece of an application: most of what a window needs exists. Each module's own
`//!` comment is the full account; this page is the map. **Add a line here with every module
you add** ([NEW-MODULE.md](NEW-MODULE.md)).

Conventions that hold throughout: logical pixels; the application owns the window loop, the
events and the hits; a model answers questions and takes events, a `draw_*` function draws and
changes nothing; a function that ends in `_animated` or `_presence` is the same drawing with
motion ([ANIMATION.md](ANIMATION.md)).

## I want to ...

- **Show a Markdown page:** [`mdshared-md-preview`](#markdown-pages) for an embedded preview, or [`mdshared-md-render`](#markdown-pages) for headless layout and drawing.
- **Show a terminal:** [`termview`](#widgets-mdshared-ui) (feature `term`) draws the [`mdshared-term`](#terminal-mdshared-term) screen.
- **Run a shell in a pseudo-terminal (PTY):** [`mdshared-pty`](#pseudo-terminal-mdshared-pty).
- **Draw rich or styled text:** [`richtext` and `Painter::rich_text`](#painting-mdshared-ui-mdshared-text).
- **Draw a laid-out document with bounded monochrome glyph masks:** [`mdshared_text::text`](#text-mdshared-text-mdshared-highlight).
- **Draw text over premultiplied pixels in a frosted window:** [premultiplied text and `frost::finish_premultiplied`](#text-mdshared-text-mdshared-highlight).
- **Edit text in a field:** [`textfield`](#widgets-mdshared-ui) for mouse selection, edge scrolling and an edit menu; a multi-line editor is not yet shared ([ROADMAP K9](ROADMAP.md#barbican-fast-track-2026-11-25)). Use [`clicks`](#list-and-layout-models-mdshared-ui) to count clicks in a custom editor.
- **Pick a colour, with optional Apply and Cancel:** [`colour_picker`](#widgets-mdshared-ui) or a [settings colour row](#widgets-mdshared-ui).
- **Show a settings form and edit its text rows:** [`settings::Form` or `settings::pages::Form`](#widgets-mdshared-ui), including shaped mouse selection, an edit menu and popup-aware cursors.
- **Open a menu, submenu or context menu:** [`menu` and `submenu`](#widgets-mdshared-ui).
- **Show a file list or directory listing:** [`dirlist`](#application-plumbing-mdshared-app-mdshared-fetch) with [`rows`](#list-and-layout-models-mdshared-ui).
- **Watch a folder for changes, with a rescan fallback:** [`dirwatch`](#application-plumbing-mdshared-app-mdshared-fetch).
- **Hand files or unsaved text to a running window:** [`single` and `sister`](#application-plumbing-mdshared-app-mdshared-fetch) authenticate before sending and acknowledge delivery to the receive channel; legacy peers get liveness checks only.
- **Find the file a Linux update can replace:** [`updater::installed_files`](#application-plumbing-mdshared-app-mdshared-fetch) verifies the executable's mount and the AppImage runtime's file identity.
- **Handle file drops:** use winit's file events; toolkit drag-and-drop is not yet built ([ROADMAP L7/L8](ROADMAP.md#lists-menus-and-input)).
- **Use the clipboard:** [`platform`](#platform-mdshared-ui) for text and HTML; files and images are not yet supported ([ROADMAP L9](ROADMAP.md#lists-menus-and-input)).
- **Draw images or pictures:** [`mdshared_text::image`, `Painter::image` and `picture`](#painting-mdshared-ui-mdshared-text).
- **Make thumbnails:** [`thumbs`](#widgets-mdshared-ui) for pictures; shell icons and thumbnails are not yet provided ([ROADMAP F5](ROADMAP.md#files)).
- **Animate a control or transition between pages:** [`anim::Animator` and `PageTransition`](#animation-mdshared-uianim).
- **Make a scroll area:** [`scrollarea`, `viewport` and `scroll`](#widgets-mdshared-ui).
- **Lay out controls:** [`layout`](#list-and-layout-models-mdshared-ui) for flexible rows and columns.
- **Draw buttons and controls:** [`overlay::draw_dialog_button`, `link`, `check`, `toggle`, `dropdown` and `slider`](#widgets-mdshared-ui); the broader set is not yet on this branch ([ROADMAP K4](ROADMAP.md#barbican-fast-track-2026-11-25)).
- **Put an icon in the tray:** [`tray`](#widgets-mdshared-ui) (Windows and Linux).
- **Send a system notification:** [`notify`](#widgets-mdshared-ui) (feature `notify`, Windows and Linux).
- **Expose accessibility:** [`a11y`](#accessibility-mdshared-ui-feature-a11y) (feature `a11y`; Windows adapter today).
- **Host a window:** [`host`](#platform-mdshared-ui) (feature `host`, one window).
- **Give a window frosted chrome:** [`frost`](#platform-mdshared-ui) (the host supports it on Windows).

## Painting (`mdshared-ui`, `mdshared-text`)

| Module | For | Start at |
|---|---|---|
| `mdshared_text::bitmap` | The framebuffer every frame is drawn into: opaque BGRA bytes. PNG in and out | `Bitmap::new`, `clear`, `rrect`, `line`, `image`, `write_png`; `decode_png_rgba` |
| `mdshared_text::image` | Pictures from files: PNG and JPEG (zune-jpeg) into RGBA under limits a hostile file cannot pass, the EXIF turn applied, an area-average downscale (provisional) | `decode(bytes, Limits::DEFAULT) -> Result<Image, Error>`, `dimensions`, `sniff`, `Image::scaled_to_fit`, `scaled`, `pixel`; `encode_png`; `exif_orientation`, `orient` |
| `mdshared_ui::gfx` | The painter: boxes, frames, text, icons, clip, offset, group opacity. Everything else draws through it | `Gfx::new` (one per window), `Painter::new(bmp, gfx, scale)`; `fill`, `rbox`, `rbox_a`, `frame`, `shadow`, `shadow_with(r, radius, dark, strength)`, `soft_shadow(r, radius, &layers, k)` (a CSS `box-shadow`: the exact Gaussian blur of a rounded box, layers of `Shade { dx, dy, blur, alpha }`, recipes `Shade::POPUP` and `Shade::TAB`, `LIGHT_SHADOW`, reach `soft_bounds`; provisional), `polygon`, `blit`; `icon(glyph, size, cx, cy, c)`, `icon_rotated(glyph, size, cx, cy, degrees, c)` (the icon turned clockwise about its centre: the path is turned before it is stroked, so it stays crisp; at 0 degrees the pixels of `icon`; turned masks are not cached); `clip`/`restore`, `offset`/`restore_offset`, `snapshot`/`fade`; types `Rect`, `Rgb`, `Face`; `rgb(hex)`, `mix(a, b, t)` |
| `mdshared_ui::gfx` (text) | Interface text in three faces, measured and elided | `Painter::text`, `text_center`, `text_right`, `paragraph`, `paragraph_height`, `paragraph_wrapped`, `paragraph_height_wrapped`, `paragraph_lines_wrapped` with `Wrap::{Words, Anywhere}` (a word too wide for its column broken between clusters), `measure`, `elide`, `caret_stops`, `underline_range`, `strikethrough_range`; `paragraph_lines(face, size, width, text, colour)` with `Line { range, y, height, stops, edges }` |
| `mdshared_ui::gfx` (shapes) | Pictures, paths, rings and arcs, waveform and meter columns, gradients | `Painter::image(rgba, w, h, dst, radius, alpha)`, `fill_path`, `stroke_path` with `gfx::Path` (`move_to`, `line_to`, `quad_to`, `curve_to`, `close`, `polygon`, `svg`), `ring`, `arc(cx, cy, r, width, from, to)`, `columns(area, &[(lo, hi)], gap)`, `gradient(r, radius, from, to, Along::X \| Y)` |
| `mdshared_ui::picture` | A picture kept at its drawn size: prepared once at a device size, then blitted every frame without rescaling; prepared again when the target size, scale or corner radius changes. A decoder slot for formats the toolkit does not read | `Picture::new(image)`, `Picture::load(bytes, limits, Some(&decoder))`, `size`, `draw(painter, dst, radius, alpha)`; `Decoder` (a `Fn(&[u8], Limits) -> Option<Image>` is one) |
| `mdshared_ui::richtext` | Rich text a caller shapes itself: the painter draws the caller's own parley layout with a brush per run (colour, family, size, weight, baseline shift), through the same glyph cache as every other text draw; a spelling squiggle as one path that stays inside its band; the runs of a layout and where each went; `parley` re-exported so an application names one version (provisional) | `Painter::rich_text(&layout, x, y) -> Rect`, `Painter::wavy_underline(band, thickness, wavelength, c, alpha)`, `runs(&layout) -> Vec<RunBox { range, rect, baseline }>`, `Brush`, `parley` |
| `mdshared_ui::glyph` | The icons: SVG path data on a 16 px grid, drawn as strokes by `Painter::icon`; the tree's disclosure, a chevron that turns from right to down as a folder opens (provisional) | The constants (`FOLDER`, `SEARCH`, `PLAY`, `SORT`, ...), `ALL`; `draw_disclosure(p, cx, cy, size, open, c)` (`RIGHT` turned by `open` times 90 degrees; at 0 and 1 the plain `RIGHT` and `DOWN` icons), `disclosure_turn(anim, id, expanded)`, `draw_disclosure_animated(p, anim, id, cx, cy, size, expanded, c)` (or `draw_disclosure_animated_with(.., motion)` for a timing of the caller's), `DISCLOSURE_MOTION` (`fluent::MOVE`) |
| `mdshared_ui::theme` | The chrome colours, light and dark | `Palette` (fields `page`, `bar`, `pane`, `text`, `muted`, `faint`, `hover`, `press`, `on`, `line`, `accent`, `on_accent`, `field`, `popup`, ...), `LIGHT`, `DARK` |
| `mdshared_ui::tokens` | The look as named tokens, opt-in: a palette with an accent made legible (the application's or the system's), high-contrast palettes at 7:1, the Windows contrast theme's own colours mapped onto the palette as WinUI maps them, no backdrop under high contrast, and the widgets' sizes by name (provisional) | `Tokens::new(dark)`, `high_contrast(dark)`, `from_system_colors(&SystemColors)`, `high_contrast_with(Option<&SystemColors>, dark)`, `with_accent(rgb)`, `scaled(f)`, `from_system(follow_accent)`; `SystemColors`, `allowed_backdrop(want, high_contrast)`, `selected(pal, high_contrast) -> (fill, text)`, `Sizes::DEFAULT`, `HIGH_CONTRAST_LIGHT`, `HIGH_CONTRAST_DARK`, `accented`, `legible`, `ACCENT_CONTRAST`, `SYSTEM_FILL_CONTRAST`, `SYSTEM_LINE_CONTRAST` |
| `mdshared_ui::appearance` | The look as a user setting, opt-in: a scheme (dark, light, warm, cool), where the accent comes from (the theme, the system, the cover, a colour), corners, shadow strength, menu rows, menus joined to their control, the speed of motion and the submenu delay, as plain data saved in one setting text with the application's own keys beside them; the default is the toolkit's own look (provisional) | `Appearance::DEFAULT`, fields `scheme`, `accent`, `radius`, `shadow`, `menu_row`, `joined`, `motion`, `submenu_delay_ms`; `palette(&base, system, cover)`, `menu_look()`, `submenu_delay()`, `sane`, `write`/`read`/`read_all`, `KEYS`; `to_text(name, version, &pairs)`, `parse(text, name) -> Option<(version, pairs)>`; `Scheme::{Dark, Light, Warm, Cool}` with `ALL`, `name`, `label`, `from_name`, `is_dark`, `palette`; `WARM`, `COOL`; `AccentFrom::{Theme, System, Cover, Custom(rgb)}` with `SOURCES`, `name`, `label`, `from_name`, `apply(&base, system, cover)`; `Speed::{Off, Snappy, Fluent, Soft}` with `ALL`, `name`, `label`, `from_name`, `factor`, `scale`, `scale_ms`, `policy`; ranges `MAX_RADIUS`, `MAX_SHADOW`, `MIN_MENU_ROW`, `MAX_MENU_ROW`, `MAX_SUBMENU_DELAY_MS`; text limits `MAX_TEXT`, `MAX_PAIRS`, `MAX_KEY`, `MAX_VALUE` |

## Text (`mdshared-text`, `mdshared-highlight`)

| Module | For | Start at |
|---|---|---|
| `mdshared_text::text` | Laying out and rasterizing text for a document page: parley layouts drawn with subpixel masks, a byte-capped glyph cache. Monochrome outlines fit vertically without changing X or the layout; a failed hint draw or a fit that exceeds the movement, bounds or mask limits uses the unhinted outline. Monochrome masks are capped at 4 MiB before rendering | `TextEngine::new`, `draw_layout`, `draw_layout_clipped`, `set_subpixel`; `Subpixel`, `Brush` |
| `mdshared_text::text` (premultiplied glyphs) | Tinted text over premultiplied pixels, with grayscale coverage where the destination is translucent and the usual text drawing elsewhere. Over translucent pixels, Swash colour outlines keep their premultiplied RGB; colour bitmap strikes use straight RGB | `TextEngine::draw_layout_clipped_tinted_premultiplied`, `bitmap::blend_premultiplied`; before presenting with a frost key, `mdshared_ui::frost::finish_premultiplied` on the unmarked drawing region |
| `mdshared_text::fonts` | One font enumeration per thread, fallback families, colour emoji runs | `shared_fonts`, `add_fallbacks`, `push_emoji_runs` |
| `mdshared_highlight` | Highlighting a whole code block by language name | `tokens`, `Token` |
| `mdshared_highlight::incremental` | Highlighting line by line for an editor, with checkpoints and limits | `LineHighlighter`, `Language::for_path` / `by_token`, `Limits`, `Span` |
| `mdshared_highlight::scan`, `sfc` | The hand-written scanners behind the above (Kotlin, Swift, PowerShell, TypeScript, Vue, Svelte, ...) | `scan::Lang` |

## Markdown pages

**I want to show a Markdown page:** parse it with `mdshared_md_core::parse`, then render
it with `mdshared_md_render::render` or keep a `DocumentRenderer` for a view that scrolls.
See the [Markdown renderer README](../crates/mdshared-md-render/README.md) for dependencies,
engine shipping and editor previews. The renderer does not run a window loop.

**I want to draw into an exact-size preview pane:** use `DocumentRenderer::view_frame_sized`
with the pane's device dimensions and pass the logical layout options separately. For a
clipped whole page, set `Options::max_height` instead.

| Module or crate | For | Start at |
|---|---|---|
| `mdshared-md-core` (`mdshared_md_core`) | Parse Markdown into the document model with bounded input and wiki-link resolution supplied by the caller | `parse`, `parse_bytes`, `parse_wiki_embeds`, `Document`, `limits::notes_config` |
| `mdshared_md_core::{autolink, emoji, frontmatter, html, slug}` | Recognise bare links, shortcodes, front matter and HTML, and make heading anchors | `find_links`, `replace`, `take`, `Tokenizer`, `Slugger` |
| `mdshared_md_core::{find, links, rich, select, tasks}` | Search the page, classify links and pictures, copy selected text as HTML and find or toggle task marks | `find`, `classify_link`, `classify_image`, `selection_html`, `SpanChildren`, `word_range`, `document_boxes`, `toggle` |
| `mdshared-md-render` (`mdshared_md_render`) | Page layout, bounded bitmap drawing, animated images and export, with no window system; image paths and caller-supplied base directories are checked before probing, and layout uses one normalized scale | `DocumentRenderer`, `view_frame_sized`, `Options`, `render`, `render_slice`, `decode_image`, `export_html`, `set_engine_path` |
| `mdshared-md-engine-proto` (`mdshared_md_engine_proto`) | Versionless request and reply frames between the renderer and the separate rasterizer | `Request`, `Reply`, `write_request`, `read_reply` |
| `mdshared-md-engine` (`mdshared_md_engine`) | Separate process that draws SVG, TeX and Mermaid; its default SVG options do not load filesystem image references | `mdview-engine` binary; `options`, `render_svg`, `render_math`, `render_mermaid` |
| `mdshared-md-preview` (`mdshared_md_preview`) | Version 7 editor preview frames; `server` opts into the drawing loop | `Request`, `Update`, `wire::MAGIC`, `Server`, `serve` |

## Widgets (`mdshared-ui`)

Each is geometry plus drawing; the application records hits and handles the keyboard unless
the row says the widget holds state.

| Module | For | Start at |
|---|---|---|
| `chrome` | The title row: caption buttons, the tab strip's widths, one tab; their hover fades (`_animated`, the plain pixels at rest) | `TITLE_H`, `draw_win_button`, `tab_widths`, `draw_tab`, `draw_new_tab_button`, `button_fill`; `draw_win_button_animated`, `draw_tab_animated`, `draw_new_tab_button_animated`, `button_fill_animated` |
| `window` | Frameless-window helpers: resize edges, title double click, identity, pinch zoom | `edge_at`, `title_double_click`, `identify`, `set_edited`, `PinchZoom` |
| `window_state` | A window's place saved as one settings value and put back on the monitors there are now: its own, else the nearest; at that monitor's scale; shrunk and moved onto the work area (provisional) | `WindowState::{to_setting, from_setting}`, `restore(&state, &monitors) -> Option<Placement>`, `Monitor`, `MonitorId`; `capture(&window)`, `monitors(&window)` (winit) |
| `focus` | Which widget has the keyboard: Tab order declared each frame, rings on keyboard use only, scopes for a dialog or a menu that give the focus back when they close (provisional) | `Focus::new()`, `begin_frame`, `add` / `add_with(id, Options::no_tab_stop())` / `add_in(scope, id)`, `end_frame`, `tab(shift)`, `focus(id)`, `blur`, `key_used`, `pointer_used`, `is_focused`, `ring_visible`, `push_scope`, `pop_scope`; `draw_ring` |
| `cursor` | The pointer's cursor: one enum of the cursors the widgets ask for, the hits of the table, the splitter and the window edge turned into one, and a state that tells the window only on a change (provisional) | `Cursor` (`icon()`, `of_resize`), `of_header`, `of_splitter`, `of_window_edge`, `pick([held, hover, ...])`, `CursorState::new().set(cursor) -> Option<CursorIcon>`, `forget` |
| `pane` | Side panes: head, head switches, a selectable row's fill, the splitter line | `draw_head`, `draw_head_switches`, `draw_row_fill`, `draw_splitter` |
| `knob` | A rotary knob and a mixer fader, both holding a `Slider`: the knob turned by vertical drag (Shift fine, no jump), bipolar for pan; the fader with dB ticks and labels ("-inf" at silence); reset for the double click (provisional) | `Knob::new(slider, default).bipolar()`, `press(r, x, y)`, `drag(x, y, fine)`, `release`, `reset`, `wheel`, `step_by`, `angle_of`; `Fader::new(slider, default)`, `drag(r, x, y, fine)`, `ticks(r, marks)`; `draw_knob`, `draw_fader`, `draw_fader_with`; `DEFAULT_TICKS` |
| `media_session` | The system's now-playing surface and media keys (feature `media-session`): SMTC on Windows, MPRIS on Linux; the player gives a `NowPlaying` and takes `Command`s after a wake. Without the feature, and on macOS, `new` is `Unsupported` (provisional; Linux unverified) | `MediaSession::new(&Identity { name, bus_name, desktop_entry }, hwnd, wake)`, `set(&NowPlaying)` (sends what `changes` found), `seeked(position)`, `commands() -> Vec<Command>`; `PlayState`, `Command::{Play, Pause, PlayPause, Stop, Next, Previous, SeekBy, SetPosition, Raise, Quit}` |
| `tray` | An icon in the system's tray with a tooltip and the system's own menu of `menu::Item<u32>`s; Shell_NotifyIcon on Windows (its own hidden window and thread), StatusNotifierItem with dbusmenu on Linux; macOS `Unsupported` (provisional; Linux unverified) | `Tray::new(&Identity { name, id }, &icon, wake)`, `set_icon`, `set_tooltip`, `set_menu(&items)`, `events() -> Vec<Event::{Clicked, MiddleClicked, Chosen(id)}>` |
| `notify` | System notifications (feature `notify`): toasts on Windows under an AppUserModelID it registers in HKCU, the desktop's notification server on Linux; buttons, a tag that replaces the last one, and what the person did back as events after a wake. Without the feature, and on macOS, `new` is `Unsupported` (provisional; Linux unverified) | `Notifier::new(&Identity { name, app_id, icon }, wake)`, `show(&Notification::new(title, body).action(id, label).tagged(tag)) -> Id`, `close(id)`, `events() -> Vec<Event::{Clicked, Action, Dismissed}>`; `Urgency`, `MAX_ACTIONS` |
| `menu` | Popup and context menus drawn in the window, with access keys and scrolling. Holds state. Its look (corners, shadow and its strength, rows, padding, highlight, gaps) is opt-in through `Look`; the default is the rounded look every menu had (provisional for `Look`). A menu can be drawn joined to the control that opens it, the control a tab of the menu with a concave curve where its side meets the menu | `Menu::new(items, x, y, right)`, `Item::new(id, label).hint().icon().checked()`, `draw_in`, `draw_in_animated` (the hover row fades), `draw_in_presence`, `presence_bounds`, `pointer`, `step`, `highlight(i)`, `chosen`, `access_key`, `min_width(w)`, `set_high_contrast(on)` (the highlighted row in accent and `on_accent`); `with_look(Look::FLAT)` / `set_look`, `Look::ROUNDED`, `Look::FLAT`, `Look::sane`, `Look::shadow_strength`, `Look::soft_shadow` (the soft popup shadow instead of the toolkit's, off in both looks), `Look::text` (labels' size, `TEXT` 13.5), `Look::one_lead` (mark and icon in one `ONE_LEAD` column, `NO_LEAD` without either; off in both looks), `row_span_with`; joined to its control: `joins(tab, menu_rect)`, `join_under(p, pal, look, tab)` before the menu, `join_over(p, pal, look, tab, menu.rect())` after it, `FILLET` (provisional) |
| `submenu` | Cascading menus: a tree of items opened one menu beside another, with the hover delay, the safe-triangle aim, the keys (Right opens, Left and Escape close), flips and slides to stay on screen, and a waiting pointer change a close key may drop instead of making, putting the lit rows back on the open path; a menu anchored to a rectangle at any corner (provisional) | `Cascade::new(nodes, anchor)` / `at(nodes, x, y)`, `with_look(look)`, `keys_cancel_wait(on)`, `pointer(x, y, now)`, `tick(now)`, `deadline()`, `cancel_wait()`, `click`, `key`, `wheel`, `place_in`, `draw_in`, `draw_animated` -> `Outcome::{Chosen, Close, Outside, Ignored, ...}`; `Node::submenu(item, children)`; `around`, `beside`, `beside_with(.., look)`, `aims_at`; on `menu::Menu`: `around(rect)`, `beside(rect, leftward)`, `submenu_rows` |
| `flyout` | A card anchored to a rectangle with any content the application draws in it (text, a check box, a button): placed as `Menu::around` places a menu (below, flipped above, gone left, pushed inside, at any corner), a gap from the anchor; Escape, a press outside and a press on the anchor close it; its own focus scope, the focus back where it was before it opened, as in WinUI (provisional) | `Flyout::new(anchor, (w, h)).side(Side::Above).gap(px).padding(px)`, `place_in(area) -> Rect`, `content_rect`, `pointer_down(x, y)`, `key(&key)` -> `Outcome::{Inside, Outside, Anchor, Close, Unused}`, `opened_above`, `opened_left`, `set_anchor`; `open_scope(&mut focus, scope, first)`, `close_scope`; `draw_card(p, pal, rect)`, `draw_card_animated(p, pal, &flyout, t, content)`; `PADDING`, `GAP` |
| `menu_bar` | The native menu bar of a Mac application | `install`, `AppCommand` |
| `meter` | A level meter: the level jumps up and falls back, the loudest reading held above it and then falling, a clip mark until reset; drawn in three zones (accent, warn, danger), flat or upright, one or a stereo pair. Time is given, never read (provisional) | `LevelMeter::new().floor().release().hold().fall()`, `update(now, peak)`, `level_db`/`peak_db`, `fraction`/`peak_fraction`, `clipped`/`reset_clip`, `next_wake`, `at_rest`; `draw_meter(p, pal, r, meter, now, vertical)`, `draw_stereo`; `ZONE_WARN_DB`, `ZONE_DANGER_DB` |
| `overlay` | Toast, tooltip and link-hint bubbles; the pieces of a modal dialog | `Bubble::toast` / `tip` / `hint`, `TOAST_H`, `draw`, `draw_presence`, `presence_bounds`; `dialog_card`, `dialog_presence_bounds`, `draw_backdrop`, `draw_dialog_card`, `draw_dialog_card_with(p, pal, r, radius, soft)`, `draw_dialog_button`, `draw_keycaps`, `dialog_bounds` |
| `tooltip` | A tooltip's timing with no clock: the delay, a neighbour's tip at once, none on a control just pressed until the pointer leaves it, `deadline` for the loop (provisional) | `Tooltip::new()`, `hover(now, Option<Over { key, text, anchor }>)` (the delay from entering) or `hover_at(now, over, (x, y))` (from the pointer coming to rest), `press(key)`, `hide`, `tick(now)`, `deadline`, `waiting` (the tip that waits), `shown` |
| `notice` | Bars across the top of the page: update, crash report, an offer with two buttons | `draw_update_bar`, `draw_crash_bar`, `draw_offer_bar`, `bar_button`, `draw_sliding` and `bar_height` (a bar sliding in) |
| `infobar` | An InfoBar in WinUI's sense: info, success, warning and error cards with their own glyph and colour, a semibold title, a wrapped message, up to two actions and a close button, stacked inside any rectangle (provisional) | `InfoBar::new(severity, title, message)`, `action(id, label)`, `closable`, `height`, `hit`, `parts -> Parts { actions, close }`, `draw_infobar`, `draw_infobar_with(.., Look { hover, pressed, focused })`, `stack_layout`; `Severity`, `Action`, `Hit`, `SeverityColors`, `severity_colors`, `severity_colors_in(severity, pal, high_contrast)`, `DARK_ERROR_INK`; `Look::high_contrast` for the system's contrast theme |
| `cover` | Cover art: the colours a picture suggests (background, text at 4.5:1 or better, a saturated accent) and the picture drawn letterboxed, or a music note when there is none (provisional) | `colors(rgba, w, h) -> Option<CoverColors>` (once per picture, not per frame), `draw_cover(p, pal, picture, r, radius)`, `contrast`, `luminance`; `glyph::MUSIC` |
| `segments` | A segmented switch of two or three options, and a selector bar of any number as a tab strip; each option is as wide as its label plus padding and elides when the rectangle is too narrow (provisional) | `switch_width`, `draw_segments`, `draw_segments_animated`, `segment_at`; `bar_width`, `draw_selector`, `draw_selector_animated`, `selector_rects`, `selector_at`, `selector_key` |
| `check` | A tick box | `Tick::new(on)`, `draw_tick`, `draw_tick_animated` |
| `colour_picker` | A colour picker: a saturation and brightness square over a hue strip, the old and the new colour, the hex in a field; the colour kept as HSV so grey and black keep their hue; hex in and out in the text's own form (`#` or not, capitals or not); press, drag, arrows (Shift for more), Tab between the parts, Enter accepts, Escape cancels; square and strip cached as bitmaps; no clock (provisional) | `Hsv::new`, `hsv_to_rgb`, `rgb_to_hsv`, `parse_hex`, `HexForm::{of, format, DEFAULT}`; `Picker::new(rgb, form)` / `from_text(text, fallback)`, `press(&layout, x, y)`, `drag`, `release`, `key(key, mods, typed)`, `tab`, `set_focus`, `set_rgb`, `set_hsv`, `cancel`, `rgb`, `hsv`, `hex`, `opened` -> `Outcome::{None, Redraw, Colour, Accept, Cancel, Unused}`; `Part`; `flyout(anchor)`, `Layout::new(content)` with `square_marker`, `hue_marker`; `Cache`, `draw_picker(p, pal, card, &layout, &mut picker, &mut cache, Look { ring, caret_on })`; `CONTENT_W`, `CONTENT_H`, `PADDING` |
| `colour_picker` (with confirmation) | A popup with Apply and Cancel. Tab reaches both buttons; Enter and outside presses apply, and Escape cancels | `confirmation_flyout(anchor)`, `ConfirmationLayout::new(&flyout)`, `draw_picker_with_confirmation` with `ConfirmationLook`; `Picker::{press,pointer,release,key}_with_confirmation`, `confirmation_focus`; `Confirmation::{Applied(rgb), Cancelled, Open}` |
| `expander` | A header with a chevron and a title over a body that opens and closes; the body's height moves on the animator and jumps under reduced motion; the caller draws the body in the returned clip (provisional) | `Expander::new(expanded)`, `toggle`, `set`, `header_hit`, `key(KeyName, Mods)` (Space and Enter, no Ctrl or Alt); `HEADER_H`, `header_rect`, `body_height`, `height`, `draw_header`, `draw_expander_animated`, `draw_expander_turning` (its chevron turns as well; at rest the pixels of `draw_expander_animated`), `Look { hovered, pressed, focused, disabled }` |
| `link` | A text button that looks like a link: the accent colour, an underline under the pointer, the hand cursor, the focus ring, a deeper accent while pressed and a faint disabled look (provisional) | `Link::new(text)`, `text`, `measure`, `rect`, `hit`, `cursor`, `key`; `measure(p, text)`, `link_rect(p, x, y, text)`, `hit(r, x, y)`, `cursor`, `key(KeyName)`; `draw_link(p, pal, r, text, Look { hovered, pressed, focused, disabled, ink })` |
| `toggle` | A toggle switch on its own, the settings dialog's pixels: Space turns it over, a click when the button goes up on it; the knob slides when animated (provisional) | `Toggle::new(on)`, `on`, `set`, `hit(r, x, y)`, `key(&key, mods) -> bool`, `press`, `release -> bool`, `cancel`; `Look { hovered, pressed, focused, disabled }`, `draw_toggle`, `draw_toggle_animated`; `TOGGLE_W`, `TOGGLE_H` |
| `dropdown` | A drop-down list (a combo box that cannot be typed in) on its own, the settings dialog's field: opens a `menu::Menu` under itself, placed as `Menu::around` places one but never over the field, at least as wide as the field; the Windows keys (Space, Enter, Alt+Down, F4 open; Up and Down change it closed; Escape closes with no change) (provisional) | `Dropdown::new([(value, label), ..], selected)`, `selected`, `selected_value`, `selected_label`, `select`, `is_open`, `open_menu(field) -> Option<&mut Menu<usize>>`, `set_field(field)` (an open menu follows its field after a scroll), `close`, `menu`, `hit`, `key(field, &key, mods)`, `press`, `release`, `pointer`, `wheel`, `cancel` -> `Outcome::{Unused, None, Redraw, Opened, Closed, Changed(i)}`; `Look`, `draw_dropdown`, `draw_dropdown_animated`, `draw_dropdown_menu`, `draw_dropdown_menu_animated`; `DROPDOWN_H`, `MENU_GAP` |
| `slider` | A slider (linear, log or decibel scale), a seek bar, progress bars, a spinner. Holds the value | `Slider::new(min, max, value).step().vertical().scale()`, `press`/`drag`/`release`, `step_by(Step)`, `wheel`, `draw`, `draw_animated`, `Look { buffered, .. }`; `draw_progress`, `draw_progress_indeterminate`, `draw_spinner` (both on `anim::cycle`), `draw_progress_indeterminate_at`, `draw_spinner_at` (at a phase, no animator) |
| `sections` | A list whose rows differ in height and fall in groups: headings that stick while their group scrolls, collapsible groups, rows laid out at an estimate until measured (the screen stays still), one index space for headings and rows; the questions `rows` answers (provisional) | `Sections::new(view, estimate)`, `add_group(rows, Some(heading_h))`, `entry`/`index`, `set_height`, `set_collapsed`/`toggle`, `visible`, `rect`, `sticky`, `entry_at`, `thumb`, `nav`, `reveal`, `anchor`/`restore`; `scroll: f64` |
| `seek` | A player's seek bar: a `Slider` over a track's length, chapter marks cut through the track, the time and chapter under the pointer as a tooltip (provisional) | `SeekBar::new(secs).chapters(vec![Chapter { at, title }])`, `slider_mut` (press, drag, wheel as a slider), `position`/`set_position`, `time_at`, `chapter_at`, `next_chapter`/`previous_chapter` (the three-second rule), `hover_label`, `marks`; `draw(.., ground)`, `draw_animated`, `hover_tip` |
| `spectrum` | A spectrum analyser: the caller's FFT bins gathered into bars on a log frequency axis, each bar a `meter::LevelMeter` (rises at once, falls, holds a peak); drawn as bars in the accent or the meter's zones with peak caps (provisional) | `Bands::new(count, sample_rate, fft_size).range(low, high)`, `reduce(bins)`, `edges`, `x_of(hz, r)`; `Spectrum::new(bands)` with `floor`/`release`/`hold`/`fall`, `update(now, bins)`, `bars(now)`, `peaks(now)`, `at_rest`, `next_wake`; `Look`, `draw`, `frequency_label` |
| `thumbs` | Thumbnails of picture files made on a `pool` and kept: in memory (LRU with a byte cap, so drawing does no I/O) and on disk as PNG keyed by path, length, change time and size bucket (provisional) | `Thumbnails::new(workers, memory_cap, Some(DiskCache::new(dir, cap)), wake)`, `request(path, side, priority, generation) -> Lookup { Ready, Pending, Failed }`, `collect` (after a wake), `invalidate(path)`, `set_generation`; `MemoryCache`, `DiskCache::{load, store, trim}`, `make`, `bucket`, `SIZES` |
| `scroll` | The overlay scrollbar, upright and on its side, and an offset that glides (kept as `f64`) | `scrollbar(area, content, view, offset)`, `offset_for_thumb`, `draw_scrollbar`, `draw_scrollbar_animated`; `hscrollbar`, `offset_for_hthumb`, `draw_hscrollbar`, `draw_hscrollbar_animated`; `scrollbars` (both, clear of the corner); `SmoothScroll` (`scroll_by`, `jump_to`, `offset` / `offset_f64`, `current` for hit tests during a glide) |
| `viewport` | A view that scrolls both ways (a wide table, a terminal, a large picture): content and view sizes and both offsets as `f64`, clamped; the offset that reveals a rectangle; points between the content and the screen; both scrollbars (provisional) | `Viewport::new(content, view)`, `max`, `clamp`, `at`, `scroll_to`, `scroll_by`, `scrolls`, `visible`, `reveal(at, extent)`, `to_area`, `to_content`, `bars(area)`; `wheel_delta(across, down, shift)` |
| `scrollarea` | A scroll area, one model tying a `Viewport` to both bars, the clip, the wheel, the keys and the focus reveal: content and view sizes, offsets always kept inside them, a column laid out as long as its items want (the dialog whose body scrolls), the painter cut and moved to content pixels, and a control brought into view above, below or taller than the view | `ScrollArea::new()`, `set_view` / `set_content` / `set_offset`, `scroll_by` / `scroll_to`, `wheel(across, down, shift)`, `key(KeyName, Mods)`, `reveal(at, extent)`, `reveal_focus(&focus, id, at, extent)` (once, when the focus comes to it), `layout_column(area, &Flex, &[Item])`, `scrolled(p, area) -> Scrolled` (`restore(p)`), `bars`, `content_rect`, `to_area` / `to_content`, `viewport`, `content` / `view` / `offset` / `max` / `scrolls` / `visible`; `draw_bars`, `draw_bars_animated`; `LINE` |
| `termview` (feature `term`) | A terminal's grid drawn with the painter: a `mdshared_term::Screen` in a rectangle, one character a cell on a whole-device-pixel grid; colours and attributes from each `Cell`, wide characters over two cells, U+2500 to U+259F drawn as shapes that join, block, bar, underline or hollow cursor with an application default for shape and blink, the selection, a scrollback offset, and only the changed rows redrawn | `CellFont` (`metrics`), `Metrics` (`grid_size`, `cell_at`, `cell_clamped`, `cell_rect`, `row_rect`), `Scheme` (`from_palette`, `cell_colors`), `View` (`scrolled`, `focused`, `cursor_preference`, `point_at`, `cursor_row`, `damage`), `draw_screen`, `draw_rows`, `draw_screen_animated`, `draw_rows_animated`, `cursor_blinks_on`, `is_box_char`; also `mdshared_text::TextEngine::draw_layout_clipped_tinted` |
| `termview` (translucent background, opt-in) | Lets Windows Mica or Acrylic show through the default terminal background with an independent opacity, optional right and bottom padding, and a text contrast floor. Ink stays within its one- or two-cell span while translucency is active. Opaque when the host's backdrop is inactive, including Linux, macOS and high contrast | `BackgroundOpacity::new(opacity).with_padding(on)`, `draw_screen_with_background`, `draw_rows_with_background`; `examples/term_backdrop.rs` |
| `textfield` | A single-line text field: text, caret, visual selection bands, IME preedit. Its caret blinks at the system's rate on a caller-owned `Animator`. Opt-in mouse editing hits the shaped line: I-beam, click, drag, Shift+click, double click for a word, triple click for all, horizontal auto-scroll, and a right-click edit menu with clipboard actions disabled for secrets | `TextField`, `EditKey`, `FieldColors`, `Preedit`; `blink_caret`, `draw_blinking`; `Pointer::system(scale)` / `new(interval, slop)`, `press`, `pointer_move`, `tick`, `deadline`, `release`, `cancel`; `FieldHit::cursor`; `TextField::context_menu`, `apply_context_action`, `selection_for_export`, `ContextAction`, `MAX_PASTE_BYTES`; `examples/textfield.rs` |
| `seltext` | Read-only selectable text: a wrapped paragraph with a selection by drag, double click or Shift, carets only on cluster edges, Ctrl+C as an action for the caller's clipboard and a context menu; hit testing from the same parley layout that draws, wrapped and monospace (provisional) | `SelectableText::new()`, `with_wrap`, `set_wrap`, `seltext::lines`, `set_text`, `set_lines`, `press(lines, x, y, mods, clicks)`, `drag`, `release`, `key(KeyName, Mods) -> Option<Action>`, `select_all`, `clear_selection`, `selected_text`, `selection`, `caret`, `caret_line`, `context_menu_items`; `Action::{Copy, SelectAll}`; `draw(p, pal, area, face, size, &model, focused)` |
| `palette` | The command palette: a text field over a ranked list. Holds state | `Palette`, `Entry::new(id, label).hint().icon().group()`, `Outcome`, `draw_presence` and `presence_bounds` (its entrance) |
| `settings` | A settings dialog described by rows of data. Holds state | `Form::new(title, rows)`, `Row::toggle` / `choice` / `number` / `text` / `action` / `header`, `layout`, `key`, `press`, `release`, `wheel`, `Outcome`, `Value`; a number row as a slider, opt-in: `show_as_slider(id)` (kept by id through `set_rows`), `slider_rect(i)`, `pointer` (`pointer_move` with a slider's drag as `Outcome::Changed`); a number row's value dragged left and right, still a stepper, opt-in: `make_draggable(id)` (kept by id through `set_rows`; a step per 4 px, per 16 px with Shift, `pointer_with(x, y, mods)`; Escape puts the value back), `cursor_at(x, y) -> Option<Cursor>` (`ResizeEw` over such a value and through its drag); a text row holding a 6-digit hex as a colour, opt-in: `show_as_colour(id)` (kept by id through `set_rows`; a swatch before the field, a click on it or Enter, F4, Alt+Down opens a `colour_picker` popover over the form; each pick an `Outcome::Changed` with the text in its own form; Escape or a press outside puts the old text back), `colour(id) -> Option<Rgb>`, `colour_open`, `swatch_rect(i)`, `picker_rect`, `picker_layout` (single-page form only); with motion, opt-in: `draw_animated(p, pal, window, caret_on, anim, id)` (switches and the chosen segment slide, hover fills and the scrollbar fade, a slider's thumb glides; `draw` at rest and under `Policy::Never`), `set_high_contrast` (highlights at once); how it is drawn, opt-in: `Form::style(Style)` / `set_style`, `Style::CLASSIC` with `radius`, `soft_shadow`, `neutral_segments`, `caps_headings`, `backdrop` (the part of the classic dimming, 1 in `CLASSIC`); the paged form, the one the applications draw: `pages::Form::new(title, pages)` with `Page::new(name, items)` and `Item::new(row)` (`child`, `disabled`, `stacked`, `dropdown`, `button`), `fit`, `show_page`, `menu_rect` and `menu_bounds` (the open dropdown, for hits and damage), `pages::CARD_RADIUS`, with motion, opt-in: `pages::Form::draw_animated(p, pal, window, caret_on, anim, id)` (switches and the chosen segment slide, the list's and the open dropdown's hover fills fade; `draw` at rest and under `Policy::Never`), `pages::Form::set_high_contrast` (highlights at once) (provisional) |
| `settings` and `settings::pages` (text rows) | Opt-in mouse selection, click counting, edge scrolling and an edit menu backed by the field's real selection; secret rows cannot cut, copy or paste through the menu. Form popups take input before the text behind them | `pointer_press`, `pointer_motion`, `pointer_release`, `text_scroll_deadline`, `tick_text_scroll`, `cancel_text_pointer`; `pointer_cursor_at`, `popup_open`; `context_menu` (at a point or for the focused field), `apply_context_action`; `examples/settings_textfield.rs` |
| `settings` (colour confirmation) | A single-page text row with a colour picker that keeps edits in its popup until Apply, Enter or an outside press. Cancel and Escape leave the row untouched. The legacy live-preview colour row remains available | `Form::show_as_colour_with_confirmation(id)`, `picker_confirmation_layout`, `Outcome::Changed`; `mdshared-ui` example `colour_picker` with feature `host` |
| `tree` | A lazy folder tree for a files pane (model only; the application draws the rows, and their chevron with `glyph::draw_disclosure_animated`) | `FileTree`, `Row`, `Rules` |
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
| `layout` | Rows and columns of boxes, CSS's flexible box cut down: fixed and flexible sizes, grow and shrink weights, least and most sizes (the least wins), padding, gaps, justification along and alignment across, wrapping onto lines, edges snapped to a pixel grid with no pixel lost; returns one rectangle per item, keeps nothing (provisional) | `Flex::row()` / `column()` / `new(Direction)`, `.padding(Insets)`, `.gap`, `.line_gap`, `.justify(Justify)`, `.align(Align)`, `.wrap`, `.snap(step)`, `layout(area, &items) -> Vec<Rect>`, `layout_into`, `natural(&items)`, `cross_extent(main, &items)`; `Item::fixed(px)` / `flex(weight)` with `.grow`, `.shrink`, `.min`, `.max`, `.cross`, `.align`, `natural`; `Insets::{new, all, xy, ZERO, shrink}`; `MAX_LENGTH` |
| `select` | Which rows are selected (click, Ctrl, Shift, keys, rubber band), and type-ahead | `Selection`: `click(i, mods)`, `context`, `nav(&rows, nav, extend)`, `move_to`, `toggle`, `select_all`, `set_band`, `remap`, `contains`, `ranges`; `TypeAhead::key(ch, now, count, cursor, label)` |
| `clicks` | Counts double and triple clicks from the time, position and button, shared by the host and the text field and reusable in an editor | `ClickCounter::default()`, `press(now, at, button, interval, slop)`, `reset`, `click_slop(device_size, scale)`, `MAX_CLICKS` |
| `pins` | Pinned tabs: order, moves, which to close next | `Pinnable`, `set`, `move_tab`, `next_to_close` |
| `stale`, `mdshared_app::brief` | Offering to close tabs not used for days; leaving out tabs barely looked at | `stale::is_stale`, `UseDays`; `brief::Clock`, `left_out` |
| `keys` | Shortcut strings parsed into chords and matched against key events | `parse("Ctrl+Shift+F")`, `Chord::matches`, `KeyName`, `Mods::from_winit`, `key_name`, `label`, `for_platform` |
| `commands` | An application's command table with the user's own keys over it | `Command::new(id, name, title).keys().group()`, `Keymap::new(&TABLE)`, `lookup(key, mods)`, `hint(id)`, `bind`/`unbind`/`reset`, `conflicts`, `entries` (for the palette), `write`/`read` (settings) |
| `cf_html` | The Windows clipboard's HTML format, as pure byte arithmetic | `cf_html`, `fragment` |

## Accessibility (`mdshared-ui`, feature `a11y`)

A model like the others: it builds AccessKit nodes from the widgets' models and calls nothing of
the system. The window's adapter (`accesskit_winit`, Windows only so far) is the application's.

| Module | For | Start at |
|---|---|---|
| `a11y` (feature `a11y`) | An AccessKit tree built from the models that draw, for UI Automation: stable ids from author ids (UIA's AutomationId), updates that send only the nodes that changed and only those the root reaches, node helpers for the widgets, a list that puts every row in the tree keyed by its item, scrolls by its transform and gives bounds only to what is in view, a plain list of a group's items with their place in the set. The tree builds on every system (provisional; driven by `tests/uia/purge_shape.ps1`, not yet tried with Narrator) | `id(author)`, `item_id(list, key)`, `Tree::new` / `begin` / `add` / `get_mut` / `end(focus)` / `full`, `request -> Request`; `window`, `group`, `button`, `link`, `check_box`, `switch`, `combo_box`, `expander`, `progress`, `text`, `text_input`, `tab_list`, `tab(.., position)`, `column_header`, `dialog` (its body as a `text` child); `SWITCH_ROLE`; `clip(rect, viewport)` for row bounds; `List`, `Row`, `list`, `RowIndex`, `key_of`; `Item`, `plain_list`. In a window of `host`, `App::accessibility` fills the host's `Tree` |

## Animation (`mdshared-ui::anim`)

Full account: [ANIMATION.md](ANIMATION.md).

| Piece | For | Start at |
|---|---|---|
| `anim::Animator` | Named values that move to their targets, asked for while drawing; says when the next frame is needed, and per group of a region (`group`, `keep_group`, `group_deadlines`) for a window that repaints only what changed. A name is anything that hashes, or an `Id`, which is itself (provisional) | `begin_frame(now)`, `value`, `value_from`, `toggle`, `presence`, `set`, `get`, `keep`, `forget`, `end_frame() -> Option<Instant>`, `set_system_reduced`, `set_policy`; `Id::new(name).with(part)`, `AsId` |
| `anim::Motion`, `Easing`, `Spring`, `fluent` | How a value moves; the Windows 11 timings. A delay is kept under reduced motion; a bezier only exists checked (provisional) | `Motion::tween`, `Motion::spring`, `.delayed`, `Easing::cubic_bezier(..) -> Option`, `Easing::bezier(..)` (const), `Spring::with_threshold`, `fluent::FADE` / `ENTER` / `EXIT` / `MOVE` / `PAGE` |
| `anim::Animatable` | What can move: `f32`, `f64`, a point, `Rect`, `Rgb`, `[f32; 4]`; kept as `f64`, with a settle precision per type (provisional) | `parts`, `from_parts`, `precision()`, `lerp` |
| `anim::cycle` | What repeats while shown: a running phase, and a blink that wakes the window only when it changes (provisional) | `Animator::cycle(id, period) -> f32`, `Animator::blink(id, on, off) -> bool`, `Periodic::new(period).ping_pong().still_at(phase)`, `.phase(anim, id)`, `.at(elapsed)` |
| `anim::Clock` | Where time comes from; a clock moved by hand for tests | `SystemClock`, `ManualClock` |
| `anim::Snapshot`, `PageTransition` | A saved piece of the bitmap; cross-fade, slide or push between an old page and a new one. `PageTransition::apply` uses the painter's frost key when fading between marked chrome and premultiplied pixels. Ends when the bitmap size, scale or region changes, unless the old page is drawn again at the new size and handed to `rebase` (provisional) | `Painter::snapshot`, `Snapshot::crossfade_keyed`, `PageTransition::begin` / `apply(anim, p, region)` / `apply_dev` / `fits` / `rebase`, `PageEffect` |
| `anim::Transient`, `pop`, `pop_bounds` | Things that appear and disappear, kept alive while they leave; any drawing faded and moved as one | `Transient::show` / `hide` / `frame`, `pop(p, bounds, t, shift, draw)` |
| `anim::Stack` | Several things that enter and leave each on its own, newest at the bottom, the ones above gliding into a gap: a stack of toasts (provisional) | `Stack::new()`, `push(item, until)`, `show(item, until)` (an equal one brought back, not a second), `replace(key, item, until)`, `replace_newest(item, until)` (a readout changed in place), `revive`, `remove`, `clear`, `find`, `with_max_drawn(n)`, `with_wait_for_lift()` (a new one enters once the others have lifted; at once when nothing animates), `hide`, `keep`, `tick(now)`, `deadline`, `frame(anim, id, height, gap)` -> `Shown { item, t, lift }` |
| `anim::document` | One set of motions and timings for the document applications: `MENU` (in by `MENU_ENTER`, 117 ms), `TIP`, `TOAST`, `DIALOG` (`Pair { enter, exit }`, `Pair::transient`), `HOVER`, `TIP_DELAY` (provisional) | `document::MENU.transient()`, `document::TIP_DELAY` |

## Platform (`mdshared-ui`)

The only modules that call the system. Each fails soft where the system lacks the feature.

| Module | For | Start at |
|---|---|---|
| `platform` | Clipboard (text and HTML), opening URLs and files, the file manager, file dialogs, system theme, accent colour and high contrast (and the contrast theme's colours, watched for a switch on Windows), reduced motion, double-click time and rectangle, caret times, display scale, DWM window styling and backdrop | `clipboard_text`, `set_clipboard_text`, `clipboard_html`, `open_url`, `open_file`, `reveal_in_file_manager`, `pick_files_async`, `pick_folder_async`, `pick_save_async`, `system_is_dark` and `watch_system_dark(wake)` (never blocks; Linux reads the portal on a helper thread and wakes on a change), `system_accent`, `system_high_contrast`, `system_contrast_colors` and `watch_contrast(wake)` (provisional; a hidden window on its own thread hears `WM_SETTINGCHANGE` and `WM_SYSCOLORCHANGE`; Windows only, the wake is dropped elsewhere), `motion_reduced` and `watch_motion_reduced(wake)` (never blocks; Linux reads the portal on a helper thread), `double_click_time`, `double_click_size`, `caret_blink_time`, `system_scale`, `style_window`, `set_backdrop` |
| `platform::recycle` (feature `recycle`) | Moving files to the Recycle Bin or Trash | `recycle`, `recycle_refusal`, `open_recycle_bin` |
| `corners`, `shadow`, `blur`, `frost` | Rounded window corners; the drop shadow of an undecorated window (Linux routes in LINUX.md); blur behind the window on KDE; a system backdrop behind the chrome | `corners::set_window_corners`, `shadow::Shadow::attach`, `blur::set`, `frost::Frost` (`set_veil` for an application's own veil strength), `frost::finish_premultiplied` (protect an unmarked premultiplied region from the chrome key) |
| `snap` | The Windows 11 snap-layouts flyout over a hand-drawn maximize button | `set_maximize_button_rect` |
| `host` (feature `host`) | The winit loop, softbuffer present and timers of ONE undecorated window: winit's events reduced to logical px, damage-only presents with each rectangle kept for an app that opts in (capped at 16, otherwise one union and no allocation), no drawing while covered, minimized or hidden by the app (hiding is Windows/macOS only; a no-op on Wayland), one wake-up for the animator, the one-shot timer and the app's named timers, the window hidden but alive for a tray (drawn before it is shown again; resizes still delivered), the hidden first frame, Mica and frost on Windows (off under high contrast), resize edges and snap; with `a11y` the AccessKit tree, served on Windows (provisional) | `host::run(Config, app)`, `trait App { type Message; start, event, draw, message, timer_fired(cx, name), damage_regions() -> bool, accessibility(cx, &mut a11y::Tree) -> focus, raw(cx, &WindowEvent) }`, `Event`, `Cx::{request_redraw, request_redraw_all, animating, wake_at, timer_at, cancel_timer, hide_window, show_window, hidden, set_cursor, anim, proxy, drag_window, set_maximize_button, set_no_resize, set_backdrop, set_backdrop_veil, frost, high_contrast, accessibility_changed, exit}` (`set_backdrop_veil(Option<u8>)`: the veil over the backdrop, for a strength setting), `Proxy::send`, `Config::new(..).size(..).min_size(..)`, `access_rect` |
| `place` | Putting a window on a half of the screen (the two Markdown applications side by side) | `halves`, `work_area`, `set` |
| `desktop_entry` | The desktop entry and icon of an AppImage | `ensure`, `Spec` |
| `open_documents` | Files macOS asks a running application to open | `on_open` |

## Application plumbing (`mdshared-app`, `mdshared-fetch`)

No window system. All of it is usable from a test with a temporary folder.
`mdshared-app` defaults to `full`; with default features off it builds only `notes`,
`wikilink`, `files` and `paths`, with no third-party dependencies.

| Module | For | Start at |
|---|---|---|
| `fmt` | Sizes, counts, lengths of time, dates, natural order of names | `size(bytes)`, `count(n)`, `duration(secs)`, `date`, `date_time(unix_secs, offset_minutes)`, `Civil::from_unix`, `natural_cmp` |
| `settings` | The `key=value` settings file: sections, escaping, atomic save, the data folder | `default_settings_path`, `read`, `pairs`, `Writer::new(section)` with `line(key, value)` and `finish()`, `write_atomic`, `escape`, `encode_path` |
| `paths`, `files` | One identity per file; reading with change stamps and decoding; creating without replacing; replacing safely | `paths::same_path`, `canonical_or_absolute`, `tab_labels`; `files::read_stable`, `stamp`, `decode`, `create_new`, `replace` |
| `recents`, `places` | The recent-files list; where the user was in each file | `Recents`, `Places` |
| `single`, `folder_lock`, `sister` | One running window per user or per settings folder, with host-scoped registration on Linux and macOS. Authenticated hand-off queues large file selections only after the final batch and allows one destination per text update. Windows checks legacy ownership and write grants before a payload-free probe. All requests stay in arrival order on one worker. A failed send to a closed receiver retires the listener ([SISTER.md](SISTER.md)) | `single::listen_waking`, `listen_requests_waking`, `listen_unresolved_requests_waking`, `forward`; `folder_lock::claim`; `sister::find`, `open`, `Request` |
| `single_mutex` (Windows) | One running instance per session with no listening socket: a named mutex in `Local\`, a record of the first instance's process and window in a named file mapping, the second launch brings that window forward and hands it its command line in a `WM_COPYDATA` (provisional) | `acquire(app_id) -> Instance::{First(Guard), Running(Running)}`, `Guard::publish_window(hwnd)`, `held`; `Running::pid`, `window`, `wait_window(timeout)`; `bring_forward(hwnd)`, `send(app_id, hwnd, &args)`, `received(app_id, msg, lparam)` (unsafe, in the window procedure); `windows_of(pid)`, `main_window_of(pid)` |
| `handover` | A second launch's hand-over (`single_mutex::send`) reaching a winit window: the window procedure replaced once, each hand-over for the app's id queued and answered, a wake; Windows only (false and nothing elsewhere) (provisional) | `handover::listen(&window, app_id, wake)`, `handover::take(&window) -> Vec<Vec<OsString>>` |
| `watch` | Telling the application that an open file changed on disk | `FileWatcher` |
| `dirwatch` | Watching one folder and reporting changes for a `dirlist::Listing`. The pure coalescer settles native events; the worker reads under the startup root and reports paths as the caller's folder. On Windows configurable periodic full reads and native watch restarts recover from missed notifications. Take an exact recovery snapshot with its signals when a fresh baseline is needed. Drop does not wait for metadata reads or native teardown. See the [watcher details](FEATURES.md#a-watcher-that-says-what-changed-mdshared_appdirwatch) | `DirWatcher::new` / `with_debounce` / `builder` / `take` / `take_with_snapshot` / `woken` / `folder`; `DirWatcherBuilder::{debounce, reconcile_interval, initial_entries, start}`; `Signal` (`Changed`/`Rescan`/`Lost`); `Coalescer::new` / `with_max_wait` / `push` / `flush` / `deadline` / `is_idle`, `RawChange`, `Verdict`, `DEBOUNCE`, `MAX_WAIT` |
| `job` | One worker thread with cancel, progress and a wake closure | `Job::start`, `Cancel`, `Progress` |
| `pool` | A bounded pool for many small pieces of work (thumbnails, tags, sizes), with keys, priorities and generations that drop stale work | `Pool::new(workers, wake)`, `submit(key, priority, generation, work)`, `set_generation`, `cancel(&key)` (one key's work dropped, told and thrown away), `drain`, `drain_ordered` (by priority, then submission), `Token::cancelled` |
| `fuzzy` | Fuzzy subsequence matching with scores and match positions, for pickers | `score`, `rank`, `Match` |
| `foldersearch` | Searching the text files of a folder on a worker, streaming hits | `Search`, `search_tree`, `Options`, `Hit` |
| `dirlist` | A folder's entries as a file manager's list: read on a worker and cancellable (with the folder's error kept, for telling an unreadable folder from an empty one), links never followed, sorted by any column with the folders first and a total order, filtered, kept in step with the disk change by change, with a remap that keeps the selection on its entries. No drawing | `Listing::new`, `set_folder`, `set_entries`, `apply(Change)`, `set_sort`/`toggle_sort`, `set_filter`, `set_show_hidden`, `entry`/`row_of`/`visible`/`all`, `Remap::mapping`; `read`, `read_checked`, `FolderRead::start`/`take`/`take_checked`/`cancel`; `Entry::at`; `Column`, `Sort`, `Change` |
| `html_md` | HTML from the clipboard turned into Markdown | `to_markdown` |
| `wikilink`, `notes`, `library`, `split`, `moves`, `runs` | The notes side of the Markdown applications: wiki-links, a notes index with backlinks, the notes library, splitting a document into notes, moving notes, and undoable runs that write files | `wikilink::scan`, `notes::NoteIndex`, `library::read`, `split::plan`, `moves::plan_group`, `runs::Run` |
| `update`, `updater`, `selfupdate`, `relaunch`, `appimage` | Updating the application in place: manifest and SHA-256, download and check, the file swap, restarting; Linux verifies the running AppImage before swapping or cleaning up | `updater::App`, `check`, `install`, `Stage`; `update::Version`, `verify_sha256_file`; `relaunch::spawn` |
| `crash` | Crash reports written at a panic or a fatal fault, offered at the next start | `install`, `pending`, `report_text` |
| `backdrop` | The "Window backdrop" setting's type and words | `Backdrop` |
| `mdshared_fetch` | Downloading untrusted `http(s)` URLs with strict limits and a disk cache; one file for an update | `Fetcher`, `State`, `fetch_file`, `Url`, `ImageRequests` |

## Terminal (`mdshared-term`)

No dependencies, no window, no process: bytes in, a grid out, events to bytes. Its crate doc
lists what a full terminal has that this does not yet (reflow, combining marks, OSC 8 and 52,
the kitty keyboard protocol, images).

| Module | For | Start at |
|---|---|---|
| `screen` | The grid a program's output is written into: cells with colours and attributes, cursor (including whether the program chose its style), scrollback, alternate screen, scroll regions, modes, dirty rows, replies | `Screen::new(cols, rows, scrollback)`, `feed(bytes)`, `resize`, `cell`, `line`, `row_text`, `cursor`, `cursor_style_is_explicit`, `modes`, `take_dirty`, `take_reply`, `take_title`, `scrollback_line` |
| `parse` | The VT byte-stream parser, apart from what the sequences mean | `Parser::feed(bytes, &mut impl Perform)`, `Params` |
| `select` | Selecting cells across scrollback and screen, by cell, word or line, as a stream or a block; a word and a line run across a soft wrap (a row whose text went on in the next) and stop at a hard break; the selection holds the text it was made on, so it follows the screen after output (`follow`, `is_intact`) and is dropped when the program rewrote or erased it, or when its first line left the ring | `Selection::start(&screen, point, kind, clicks)`, `extend`, `span_on`, `contains`, `range`, `text`, `clip`, `is_intact`, `follow` |
| `input` | Keys, pastes, mouse events and focus changes as the bytes a program expects | `encode_key(Key, Mods, &modes)`, `encode_paste`, `encode_mouse`, `encode_focus` |
| `width` | How many cells a character takes | `char_width`, `str_width` |

## Pseudo-terminal (`mdshared-pty`)

The system side of a terminal: a program started with a terminal of its own, ConPTY on Windows
and `openpty` on Unix. Bytes out for `mdshared_term::Screen::feed`, bytes in from
`mdshared_term::input`. Three threads a terminal (read, write, wait) and one wake closure, as
`job` has; no call blocks. The only crate with `unsafe` system calls for processes.

| Module | For | Start at |
|---|---|---|
| (crate root) | Starting a program in a terminal and talking to it | `Command::new(program)` (`arg`, `args`, `cwd`, `env`, `env_remove`), `Pty::spawn(&command, cols, rows, wake)`, `take_output`, `write`, `resize`, `exit` -> `Exit { code, signal }`, `running`, `kill`, `pid`, `HIGH_WATER` |

## What an application still writes itself

The window loop (winit events, softbuffer present, damage, timers), unless one window is
enough and `host` runs it, focus between widgets, drag and drop, and everything listed in [ROADMAP.md](ROADMAP.md). A typical frame:

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

## What exists outside these crates

These libraries live in sibling repositories, not in this workspace. For a Cargo dependency,
use the package name with `git =` the repository URL and `rev =` its full commit below. Paths
are relative to that repository; Cargo finds the package in its checkout. **Toolkit pin** is
the revision the library actually uses, including transitive dependencies. “None” means the
library does not pull in the toolkit, even if its app does.

### Markdown viewer

[markdown-viewer-rs](https://github.com/Kaan88/markdown-viewer-rs) ·
`https://github.com/Kaan88/markdown-viewer-rs` · `rev = "01f67b2aba1a89e0e1ccfba5f423b1b8085a6b99"`.
All five reusable crates have moved into this toolkit ([ROADMAP X13](ROADMAP.md#left-over-from-what-exists)).
For new dependencies, use the `mdshared-md-*` packages above and pin them and any other
toolkit crates to the full toolkit commit that merges `feat/md-renderer`. The table below
records the old mdview revision and its actual pins until mdview switches over.

| Crate | Path | For | Toolkit pin |
|---|---|---|---|
| `mdview-preview` | `crates/preview` | Markdown page pixels for another window; enable `server` to embed it | `48eb3ce` with `server`; none without it |
| `mdview-render` | `crates/render` | Headless page layout, raster drawing, images, hit testing and HTML export | `48eb3ce` |
| `mdview-core` | `crates/core` | Markdown parser, document model, search and selection; also contains viewer session glue | `48eb3ce` |
| `mdview-engine` | `crates/engine` | SVG, TeX and Mermaid rasterizers; build and ship the `mdview-engine` binary for these in previews | None |
| `mdview-engine-proto` | `crates/engine-proto` | Request and reply frames shared by the renderer and helper process | None |

```toml
mdview-preview = { git = "https://github.com/Kaan88/markdown-viewer-rs", rev = "01f67b2aba1a89e0e1ccfba5f423b1b8085a6b99", features = ["server"] }
```

**Known revision pair:** mdview `01f67b2` uses toolkit
[`48eb3ce`](https://github.com/Kaan88/markdown-shared-rs/commit/48eb3cefb17f5025f3ac418f7a01b31b7a914779).
Pin your app's toolkit dependencies to the same URL and full revision when sharing types.
`preview/server` pulls in `render` and `core`: `render` pins `mdshared-text` and
`mdshared-highlight`, and `core` pins `mdshared-app`. Without `server`, preview is just the
wire format for `mdview --preview-server` and has no toolkit pin.

Cargo treats different Git revisions as separate crates. If your app also uses `mdshared-text`
and `mdshared-highlight` from newer toolkit `main`, Cargo builds two copies of each. Their
types do not mix: `mdview_render::Bitmap` is the older `mdshared_text::Bitmap`, so it cannot
be passed to a newer toolkit's `Painter::blit`. `mdshared-ui` brings in `mdshared-text` even if
your app does not name it directly. Separate revisions can coexist if you exchange bytes or
other revision-independent values instead of those types.

For example, [Claude Switch](https://github.com/Kaan88/claude-switch) at `fd1d3ad` uses mdview
`01f67b2` with its own toolkit pin at `d1cca33`. Its lockfile has both copies of `mdshared-text`;
it gives `Painter::image` RGBA bytes rather than handing it the renderer's `Bitmap`.

[mdedit](https://github.com/Kaan88/markdown-editor-rs) at `9898d24` uses mdview `e2b3e31` for
its preview (toolkit pin `49de4ec`). The editor itself pins `48eb3ce`; its lockfile has two
copies each of `mdshared-text` and `mdshared-highlight`. Its preview applies wire updates to
the editor's own bitmap before blitting.

### Other sibling libraries

| Repository (`git`) | Repository `rev` |
|---|---|
| [markdown-editor-rs](https://github.com/Kaan88/markdown-editor-rs) | `9898d24d85ed06b46b7a91f5dd1b194224af25b1` |
| [mdmix](https://github.com/Kaan88/mdmix) (Rust port on `f/g0-proto`, not `main`) | `adb10855c95a5f2b39c10da05f214827da6f5a29` |
| [purge](https://github.com/Kaan88/purge) | `7022fde4771ab6762a3f7111fae6e94c83aff0df` |
| [claude-switch](https://github.com/Kaan88/claude-switch) | `fd1d3ad86119dd866b64d25691f2accb161ef947` |
| [mdhub](https://github.com/Kaan88/mdhub) | `f0e0c93d3360eaf5f72cc33b1b0aaf099c59576e` |

| Repository | Crate | Path | For | Toolkit pin |
|---|---|---|---|---|
| mdedit | `mdedit-core` | `crates/mdedit-core` | Headless editor: text buffer, editing and search | `48eb3ce` (`mdshared-app`) |
| mdedit | `mdedit-highlight` | `crates/mdedit-highlight` | Background syntax highlighting over rope snapshots | `48eb3ce` (`mdshared-highlight`) |
| mdedit | `mdedit-outline` | `crates/mdedit-outline` | Find headings, functions and types for an outline | None |
| mdmix | `g6-proto` | `rust/g6-proto` | Sound BlasterX G6 packet builders and status parsers, without I/O | None |
| mdmix | `g6-device` | `rust/g6-device` | G6 device access over HID | None |
| mdmix | `g6-osaudio` | `rust/g6-osaudio` | Read G6 mixer state and output levels on Windows | None |
| purge | `purge-model` | `crates/purge-model` | Shared engine data types and JSON schemas | None |
| purge | `purge-rpc` | `crates/purge-rpc` | Engine JSON-RPC framing and message types | None |
| purge | `purge-client` | `crates/purge-client` | Windows clients for the Purge engine and broker | None |
| purge | `purge-ui-model` | `crates/purge-ui-model` | Purge-specific view state and requests, without I/O | None |
| claude-switch | `claude-switch-core` | `crates/core` | Discover Claude Code sessions and accounts without a window | None |
| mdhub | `hub-core` | `crates/hub-core` | Hub app catalogue, install detection and updates | `b4b2182` (`mdshared-app` and `mdshared-fetch`) |

The app pins are separate: mdmix's `g6-gui` pins `6017fcd` at the Rust port revision above,
Purge's UI pins `f45a71c`, and Claude Switch's UI pins `d1cca33`. None of those pins is inherited
by the libraries in the table. [Barbican](https://github.com/Kaan88/barbican) has no separate Rust
library or toolkit pin on the `main` surveyed. [mdplay](https://github.com/Kaan88/mdplay) pins
`95bbf87`, but `ui-rs` builds a static library for the foobar2000 C++ shim, not a Rust library
for another toolkit app.
