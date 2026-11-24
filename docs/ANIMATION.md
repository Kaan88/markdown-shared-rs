# Animation (`mdshared_ui::anim`)

How the shared toolkit animates, and what an application does to use it. Added on 2026-11-24.
Everything here is opt-in: an application that calls none of it draws and behaves as before.

**Not verified:** nothing here has run in a window. The module is covered by unit tests on a
clock moved by hand (below); how the motions look and feel, and the wiring into a real winit
loop shown under "The window loop", have not been seen by anyone. macOS and Linux were not run;
the Linux build was checked by `cargo clippy --all-targets` for the target only (nothing linked
or run), so the reading of the settings portal on a helper thread and its `SettingChanged`
signal have never met a portal.

Changed after a review on 2026-11-24 (the entries below say "review" where they come from it):
values are kept as `f64`, an `Id` is its own name, a spring ends at a precision the value's type
gives, a delay survives reduced motion, a kept value goes on moving, a page switch ends when the
window changes under it, `Easing::CubicBezier` only exists checked, the Linux reading of the
system's wish no longer blocks, and there are `cycle` and `blink` for what repeats.

## The model

The applications draw a whole frame into a `Bitmap` when something happens, and sleep otherwise.
Animation keeps that shape: there is no list of running animations and no timer thread.

- While it draws, the application asks an `Animator` for the current value of a thing it names,
  and says where that thing should be: `anim.value("sidebar-width", 240.0, fluent::MOVE)`. The
  animator remembers each name from frame to frame and moves its value by the motion asked for.
- At the end of the frame the animator says whether another frame is needed, and when. While a
  value is on its way that is one frame interval later; when everything has arrived it is
  "none", and the window sleeps again. An idle window does no work.
- A name first seen is at its target: nothing animates on the frame a thing first appears. A new
  target starts from where the value is, so nothing jumps when the user changes their mind half
  way. A name that a frame does not ask for is forgotten at the end of that frame, so there is
  nothing to clean up.
- What repeats (a spinner, a blinking caret) follows the same rule: asking for its phase is what
  keeps it going, and the first frame that does not ask lets the window sleep.
- A name is anything that hashes (`"sidebar"`, `("row", i, "fill")`), taken as `impl AsId`. An
  `Id` is a name already hashed and is itself, not hashed again: `Id::new("a")` names the value
  `"a"` names, and a part under it is `id.with("fill")`. (For that an `Id` is not `Hash`; a tuple
  with an `Id` in it is not a name.) One name is one value: two different targets for one name
  in one frame trip a `debug_assert`.

Time is the frames' time, from a `Clock`: `SystemClock` in an application, `ManualClock` in a
test or for a render that must come out the same every time. A frame never moves the animations
by more than 50 ms (`set_max_step`), unless the animator itself asked to sleep longer: after a
stall or a minute minimised, what was in flight goes on from where it was and does not jump.

## The pieces

