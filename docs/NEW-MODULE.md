# Adding a module

The checklist for a new public module in any of the crates, and for a sizeable addition to one
that exists. The rules themselves, with their reasons, are in [../AGENTS.md](../AGENTS.md);
this page is the order to do things in and what "done" means.

## Before writing

1. **Look for it.** Read [CATALOGUE.md](CATALOGUE.md). If a module does half of what you need,
   extend it with a new function beside the old ones; do not start a second module for the
   same job.
2. **Pick the crate** by what the code needs, not by who will use it:

   | The code needs | Crate |
   |---|---|
   | A painter, a palette, a rectangle, a key or a pointer | `mdshared-ui` |
   | Files, threads, text, time, settings; no window system | `mdshared-app` |
   | Glyphs, fonts, the bitmap itself | `mdshared-text` |
   | Nothing of ours, and it could be used alone | its own crate, as `mdshared-term` and `mdshared-fetch` are |

   Something an application needs and no other could use (its own command table, its document
   model) stays in the application.
3. **Decide what it refuses.** Write the "Deliberately not here" list first. A module that
   takes on everything near its subject cannot be tested or kept.
4. **Check the dependency rule.** If the job seems to need a crate that is not in `Cargo.lock`,
   stop: can it be written here in a few hundred lines? If not, the owner decides, and the
   roadmap is where such a decision is recorded.

## The shape

- **A model** with no painter in it: a plain struct (derive `Clone`, `Debug`, `PartialEq`
  where they make sense; `Copy` when small), built by `new` and builder methods, with
  - questions that take the rectangle and answer (`rects`, `rect(i)`, `hit`, `cell_at`,
    `visible`): pure, usable in a test with nothing but numbers;
  - events that the application passes in and that return what happened (`press`, `drag`,
    `release`, `key`/`nav`/`step_by`, `wheel`): no callback, no stored closure;
  - where it applies, `to_setting() -> String` and `apply_setting(&str)` that carry the
    user's state through one line of the settings file, the second one accepting any text.
- **Drawing** as free functions `draw_*(p: &mut Painter, pal: &Palette, ..., &model, look)`
  that change nothing and record no hits. State that comes from the application's side (the
  pointer is over it, the keyboard is on it) arrives as a small `Look`/`State` struct or
  plain flags.
- **Motion** as a second function beside the plain one, `draw_*_animated(p, pal, anim, id,
  ...)`, that draws exactly the plain one's pixels at rest and under reduced motion. See
  [ANIMATION.md](ANIMATION.md).
- Constants for the sizes a caller lays out with (`HEADER_H`, `THUMB`, `SEGMENT_H`) are `pub`.
- Logical pixels in, logical pixels out. Colours only from the `Palette` passed in.

Model first, with its tests; then the drawing, with its pixel tests. A model that is hard to
test without a painter is not separated enough.

## Required before the commit

- [ ] A `//!` module doc: what it is, what the caller owns, what it deliberately does not do.
- [ ] A `///` on every public item, saying what the edge cases give.
- [ ] Tests for every public function, including: empty and zero-size input; NaN and
      infinity; a hand-damaged settings line, if it has one; both palettes for drawing; a
      second way of computing the same answer for any arithmetic that is easy to get wrong
      by one.
- [ ] For drawing: pixel assertions, and where there is an older function it could be
      compared with, "pixel for pixel the same".
- [ ] For anything a person must judge: an `#[ignore]`d sheet test writing a PNG to
      `target/specimen/`, and its command in the module's test doc.
- [ ] `pub mod` added in the crate's `lib.rs`, in alphabetical order, and the crate doc's
      one-line list updated if the module is a new kind of thing.
- [ ] No existing signature, public field or drawing changed (AGENTS.md, "Compatibility").
- [ ] **An entry in [CATALOGUE.md](CATALOGUE.md)**, in the right group: one line of purpose
      and the entry points.
- [ ] **A section in [FEATURES.md](FEATURES.md)** if the feature has design decisions worth
      keeping or things that were not verified; for a small module the catalogue line and the
      commit message are enough.
- [ ] If the work came from [ROADMAP.md](ROADMAP.md): the item marked done there, with the
      commit, and anything left over listed as a new item.
- [ ] README.md still true (it lists the crates and what each holds).
- [ ] The five CI commands pass (AGENTS.md, "Build, test, lint").
- [ ] One commit for the module, its message saying what was run and what was not.

## What a reviewer will look for

- Could the model be used by an application with a different look? If the palette or a font
  size leaks into the model, it could not.
- Does anything remember a rectangle between calls? It will be wrong after a resize.
- Is there a clock read, a thread started or a file opened inside something called from
  drawing? There must not be.
- Does an idle window stay idle? Anything that asks for frames must stop asking when it is at
  rest or not drawn.
- Does the doc say what was not verified?
