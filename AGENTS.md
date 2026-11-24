# Guide for agents working in this repository

Read this first. It says what the repository is, how it is built and checked, and the rules a
change has to keep. It is written for an agent (or a person) who has never seen the code.
`CLAUDE.md` points here; keep this file the one place these rules live.

Before writing anything, read [docs/CATALOGUE.md](docs/CATALOGUE.md): it lists every public
module and what it already does. Most "new" widgets are a few calls to things that exist.

## What this is

`markdown-shared-rs` is a Rust workspace of libraries: a small GUI toolkit and the application
plumbing that goes with it. It began as the code two applications share, the Markdown viewer
`mdview` (markdown-viewer-rs) and the Markdown editor `mdedit` (markdown-editor-rs), and is
growing into the toolkit for the owner's other applications: `mdmix` (a Sound Blaster G6 control
panel), and next a music player, a file manager and a terminal emulator.

The toolkit draws everything itself. There is no native widget and no GPU: a software painter
(`mdshared_ui::gfx::Painter`) draws a whole frame into a `Bitmap` of BGRA bytes, which the
application shows with softbuffer. **There is no window loop here**, except the opt-in window
host `mdshared_ui::host` (feature `host`, one window). Each application owns its
winit 0.30 loop, redraws only on events (`ControlFlow::Wait` / `WaitUntil`), and calls into
these crates for geometry, state and drawing. An idle window does no work; keep it so.

## The crates

| Crate | Role | Depends on |
|---|---|---|
| `mdshared-text` | `Bitmap`, the text engine (parley shapes, swash rasterizes, subpixel masks), the shared font context, PNG decoding | parley, swash, png |
| `mdshared-highlight` | Syntax highlighting: syntect for what it bundles, hand-written line scanners for the rest, an incremental line highlighter | syntect |
| `mdshared-fetch` | Downloads with strict limits: WinHTTP on Windows, the system libcurl loaded at run time elsewhere; the image cache | nothing of ours |
| `mdshared-app` | Plumbing with no window system: paths, files, settings file layer, recents, single instance, watching, worker thread and pool, fuzzy match, formatting, updates, crash reports, the notes library | notify |
| `mdshared-ui` | The toolkit: painter, palettes, icons, widgets, list and layout models, animation, key parsing, and the OS glue (`platform`) | `mdshared-app`, `mdshared-text`, winit, zeno |
| `mdshared-term` | A terminal emulator's model: VT parser, screen, selection, key and mouse encoding | nothing at all |

`mdshared-ui` depends on winit only for types (`Window`, key events); it never runs a loop.
`mdshared-term` and `mdshared-fetch` depend on none of the other crates, on purpose.

## How the applications use it

Each application names a commit of this repository in its `Cargo.toml`:

```toml
mdshared-ui = { git = "https://github.com/Kaan88/markdown-shared-rs", rev = "<commit>" }
```

So nothing here reaches an application until its pin moves, and each application moves on
purpose and runs its own tests then. The other side of that: **an application that moves its
pin must still compile and behave as before without changing a line.** See "Compatibility".
For work on both sides at once, an application uses a local `[patch]` to a checkout on disk.

The applications live in other repositories. Do not edit them from here, and do not edit other
checkouts or worktrees of this repository than the one you were given.

## Build, test, lint

CI (`.github/workflows/ci.yml`, described in [docs/CI.md](docs/CI.md)) runs these on Linux,
Windows and two Macs, with Rust 1.99.1, and nothing may fail or be skipped:

```sh
cargo fmt --all --check
cargo clippy --release --workspace --all-targets -- -D warnings
cargo test --release --workspace            # --no-fail-fast on Windows and macOS
cargo clippy --release -p mdshared-ui --all-targets --features recycle -- -D warnings
cargo test --release -p mdshared-ui --features recycle recycle
cargo clippy --release -p mdshared-ui --all-targets --features media-session -- -D warnings
cargo test --release -p mdshared-ui --features media-session media_session
cargo clippy --release -p mdshared-ui --all-targets --features notify -- -D warnings
cargo test --release -p mdshared-ui --features notify notify
cargo clippy --release -p mdshared-ui --features a11y --all-targets -- -D warnings
cargo test --release -p mdshared-ui --features a11y a11y
cargo clippy --release -p mdshared-ui --all-targets --features host -- -D warnings
cargo test --release -p mdshared-ui --features host host::
cargo clippy --release -p mdshared-ui --all-targets --features host,a11y -- -D warnings
cargo test --release -p mdshared-ui --features host,a11y host::
```

