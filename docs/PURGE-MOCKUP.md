# Purge mockup

Purge is an uninstaller with a WinUI interface (`github.com/Kaan88/purge`, read at `027af22`).
`crates/mdshared-ui/examples/purge_mockup.rs` redraws it with this toolkit: Purge's layout, the
toolkit's palettes, widgets and fonts.

```sh
cargo run --release -p mdshared-ui --example purge_mockup
```

The pictures are still. Nothing takes input and no window opens. All data is made up.

**Not seen by a person.** The agent that wrote the example looked at the PNGs. Nobody else has.

## Screens and files

The window is Purge's default, 1360 by 860 logical pixels. The files are in `target/specimen/` at
the repository root, also when `CARGO_TARGET_DIR` points elsewhere. `-150` is scale 1.5.

| Screen | What it shows | Files in `target/specimen/` |
| --- | --- | --- |
| Programs | Header and Uninstall, tabs, search, filter, sort, list with column header and groups, details card, status strip | `purge-programs-light.png`, `purge-programs-dark.png`, `purge-programs-light-150.png`, `purge-programs-dark-150.png` |
| Programs, filter menu | The same with the Filter menu open | `purge-programs-filter-light.png`, `purge-programs-filter-dark.png`, `purge-programs-filter-light-150.png`, `purge-programs-filter-dark-150.png` |
| Programs, sort menu | The same with the Sort menu open | `purge-programs-sort-light.png`, `purge-programs-sort-dark.png`, `purge-programs-sort-light-150.png`, `purge-programs-sort-dark-150.png` |
| Uninstall confirmation | Dialog: steps, command, signer, still-running warning, quiet mode, restore point | `purge-confirm-light.png`, `purge-confirm-dark.png`, `purge-confirm-light-150.png`, `purge-confirm-dark-150.png` |
| Administrator permission | The dialog before the first elevation | `purge-elevation-light.png`, `purge-elevation-dark.png`, `purge-elevation-light-150.png`, `purge-elevation-dark-150.png` |
| Uninstall pipeline, running | Program card, progress steps, still running, safety net | `purge-pipeline-light.png`, `purge-pipeline-dark.png`, `purge-pipeline-light-150.png`, `purge-pipeline-dark-150.png` |
| Uninstall pipeline, next | All steps done and the card that offers the next program | `purge-pipeline-next-light.png`, `purge-pipeline-next-dark.png`, `purge-pipeline-next-light-150.png`, `purge-pipeline-next-dark-150.png` |
| Leftovers, review | Verdict bar, grouped items, selected item, footer | `purge-leftovers-light.png`, `purge-leftovers-dark.png`, `purge-leftovers-light-150.png`, `purge-leftovers-dark-150.png` |
| Leftovers, waiting | The code of the security prompt | `purge-leftovers-waiting-light.png`, `purge-leftovers-waiting-dark.png`, `purge-leftovers-waiting-light-150.png`, `purge-leftovers-waiting-dark-150.png` |
| Leftovers, result | What moved, what is held, what did not move | `purge-leftovers-result-light.png`, `purge-leftovers-result-dark.png`, `purge-leftovers-result-light-150.png`, `purge-leftovers-result-dark-150.png` |
| History and undo | Timeline, list by day, detail of one cleanup | `purge-history-light.png`, `purge-history-dark.png`, `purge-history-light-150.png`, `purge-history-dark-150.png` |
| Quarantine | Volume card, warning, cleanups, the "Delete now" flyout | `purge-quarantine-light.png`, `purge-quarantine-dark.png`, `purge-quarantine-light-150.png`, `purge-quarantine-dark-150.png` |
| Settings | Four cards | `purge-settings-light.png`, `purge-settings-dark.png`, `purge-settings-light-150.png`, `purge-settings-dark-150.png` |

## What comes from the toolkit

