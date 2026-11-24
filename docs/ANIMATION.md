# Animation (`mdshared_ui::anim`)

How the shared toolkit animates, and what an application does to use it. Added on 2026-11-24.
Everything here is opt-in: an application that calls none of it draws and behaves as before.

**Not verified:** nothing here has run in a window. The module is covered by unit tests on a
clock moved by hand (below); how the motions look and feel, and the wiring into a real winit
loop shown under "The window loop", have not been seen by anyone. macOS and Linux were not run;
the Linux build was checked by `cargo check` for the target only.

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

Time is the frames' time, from a `Clock`: `SystemClock` in an application, `ManualClock` in a
test or for a render that must come out the same every time. A frame never moves the animations
by more than 50 ms (`set_max_step`), unless the animator itself asked to sleep longer: after a
stall or a minute minimised, what was in flight goes on from where it was and does not jump.

## The pieces

| Piece | What it is |
|---|---|
| `Animator` | The values in flight of one window. `begin_frame(now)`, then `value`, `value_from`, `toggle`, `presence`, `set`, `get`, `keep`, `forget` while drawing, then `end_frame() -> Option<Instant>` |
| `Motion` | How a value moves: `Motion::SNAP`, `Motion::tween(duration, easing)`, `Motion::spring(spring)`, and `.delayed(duration)` |
| `Easing` | Linear, the in / out / in-out curves (quad, cubic, quint, sine, expo), `OutBack`, and `CubicBezier(x1, y1, x2, y2)` with the CSS meaning; `Easing::EASE` and the other CSS keywords |
| `Spring` | Stiffness and damping, or `Spring::response(seconds, damping_ratio)`; `SMOOTH` (the default, critically damped), `SNAPPY`, `BOUNCY`; a settle threshold. Stepped by its exact solution, so any frame time gives the same path |
| `Animatable` | What can move: `f32`, `(f32, f32)`, `Rect`, `Rgb`, `[f32; 4]` |
| `fluent` | The Windows 11 timings and curves, and motions made of them (below) |
| `Snapshot` | A saved rectangle of the bitmap, and its `crossfade`, `slide_fade` and `push` with what was drawn there since |
| `PageTransition` | A page or tab switch of a region, driven by a `Snapshot` of the old page |
| `Transient<T>` | A slot for a thing that comes and goes, which keeps it while its exit plays |
| `pop` | Draws anything faded as a whole and moved into place: the entrance and exit of a popup |
| `Policy`, `set_system_reduced` | Reduced motion (below) |

The painter gained what these need: `Painter::offset` / `restore_offset` (move everything drawn
by whole device pixels, so text stays sharp), and `Painter::snapshot` / `fade` (give a group of
drawing calls one opacity: save the pixels, draw, blend the saved ones back). There are no
layers and no scaling; a popup moves and fades, as the Windows ones do.

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
  an application that knows its monitor's rate sets it from that.
- An application that repaints only the damaged part of its window either treats a frame the
  animator asked for as damage of what is animating, or calls `anim.keep(id)` for the names in
  the parts it skipped. A name not asked for and not kept starts fresh, at its target, which is
  never wrong, only without motion.
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
// State: page_switch: PageTransition::new("content")

// When the switch is decided (the bitmap still holds the old page):
let p = Painter::new(&mut bitmap, &mut gfx, scale);
page_switch.begin(&mut anim, &p, content_rect, PageEffect::SlideFade { from: Edge::Bottom, distance: 24.0 });
state.page = next;

// Every frame, after the content region is drawn and before anything that lies over it:
draw_page(&mut p, content_rect, state.page);
page_switch.apply(&mut anim, &mut bitmap);
```

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
```

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
| `overlay::draw_backdrop` | `draw_backdrop_presence(p, pal, w, h, t)` | The dimming follows the dialog's presence |
| `overlay::draw_dialog_card` | `pop(p, dialog_bounds(card), t, (0.0, DIALOG_RISE), …)` | A dialog fades and rises as one thing |
| `menu::Menu::draw_in` | `Menu::draw_in_presence(p, pal, area, t)` | The menu fades and drops 6 px into place |

Not done, and how each would be: the settings form (`settings::Form`) draws its own switches,
ticks and scrollbar and has no animator passed in, so it is unchanged; an application can still
wrap the whole form in `pop` for its entrance. The bars of `notice` (update, crash, offer) take
their height from the application's layout, which can animate that height with `anim.value`. The
command palette, the tree's disclosure, the tab strip and the text field's caret have no motion.

## Reduced motion

`platform::motion_reduced()` reads the system's wish:

| System | What is read |
|---|---|
| Windows | `SPI_GETCLIENTAREAANIMATION`: Settings, Accessibility, Visual effects, "Animation effects" |
| macOS | `NSWorkspace.accessibilityDisplayShouldReduceMotion`: Accessibility, Display, "Reduce motion" |
| Linux | The settings portal: `org.freedesktop.appearance` `reduced-motion`, else `org.gnome.desktop.interface` `enable-animations`, else `org.kde.kdeglobals.KDE` `AnimationDurationFactor` at 0. Half a second to answer, cached for a second; false without a portal |

The application passes it to `Animator::set_system_reduced`. When it is on (and the policy is
`System`), every value is its target at once: slides, fades and smooth scrolling alike, and
`end_frame` asks for no frame. Fades are not kept: the Windows setting this follows switches off
every animation of the system's own controls. `Animator::set_policy` is the application's switch
over it: `Policy::Always` animates whatever the system says, `Policy::Never` never does.

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