Run all fifteen before you say a change is done. While working, the debug forms are faster
(`cargo test -p mdshared-ui --lib <module>::`, `cargo clippy -p mdshared-ui --all-targets -- -D
warnings`). Two traps:

- In a shell pipeline, `cargo clippy ... | tail && git commit` commits even when clippy failed:
  the pipe hides the exit code. Check the result before committing.
- Clippy reads doc comments as Markdown. A line of a `///` comment that starts with `1)` or `-`
  is a list item to it (`doc_lazy_continuation`); reword the sentence.

The tests are headless: nothing opens a window. Sheets for a person to look at are `#[ignore]`d
tests that write PNGs under `target/specimen/` (`cargo test -p mdshared-ui specimen --
--ignored`, and `shapes_sheet`, `slider_sheet`, `icon_sheet`).

To type-check the Linux code from Windows (CI does it for real), a stub `pkg-config` that
prints a version and exits 0 is enough: `PKG_CONFIG=<stub> PKG_CONFIG_ALLOW_CROSS=1 cargo
clippy -p mdshared-ui --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`. Nothing
is linked or run that way; say so when you report it.

## Rules

### Dependencies

- **No new third-party dependency without a reason written in the commit.** Look in
  `Cargo.lock` first: a crate that is already there costs nothing to name. `zeno` (paths),
  `png`, `zbus`, `objc2-*`, `windows-sys` are there.
- A new direct dependency is pinned exactly: `name = "=1.2.3"`. (Some older entries are ranges;
  do not loosen any and do not copy that.) `Cargo.lock` is committed.
- A dependency that only one platform needs goes under that platform's `[target]` table, and
  one that only some applications need goes behind a feature (`recycle`, `client-margin`, `media-session`, `notify`, `a11y`).
- Hand-written is the house style where the job is bounded: the HTML-to-Markdown converter,
  the language scanners, the VT parser, SHA-256, the date arithmetic are all written here.

### Shape of the code

- **Model and drawing are separate.** A widget is (a) a model: plain state and pure functions
  of rectangles and numbers, testable with no painter, and (b) `draw_*` functions that take a
  `&mut Painter`, a `&Palette` and the model, and change nothing. `rows::Rows`, `table::Table`,
  `slider::Slider` are the pattern.
- **No callbacks, no retained widget tree.** The application owns the events. It passes a
  press, a key or a wheel turn into the model (`press`, `drag`, `release`, `key`, ...), which
  returns what happened (a `bool`, an `Outcome`, a hit), and the application acts on it. The
  one closure in the tree is the wake of a worker (`job`, `pool`, `watch`), which nothing here
  interprets.
- **Nothing remembers the layout.** Models are given the rectangle on every call (or lay out
  on an explicit `layout(width)`), so a resize needs no telling.
- **No clock is read inside a model.** Time comes in as an argument (`Instant`, or
  `anim::Clock`), so tests run on a clock moved by hand.
- **Small, focused modules.** One file, one job, and a module doc that says what it refuses
  ("Deliberately not here: ..."). If a file passes a thousand lines, split it into a directory
  module (`settings/`, `plan/`, `anim/`).
- **Nonsense in, nothing out.** Public functions take NaN, infinities, empty lists, zero sizes
  and hand-edited settings lines without panicking, and the tests feed them these.
- **`unsafe` only at the system's edge.** It lives in the modules that call the OS: `platform`
  (and `platform/recycle`), `snap`, `corners`, `place`, `menu_bar`, `open_documents`,
  `desktop_entry`, `blur/wl`, `shadow/wl`, `mdshared-app/crash`, `mdshared-app/single_mutex`, `mdshared-fetch/winhttp` and
  `curl`. Every block has a `// SAFETY:` line. A model, a widget or a parser has none.
- Logical pixels in every public API; the painter multiplies by the scale and snaps to device
  pixels. Functions that take device pixels say so in their name or doc (`fill_dev`, `blit`).

### Doc comments

Long and in plain English. A module starts with a `//!` that says what it is, who owns what
(the caller usually owns events, hits and values), and what it deliberately does not do. A
public item's `///` says what it does to what, what the edge cases give, and why where the why
is not obvious. Write for someone who has not seen the code: full sentences, the same word for
the same thing every time, no jargon that is not explained. Look at `rows.rs` or `job.rs`
before writing your first.

### Tests

- Every public function is tested, in a `#[cfg(test)] mod tests` at the foot of its file (or
  `<name>_tests.rs` when long). Test names are sentences that state a fact:
  `a_drag_moves_the_cut_by_as_much_as_the_pointer_and_stops_at_the_least_sizes`.