| Piece | What it is |
|---|---|
| `Animator` | The values in flight of one window. `begin_frame(now)`, then `value`, `value_from`, `toggle`, `presence`, `cycle`, `blink`, `set`, `get`, `keep`, `forget` while drawing, then `end_frame() -> Option<Instant>` |
| `Id`, `AsId` | A name, and what can be one: anything that hashes, or an `Id` (which is itself) |
| `Motion` | How a value moves: `Motion::SNAP`, `Motion::tween(duration, easing)`, `Motion::spring(spring)`, and `.delayed(duration)`. A delay is a wait and is always kept: on a snap, and under reduced motion (a tooltip's half second stays half a second; only the fade goes) |
| `Easing` | Linear, the in / out / in-out curves (quad, cubic, quint, sine, expo), `OutBack`, and the CSS cubic bezier: `Easing::cubic_bezier(x1, y1, x2, y2) -> Option<Easing>` refuses what CSS refuses (a number that is not finite, an x outside 0..=1), `Easing::bezier(...)` is the `const` form for constants (it pulls a wrong curve right and never panics). `Easing::EASE` and the other CSS keywords |
| `Spring` | Stiffness and damping, or `Spring::response(seconds, damping_ratio)`; `SMOOTH` (the default, critically damped), `SNAPPY`, `BOUNCY`; a settle threshold. Stepped by its exact solution, so any frame time gives the same path. With no damping it swings for ever and keeps asking for frames |
| `Animatable` | What can move: `f32`, `f64`, `(f32, f32)`, `Rect`, `Rgb`, `[f32; 4]`. Kept as `f64` while it moves. `precision()` says how close counts as there, which is what ends a spring: a thousandth for a bare number, a quarter of a pixel for a point and a `Rect`, just under half a step for an `Rgb` |
| `Periodic` | A cycle with its settings: a period, `.ping_pong()`, `.still_at(phase)` for reduced motion; `.phase(anim, id)`, and `.at(elapsed)` for the arithmetic alone |
| `fluent` | The Windows 11 timings and curves, and motions made of them (below) |
| `Snapshot` | A saved rectangle of the bitmap, and its `crossfade`, `slide_fade` and `push` with what was drawn there since. It is blended only into a bitmap of the size it was taken from |
| `PageTransition` | A page or tab switch of a region, driven by a `Snapshot` of the old page. It ends at once if the bitmap's size, the scale or the region changes under it |
| `Transient<T>` | A slot for a thing that comes and goes, which keeps it while its exit plays |
| `pop` | Draws anything faded as a whole and moved into place: the entrance and exit of a popup |
| `Policy`, `set_system_reduced` | Reduced motion (below) |

The painter gained what these need: `Painter::offset` / `restore_offset` (move everything drawn
by whole device pixels, so text stays sharp), and `Painter::snapshot` / `fade` (give a group of
drawing calls one opacity: save the pixels, draw, blend the saved ones back). There are no
layers and no scaling; a popup moves and fades, as the Windows ones do. `Painter::offset` moves
what is drawn in logical px; it does not move `fill_dev` and `blit`, which say where in the
bitmap, so a drawing that uses them inside `pop` fades as a whole but that part does not move
(add `Painter::origin` to its device coordinates).

### Precision

A value is kept as `f64` whatever its type. An `f32` has steps of four at fifty million, so a
scroll offset far down a terminal's scrollback or a folder of a hundred thousand rows would
glide in jumps and stop beside its target; asked for as an `f64` it moves by fractions of a
pixel and ends exactly on target (tested at 5e7, for a tween, a spring and `SmoothScroll`). The
values of a widget stay `f32`. Because both are animatable, a bare literal whose result nothing
else types (`anim.value("x", 1.0, m).round()`) needs a suffix: `1.0_f32`.

A spring has no fixed end; it ends when every number is within the precision of the value's
type and too slow to leave again, or within the spring's own `with_threshold` if that is
coarser. So a colour on a spring stops asking for frames when the colour read stops changing
(about a third of a second for `Spring::SMOOTH`, where it was twice that), and a bare `f32` that
is a position in pixels wants `Spring::SMOOTH.with_threshold(0.25)`, as `SmoothScroll` has.

A critically damped spring has 1.36 % of its way left one `response` after it starts (exactly
`(1 + 2π) e^(-2π)`), and a thousandth after about one and a half.

### Timings (`anim::fluent`)

Read on 2026-11-24 from Microsoft Learn, "Timing and easing"
(`windows/apps/design/motion/timing-and-easing`) and "Motion in Windows 11"
(`windows/apps/design/signature-experiences/motion`, the table "Animation properties").

| Name | Value | Source and use |
|---|---|---|
| `FASTER` | 83 ms | WinUI `ControlFasterAnimationDuration`; a fade ("Fade – In + Out, Linear, 83ms") |
| `FAST` | 167 ms | WinUI `ControlFastAnimationDuration`; changes of a control, and every exit |
| `NORMAL` | 250 ms | WinUI `ControlNormalAnimationDuration`; something larger entering or moving |
| `SLOW` | 333 ms | The third timing of the table (167, 250, 333); no `Control…` name on the pages read |
| `DECELERATE` | `cubic-bezier(0, 0, 0, 1)` | "Fast Out, Slow In", for what enters |
| `ACCELERATE` | `cubic-bezier(1, 0, 1, 1)` | "Slow Out, Fast In", for what leaves |
| `POINT_TO_POINT` | `cubic-bezier(0.55, 0.55, 0, 1)` | "Point to Point", for what moves and stays |

Motions made of them: `FADE` (83 ms linear), `ENTER` and `EXIT` (167 ms), `ENTER_LARGE` (250 ms),
`MOVE` and `MOVE_LARGE` (point to point, 167 and 250 ms), `PAGE` (250 ms decelerating).
`fluent::web` holds the Fluent 2 tokens of the web library (`microsoft/fluentui`,
`packages/tokens/src/global/durations.ts` and `curves.ts`): 50 to 500 ms and nine curves. Not
read: WinUI's own `Common_themeresources_any.xaml` (the paths tried were not found), so the three
`Control…AnimationDuration` values are as Microsoft Learn states them.

## The window loop

The toolkit has no window loop; this is what an application adds to its own (winit 0.30).

```rust
struct App {
    anim: Animator,
    clock: SystemClock,
    // ...
}

// At start, and again when the system's settings may have changed (ThemeChanged, Focused(true)):
app.anim.set_system_reduced(platform::motion_reduced());
// Once, at start: on Linux the first answer comes from a helper thread a moment later, and the
// setting may change at any time. `Woken::MotionChanged` is the application's own user event;
// its handler runs the line above again and asks for a redraw. (Nothing is called elsewhere.)
let proxy = event_loop.create_proxy();
platform::watch_motion_reduced(move || _ = proxy.send_event(Woken::MotionChanged));
// The application's own setting, if it has one: Policy::System, Always or Never.
app.anim.set_policy(policy);

// WindowEvent::RedrawRequested:
app.anim.begin_frame(app.clock.now());
draw_everything(&mut painter, &mut app.anim /* , ... */);
present();
match app.anim.end_frame() {
    // Something is on its way: come back at `deadline`.
    Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
    // Nothing moves: sleep until an event.
    None => event_loop.set_control_flow(ControlFlow::Wait),
}

// ApplicationHandler::new_events:
if let StartCause::ResumeTimeReached { .. } = cause {
    window.request_redraw();
}
```

Notes:

- Any other event that redraws works as before; a frame drawn early is a frame like any other,
  and its `end_frame` gives the deadline anew.
- An application that already uses `WaitUntil` for its own timers (the caret, a toast's four
  seconds) takes the earlier of the two deadlines, or hands its timer to the animator with
  `anim.wake_at(instant)` before `end_frame` and uses the one deadline it gets back.
- `set_frame_interval` sets the pace while something moves (a sixtieth of a second unless set);
  an application that knows its monitor's rate sets it from that. It is never less than a
  millisecond.
- An application that repaints only the damaged part of its window either treats a frame the
  animator asked for as damage of what is animating, or calls `anim.keep(id)` for the names in
  the parts it skipped. A name not asked for and not kept starts fresh, at its target, which is
  never wrong, only without motion. A kept value goes on to its target and then stops asking for
  frames, as if it had been asked for; a `get` after `keep` reads this frame's value. The part
  itself still shows the frame that last drew it, so draw it once more when `anim.settled(id)`.
- A render that must not depend on time (`--shot`, a golden image) uses `Policy::Never`, or a
  `ManualClock`.

## Using it

A hover fade, a toggle, a value that changed:

```rust
let glow = anim.toggle(("button", id, "hover"), hovered, fluent::FADE);
p.rbox_a(r, 5.0, pal.hover, glow);

let width = anim.value("sidebar", if open { 240.0 } else { 0.0 }, fluent::MOVE_LARGE);
let fill = anim.value(("row", i, "fill"), if on { pal.accent } else { pal.field }, fluent::FADE);
```

Something that appears and disappears. The slot keeps the toast until its exit has played, so
the application's logic lets go of it at once:

```rust
// State: toast: Transient<String>
toast.show("Saved".into());            // logic
toast.hide();                          // logic, four seconds later
if let Some((text, t)) = toast.frame(&mut anim, "toast") {   // drawing
    Bubble::toast(&mut p, text, page).draw_presence(&mut p, pal, t);
}
```

Anything else that should fade and move as one thing goes through `pop`, whatever it is made of:

```rust
let t = anim.presence("about-dialog", open, fluent::ENTER, fluent::EXIT);
if t > 0.0 {
    draw_backdrop_presence(&mut p, pal, w, h, t);
    pop(&mut p, dialog_bounds(card), t, (0.0, DIALOG_RISE), |p| draw_about(p, pal, card));
}
```

A page or tab switch. The old page is the pixels of the last frame; the new page is drawn as
always and the transition blends the old one in:

```rust
// State: one for each region that switches pages.
let mut page_switch = PageTransition::new("content");

// When the switch is decided (the bitmap still holds the old page):
let p = Painter::new(&mut bitmap, &mut gfx, scale);
let effect = PageEffect::SlideFade { from: Edge::Bottom, distance: 24.0 };
page_switch.begin(&mut anim, &p, content_rect, effect);
state.page = next;

// Every frame, after the content region is drawn and before anything that lies over it:
let mut p = Painter::new(&mut bitmap, &mut gfx, scale);
draw_page(&mut p, content_rect, state.page);
page_switch.apply(&mut anim, &mut p, content_rect);
```

These lines are the doc-test of `PageTransition`, so they are compiled with the crate.
`apply` is given the region each frame: if the bitmap's size, the scale or the region's device
rectangle is not what it was when the switch began (a resize, a move to another screen, a new
layout), the switch ends at once and the frame shows the new page plainly, since the old page
is pixels and would be blended in the wrong place. `apply_dev` is the same for a bitmap held
without a painter.

`PageEffect::Crossfade` suits any content; `SlideFade` wants a page whose edge towards `from` is
its background (it repeats that edge behind the moving page); `Push { from }` moves both pages
and mixes nothing. An application whose bitmap no longer holds the old page at that moment draws
the old page once more, takes `p.snapshot(content_rect)` and passes it to `begin_with`.

Smooth scrolling. `scroll::SmoothScroll` stands in for the bare offset:

```rust
// Input: wheel, keys.
scroll.scroll_by(-dy, content_h, view_h);
// A dragged thumb, a jump to a heading: no glide.
scroll.jump_to(offset, content_h, view_h);
// Drawing:
let y = scroll.offset(&mut anim, "page-scroll", content_h, view_h).round();
// A click during the glide: the row under the pointer as drawn, with no frame asked for.
let row = ((click_y + scroll.current(&anim, "page-scroll")) / row_h) as usize;
```

The numbers given may be `f32` or `f64`; the offset is kept as `f64`. Content too long for an
`f32` (past sixteen million px) is read with `offset_f64`, `target_f64` and `current_f64`, and
the first visible row's position is taken off before anything goes to `f32`.

Something that repeats for as long as it is shown:

```rust
// A phase from 0 up to 1, a frame every interval while this line runs:
let turn = anim.cycle("sync-icon", Duration::from_millis(900));
// There and back, and half way when motion is reduced:
const PULSE: Periodic = Periodic::new(Duration::from_millis(500)).ping_pong().still_at(0.5);
let size = 4.0 + 4.0 * PULSE.phase(&mut anim, "recording-dot");
// A caret: the window wakes only when it changes, twice a second here.
if focused && anim.blink("caret", blink, blink) { p.fill(caret, pal.text); }
// On every key: the caret is solid again and its time starts over.
anim.forget("caret");
```

`blink` is given `platform::caret_blink_time()` for both times; when that is `None` (blinking
switched off) the caret is simply drawn. With reduced motion a cycle stands at phase 0 (or its
`still_at`), a blink is steadily on, and neither asks for a frame.

A `textfield::TextField` does this itself:

```rust
// Once a frame, between begin_frame and end_frame; `blink` is platform::caret_blink_time().
field.draw_blinking(p, &mut anim, "search", blink, area, Face::Ui, 14.0, &colors, "Search", focused);
// Or, for a caller that draws its own field: the `caret_on` it passes on.
let caret_on = field.blink_caret(&mut anim, "search", blink, focused);
```

Every call that types, deletes, moves the caret or changes the selection starts the blink again,
on, with no `forget` from the caller. While the field is not focused, and while a selection is
shown (the caret is hidden over it, as on Windows), the animator is not asked, so `end_frame`
gives no deadline for the caret and the window sleeps.

## Widgets with motion

Each is a second function or method beside the one that existed, which is untouched. At rest,
and on every frame under reduced motion, each draws exactly the pixels of the plain one (tested).

| Plain | With motion | What moves |
|---|---|---|
| `segments::draw_segments` | `draw_segments_animated(p, pal, anim, id, …)` | The accent fill slides to the new choice; hover and a choice made or removed fade |
| `scroll::draw_scrollbar` | `draw_scrollbar_animated(p, pal, anim, id, …)` | The thumb widens and the track fades in when active |
| a bare offset | `scroll::SmoothScroll` | The offset glides to where the wheel sent it, on a spring |
| `check::draw_tick` | `draw_tick_animated(p, pal, anim, id, …)` | The tick fades in and out |
| `overlay::Bubble::draw` | `Bubble::draw_presence(p, pal, t)` | Toast, tooltip and link hint fade; a toast also rises 8 px |
| `slider::draw` | `slider::draw_animated(p, pal, anim, id, …)` | The thumb glides to a value set by a key, the wheel or a click; its dot grows and shrinks |
| none | `slider::draw_progress_indeterminate(p, pal, anim, id, r)` | A piece of the accent runs along the track: `anim.cycle` over `PROGRESS_RUN` (1.6 s). `draw_progress_indeterminate_at(p, pal, r, phase)` is the drawing at a phase, with no animator |
| none | `slider::draw_spinner(p, pal, anim, id, cx, cy, r)` | An arc turns (`SPINNER_TURN`, 1.1 s) and breathes (`SPINNER_BREATH`, 1.6 s): two cycles. `draw_spinner_at(p, pal, cx, cy, r, phase)` is the drawing at a phase |
| `overlay::draw_backdrop` | `draw_backdrop_presence(p, pal, w, h, t)` | The dimming follows the dialog's presence |
| `overlay::draw_dialog_card` | `pop(p, dialog_bounds(card), t, (0.0, DIALOG_RISE), …)` | A dialog fades and rises as one thing |
| `menu::Menu::draw_in` | `Menu::draw_in_presence(p, pal, area, t)` | The menu fades and drops 6 px into place |

Not done, and how each would be: the settings form (`settings::Form`) draws its own switches,
ticks and scrollbar and has no animator passed in, so it is unchanged; an application can still
wrap the whole form in `pop` for its entrance. The bars of `notice` (update, crash, offer) take
their height from the application's layout, which can animate that height with `anim.value`. The
command palette, the tree's disclosure and the tab strip have no motion. The text field's caret
blinks through `TextField::draw_blinking` (above); the plain `TextField::draw` is unchanged.

## Reduced motion

`platform::motion_reduced()` reads the system's wish:

| System | What is read |
|---|---|
| Windows | `SPI_GETCLIENTAREAANIMATION`: Settings, Accessibility, Visual effects, "Animation effects" |
| macOS | `NSWorkspace.accessibilityDisplayShouldReduceMotion`: Accessibility, Display, "Reduce motion" |
| Linux | The settings portal: `org.freedesktop.appearance` `reduced-motion`, else `org.gnome.desktop.interface` `enable-animations`, else `org.kde.kdeglobals.KDE` `AnimationDurationFactor` at 0. Read on a helper thread, never on the caller's; false without a portal |

`motion_reduced()` never blocks. On Linux the portal is another process that may be slow, hung
or absent, so the call gives the last answer the portal gave (false before the first). The first
call starts a reading that stays to listen for the portal's `SettingChanged` signal; only while
nothing listens (no bus, no signal) does a later call start another reading, at most once a
second. A reading that fails or never ends leaves the last answer standing: a setting that is on
is not taken for off because the portal was slow. Each reading is numbered when it asks, and an
answer older than the one kept is dropped, so a late answer cannot undo a newer change.
`platform::watch_motion_reduced(wake)` has a wake called when the answer changes (Linux only;
elsewhere the wake is dropped), so the application has the right value a moment after it
starts and when the user changes the setting. The order of the two calls does not matter: a
wake given after the portal already answered is called once at once.

The application passes the answer to `Animator::set_system_reduced`. When it is on (and the
policy is `System`), nothing moves: a value goes to its target at once, slides, fades and smooth
scrolling alike; a cycle stands still and a blink is steadily on; and `end_frame` asks for no
frame for any of them. Fades are not kept: the Windows setting this follows switches off every
animation of the system's own controls. Delays are kept, since a delay is a wait and not a
movement: a value with a delayed motion keeps its old value for the delay and then takes the new
one, with one wake-up at the end of the wait. `Animator::set_policy` is the application's switch
over it: `Policy::Always` animates whatever the system says, `Policy::Never` never does.

A delay too long for the clock (`.delayed(Duration::MAX)`) is a wait that never ends: the value
keeps its old value and `end_frame` asks for no frame for it, and `Motion::length` gives
`Duration::MAX`. `Motion::SNAP`, like every motion, is read only when the target changes: a snap
given with an unchanged target leaves a tween or spring in flight alone (with reduced motion, the
one in flight arrives).

`platform::reduces_motion()`, which existed, is unchanged (macOS only, false elsewhere), since
the applications call it.

## Tests

`cargo test -p mdshared-ui anim` and the tests of the widgets above. On a `ManualClock`: easing
end points, monotony and mirror pairs; the bezier solver against a fine sampling of the curve
(within 1e-4, twelve curves including the Fluent ones and two that overshoot) and against the
values browsers give for the CSS keywords; tween progress; a new target on the way (no jump);
springs (no overshoot when critical, one overshoot when not, the same path for any step size,
settling, nonsense input); the deadline (none when idle, one frame interval while moving, the end
of a delay while waiting); forgetting of names not asked for; the stall guard; reduced motion and
the policy; the blends of a snapshot against values computed by hand; page switches; `Transient`
and `pop`; the painter's offset (pixel-identical drawing, moved, at scales 1, 1.25 and 1.5); and
for each widget that the animated drawing at rest is the plain drawing pixel for pixel.

Added with the review: a kept tween and a kept spring arrive and the frames stop; a snap, reduced
motion and a tooltip keep their delay; a tween, a spring and a `SmoothScroll` at 5e7 move in
steps smaller than an `f32` has there and end exactly on target; an `Id` reads the value its
name reads; an `Rgb` spring stops at most two frames after the colour read stops changing; the
checked bezier refuses six kinds of wrong curve and the constant form never gives a value that
is not a number; a page switch under a resize, a new scale or a moved region ends with the frame
equal to the new page drawn plainly, for all three effects; the example above as a doc-test;
`cycle`, `Periodic` (against a second way of computing it, on 2000 times from a fixed-seed LCG)
and `blink` (150 frames for a minute of blinking, each deadline at the next change); the spinner
and the busy bar at a phase pixel for pixel as a copy of the old code drew them; and the logic of
the Linux reading (`LastGood`: no answer keeps the last one, the wake is called on a change
only, one reading at a time) on every system.

Added with the second review: a wake given after the first answer is called once at once (and a
replacing one too); an answer older than the one kept is dropped; no reading starts while a
listener is active; `.delayed(Duration::MAX)` neither panics nor asks for a frame, as a tween, a
snap and under reduced motion; a snap with an unchanged target leaves a tween and a spring in
flight moving. The two-targets `debug_assert` is tested in debug
builds only.