`table` (column header), `sections` (grouped lists), `select` (ticked programs), `textfield`,
`seltext` (paths and commands), `segments`, `dropdown`, `menu`, `toggle`, `check`, `link`,
`expander`, `infobar`, `flyout`,
`overlay` (dialog and its buttons), `slider` (progress and spinner), `pane` (row fill), `scroll`,
`chrome` (window buttons), `glyph`, `theme`. `rows` and `tokens` are not used.

## Gaps

The example draws these itself from boxes, lines and text.

- Push buttons outside a dialog. Only the dialog buttons exist. There is no icon button, no subtle
  button and no button with an icon and a label. The example puts an icon on a dialog button.
- A button that opens a menu (WinUI's DropDownButton): Sort, and the Filter and "···" buttons,
  which open menus too. The example draws a label and a chevron and opens `menu::Menu::around`
  the button; nothing ties the two together or draws the chevron.
- A text field with a leading icon (the search box). The example draws the frame and the
  magnifier itself and puts a `textfield` beside the icon.
- Tooltips on controls. `overlay::Bubble::tip` draws a bubble, but nothing places it by a control
  or times it; Purge gives its icon buttons tooltips. None is drawn here.
- A navigation rail with icons, counts and badges.
- A title bar with a pill in it.
- A command bar, or an overflow button.
- Cards and dividers.
- Chips (Safe, Likely, Restorable and so on).
- A check box with a third, mixed state. `check::Tick` is on or off.
- A progress stepper.
- A determinate ring. `Painter::arc` draws it, but there is no widget.
- A bar split into several colours (the verdict bar).
- A small size bar inside a list cell.
- A timeline strip.
- The program tile with an initial.
- Wrapping a path at a backslash. The paths and commands in the details cards are `seltext` with
  `Wrap::Anywhere`: a word too wide for the card breaks where the card ends, which can be inside
  a name ("CurrentVers" / "ion" in the registry key), not after the last backslash that fits, as
  the example's own wrapping did before.
- Icons: shield, lock, funnel, person, archive box and gear. The example has its own path data.
- Mica. The title bar and rail are a flat colour.

## Widgets that looked wrong or did not fit

- `theme::DARK.danger` is hard to read on dark surfaces: by calculation about 2.6 to 1 on the
  card colour. Fixed for the InfoBar and what takes its colours from it: on a dark palette that is
  not high contrast, `infobar::severity_colors` gives an error `infobar::DARK_ERROR_INK` (WinUI's
  critical foreground, about 7 to 1 on the dark page), and the large leftovers line ("~1.3 GB
  cache and profile") takes that ink. `DARK.danger` itself is unchanged; it is the close button's
  hover fill.
- `segments::draw_selector` looks like a switch with a filled segment. Purge's tabs are plain
  with an underline. Counts have to be part of the label.
- `infobar` has a close cross unless `closable(false)` is set. Purge's bars have none. Its action
  is a filled button on a line of its own, so the bar is taller than WinUI's.
- `table::draw_header` draws a divider after a column with no title (the tick and tile column).
- `overlay::dialog_card` takes a height from the caller and the dialog body does not scroll. The
  confirmation is 622 high. At Purge's smallest window (760 by 520) it would not fit.
- `flyout` takes its content size from the caller. The example sets it by hand.
- `expander::draw_header` is a quiet grey heading. WinUI's expander is a bordered box, and this one
  is easy to read as a caption.

## What is not certain about Purge's layout

- It was read from the XAML and the code. Purge was not built or run. Sizes that WinUI sets by
  default (type sizes, control heights, button padding) are estimates.
- The rail icons are chosen here.
- The "Administrator" pill is drawn on the pipeline screens only, to show it. In Purge it depends
  on the process being elevated, not on the page.
- Made up or filled in: program rows, history and quarantine rows, evidence lines, the reasons
  under leftovers, the pipeline step details and the result panel's lines. The sample data does
  not agree between screens in every detail.

Not drawn: the overflow menu, child rows under a program, narrow windows (below 1000,
900 and 640), empty and error states, the quarantine's "Choose items" panel, history's bars for
interrupted cleanups, and the security prompt itself (a native Windows dialog).