- **Drawing is tested by pixels.** Draw into a `Bitmap`, read pixels back, compare with the
  palette's colours or with another drawing. The strongest form: "the new drawing at rest is
  the old drawing, pixel for pixel" (`segments`, `slider`, `splitter` do this).
- **Time is tested on `anim::ManualClock`**, never with sleeps. Threads are tested with
  channels and gates with timeouts, never with a sleep as the condition.
- Check arithmetic against a second, plain way of computing the same thing (the rubber band
  against the overlap test of every rectangle; the bezier solver against a fine sampling),
  and state machines against pseudo-random input from a small LCG with a fixed seed.
- A thing a person must judge gets an `#[ignore]`d sheet test that writes a PNG, light and
  dark, at scale 1 and 1.5.
- `cargo test` must pass on all three systems. Code under `cfg(windows)` needs its tests under
  the same `cfg`; a test that needs a desktop service must pass without it.

### Compatibility

`mdview` and `mdedit` must compile and behave exactly as before after any change here.

- **Add; do not change.** A new behaviour is a new function beside the old one
  (`draw_segments_animated` next to `draw_segments`), a builder method that defaults to off
  (`Menu::access_keys`), or a new module. Do not change a signature, a public field's type or
  what an existing function draws.
- Adding a variant to a public enum, or a public field to a struct that applications build
  with a literal, breaks them. Private fields and new methods are safe.
- If two functions now do one job, make the old one call the new one and test that its output
  is unchanged against a copy of the old code (`plan::thousands` → `mdshared_app::fmt::count`).
- If a change cannot keep this rule, stop and ask the owner; do not decide it yourself.

### Reporting

Report plainly, including what was not run. "Nothing counts as working on Windows until it has
run on Windows", and the same for Linux and macOS: code that was only compiled is "compiled,
not run". The owner approves how a thing looks by looking at it; tests and screenshots do not
stand in for that, so say "not seen by a person" where it is true. Every feature's entry in
the docs ends with what was run and what was not.

## Commits

- One piece of work per commit; do not mix a feature with an unrelated fix.
- Subject: `<crate or area>: <what, in plain words>`, for example
  `mdshared-ui: splitter, an area cut into two panes by a handle that is dragged`.
  Areas in use: `mdshared-ui`, `mdshared-app`, `mdshared-text`, `mdshared-highlight`,
  `mdshared-fetch`, `mdshared-term`, `shared` (several crates), `docs`, `ci`.
- Body: what was added and how it behaves, then what was run and what was not. A new
  dependency is justified here.
- An agent's commit ends with a trailer naming the model that wrote it, for example
  `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- Do not push, and do not touch `main`, unless the owner asked. Work on the branch you were
  given.

## Platforms

Windows is the first platform; Linux and macOS are supported and checked by CI.

- **Windows:** undecorated windows drawn by the application, DWM for corners, shadow and
  backdrop, WinHTTP for downloads, `windows-sys` for everything else.
- **Linux:** read [docs/LINUX.md](docs/LINUX.md) before touching anything under
  `cfg(not(windows))`. It says what was run on which desktop (KDE Plasma on Wayland, sway) and
  what was only compiled, how the window shadow needs patched forks of winit and softbuffer
  (feature `client-margin`, `docs/patches/`), and how the settings portal is read. System
  libraries (libcurl, the portal) are loaded or asked at run time and may be absent: every
  such call fails soft and has a time limit.
- **macOS:** AppKit through winit, rfd and `objc2`; the menu bar (`menu_bar`), the
  open-documents Apple Event (`open_documents`). `keys::Mods::ctrl` is Command there.

## Where things are written down

| File | What |
|---|---|
| [docs/CATALOGUE.md](docs/CATALOGUE.md) | Every public module, its purpose and entry points. **Update it with every module you add or extend.** |
| [docs/NEW-MODULE.md](docs/NEW-MODULE.md) | The checklist for adding a module |
| [docs/ROADMAP.md](docs/ROADMAP.md) | What is not built yet, sized, with what each builds on. Claim an item there before starting it |
| [docs/FEATURES.md](docs/FEATURES.md) | One section per feature added since the applications began using the crates: design notes, API added, what was verified |
| [docs/ANIMATION.md](docs/ANIMATION.md) | The animation model and how an application wires it into its loop |
| [docs/PLAN.md](docs/PLAN.md) | Why the repository exists and the original order of work (the shared window frame, step 4, is still open; see the roadmap) |
| [docs/LINUX.md](docs/LINUX.md), [docs/SISTER.md](docs/SISTER.md), [docs/CI.md](docs/CI.md) | Linux support, the hand-off protocol between the two Markdown applications, CI |
| `docs/next/` | Notes for a release in preparation |
