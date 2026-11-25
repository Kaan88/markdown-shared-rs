//! The commands of an application and the keys that run them: a table the application writes
//! once, the user's own keys over it, and the questions every shell asks of the two. Which
//! command does this key press mean? What does the menu print beside this item? Which keys are
//! given to two commands? What goes into the settings file?
//!
//! The table stays the application's ([`keys`](crate::keys) says so too): this module holds no
//! command of its own and runs none. [`Keymap::lookup`] gives back the id of a command and the
//! application acts on it, as it does for a click on a menu item with the same id.
//!
//! Shortcuts are written the way [`keys::parse`](crate::keys::parse) reads them ("Ctrl+Shift+F",
//! "F5"), several for one command separated by ", " ("Ctrl+W, Ctrl+F4"). A table is written once
//! with "Ctrl" as the primary modifier; the defaults go through
//! [`keys::for_platform`](crate::keys::for_platform), so the few shortcuts a Mac keeps for itself
//! move there, and [`Keymap::hint`] prints through [`keys::label`](crate::keys::label), so a Mac
//! shows its symbols.

use crate::keys::{self, Chord, KeyName, Mods};
use crate::palette::Entry;
use mdshared_app::settings::{self, Writer};

/// What starts the settings key of a user's shortcut: `key.<name>=<shortcuts>`.
pub const SETTING_PREFIX: &str = "key.";

/// One thing the application can do, as its table lists it. All of it is fixed text, so a table
/// is a `const` slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    /// The application's own number: what [`Keymap::lookup`] gives back, and the id of the
    /// command's menu item and palette entry.
    pub id: u32,
    /// The name the settings file knows the command by ("file.open"). It must never change once
    /// a version with it has shipped, and holds no `=` and no line break. Numbers may be
    /// renumbered between versions; names may not.
    pub name: &'static str,
    /// What the palette and a shortcut editor call it ("Open file…").
    pub title: &'static str,
    /// The application's section number, as [`palette::Entry::group`](Entry::group) means it.
    pub group: u8,
    /// The shortcuts it has until the user says otherwise, or "" for none.
    pub keys: &'static str,
}

impl Command {
    /// A command with no shortcut, in group 0.
    pub const fn new(id: u32, name: &'static str, title: &'static str) -> Command {
        Command {
            id,
            name,
            title,
            group: 0,
            keys: "",
        }
    }

    /// The same with these default shortcuts.
    pub const fn keys(mut self, keys: &'static str) -> Command {
        self.keys = keys;
        self
    }

    /// The same in this group.
    pub const fn group(mut self, group: u8) -> Command {
        self.group = group;
        self
    }
}

/// A key that more than one command has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The shortcut, as the first of the commands writes it.
    pub keys: String,
    /// The commands that have it, in the table's order. [`Keymap::lookup`] gives the first of
    /// them that the user bound, or else the first.
    pub ids: Vec<u32>,
}

/// One command's keys as they are now.
#[derive(Clone, Debug)]
struct Binding {
    /// The table's shortcuts on this platform, written and parsed.
    default_text: String,
    default: Vec<Chord>,
    /// The user's: `None` while the default stands, `Some` of nothing when the user took the
    /// shortcut away.
    user: Option<(String, Vec<Chord>)>,
}

impl Binding {
    fn chords(&self) -> &[Chord] {
        match &self.user {
            Some((_, chords)) => chords,
            None => &self.default,
        }
    }

    fn text(&self) -> &str {
        match &self.user {
            Some((text, _)) => text,
            None => &self.default_text,
        }
    }
}

/// The shortcuts of `text` that parse, written back in one spelling and joined by ", ", with
/// the chords themselves. A part that is not a shortcut is left out; an empty text gives
/// nothing.
fn read_keys(text: &str) -> (String, Vec<Chord>) {
    let mut chords: Vec<Chord> = Vec::new();
    for part in text.split(", ") {
        // A part's own spaces are not part of it, but a key that is a comma keeps its comma:
        // "Ctrl+,, F5" splits into "Ctrl+," and "F5".
        if let Some(chord) = keys::parse(part)
            && !chords.contains(&chord)
        {
            chords.push(chord);
        }
    }
    let text: Vec<String> = chords.iter().map(|c| chord_text(&c.key, c.mods)).collect();
    (text.join(", "), chords)
}

/// A key press written as a shortcut, the way [`keys::parse`](crate::keys::parse) reads it
/// back: "Ctrl+Shift+F", "Alt+Left", "Ctrl++", "F5". For a settings dialog that lets the user
/// press the keys of a command and passes the text to [`Keymap::bind`]; to show it, pass it
/// through [`keys::label`](crate::keys::label). The modifiers come in one order (Control,
/// Ctrl, Alt, Shift), an ASCII letter is in upper case, and the Mac's Control key is "Control"
/// (off the Mac [`Mods::control`] is never set).
pub fn chord_text(key: &KeyName, mods: Mods) -> String {
    let mut out = String::new();
    for (on, name) in [
        (mods.control, "Control+"),
        (mods.ctrl, "Ctrl+"),
        (mods.alt, "Alt+"),
        (mods.shift, "Shift+"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    match key {
        KeyName::Char(' ') => out.push_str("Space"),
        // Only what the parser folds back: it lowers ASCII and leaves other letters alone.
        KeyName::Char(c) => out.push(c.to_ascii_uppercase()),
        KeyName::F(n) => out.push_str(&format!("F{n}")),
        KeyName::Tab => out.push_str("Tab"),
        KeyName::Escape => out.push_str("Escape"),
        KeyName::Enter => out.push_str("Enter"),
        KeyName::Space => out.push_str("Space"),
        KeyName::Backspace => out.push_str("Backspace"),
        KeyName::Delete => out.push_str("Delete"),
        KeyName::Home => out.push_str("Home"),
        KeyName::End => out.push_str("End"),
        KeyName::PageUp => out.push_str("PageUp"),
        KeyName::PageDown => out.push_str("PageDown"),
        KeyName::Up => out.push_str("Up"),
        KeyName::Down => out.push_str("Down"),
        KeyName::Left => out.push_str("Left"),
        KeyName::Right => out.push_str("Right"),
    }
    out
}

/// Whether a press of one chord's keys is taken by the other, either way round: what makes two
/// commands collide. It is [`Chord::matches`], so "Ctrl++" and "Ctrl+=" collide as they do when
/// typed.
fn same_press(a: &Chord, b: &Chord) -> bool {
    a.matches(&b.key, b.mods) || b.matches(&a.key, a.mods)
}

/// The application's commands with the keys they have now: the table's, with the user's over
/// them. See the [module](self).
///
/// A table in which two commands share an id or a name is the application's mistake: the first
/// of them is the one every call by id or name finds. [`Keymap::table_faults`] says so in a
/// test.
#[derive(Clone, Debug)]
pub struct Keymap {
    commands: Vec<Command>,
    bindings: Vec<Binding>,
    /// Lines of the settings file that name no command of this table, kept as they were read
    /// and written back: a file a newer version wrote is not stripped by an older one.
    unknown: Vec<(String, String)>,
}

impl Keymap {
    /// The keymap of `table` with every command at its default.
    pub fn new(table: &[Command]) -> Keymap {
        let bindings = table
            .iter()
            .map(|c| {
                // The Mac's moves are per shortcut, so a list goes through one by one.
                let moved: Vec<&str> = c.keys.split(", ").map(keys::for_platform).collect();
                let (default_text, default) = read_keys(&moved.join(", "));
                Binding {
                    default_text,
                    default,
                    user: None,
                }
            })
            .collect();
        Keymap {
            commands: table.to_vec(),
            bindings,
            unknown: Vec::new(),
        }
    }

    /// What is wrong with the table, one line each, for the application's own test: an id or a
    /// name used twice, a name the settings file could not hold, a default shortcut that does
    /// not parse. Empty for a sound table. Conflicts between defaults are not faults (a table
    /// may give one key to two commands of different contexts): see
    /// [`conflicts`](Self::conflicts).
    pub fn table_faults(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, c) in self.commands.iter().enumerate() {
            if self.commands[..i].iter().any(|o| o.id == c.id) {
                out.push(format!("id {} is used twice ({})", c.id, c.name));
            }
            if self.commands[..i].iter().any(|o| o.name == c.name) {
                out.push(format!("name {} is used twice", c.name));
            }
            if c.name.is_empty() || c.name.contains(['=', '\n', '\r']) || c.name.trim() != c.name {
                out.push(format!("name {:?} cannot be a settings key", c.name));
            }
            for part in c.keys.split(", ").filter(|p| !p.is_empty()) {
                if keys::parse(keys::for_platform(part)).is_none() {
                    out.push(format!("{}: {part:?} is not a shortcut", c.name));
                }
            }
        }
        out
    }

    /// The table, in its order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.commands.iter().position(|c| c.id == id)
    }

    /// The command with this id.
    pub fn command(&self, id: u32) -> Option<&Command> {
        self.index(id).map(|i| &self.commands[i])
    }

    /// The command a key press means, or `None` when it is no shortcut. Among several commands
    /// with that key, one the user bound wins over one that has it by default (the user's word
    /// is the later one), and otherwise the first in the table.
    pub fn lookup(&self, key: &KeyName, mods: Mods) -> Option<u32> {
        let has = |b: &Binding| b.chords().iter().any(|c| c.matches(key, mods));
        let all = || self.commands.iter().zip(&self.bindings);
        all()
            .find(|(_, b)| b.user.is_some() && has(b))
            .or_else(|| all().find(|(_, b)| has(b)))
            .map(|(c, _)| c.id)
    }

    /// The shortcuts `id` has now, as written ("Ctrl+W, Ctrl+F4"); "" for none or for an id
    /// that is not in the table.
    pub fn keys(&self, id: u32) -> &str {
        self.index(id).map_or("", |i| self.bindings[i].text())
    }

    /// The shortcuts `id` has by default on this platform.
    pub fn default_keys(&self, id: u32) -> &str {
        self.index(id)
            .map_or("", |i| &self.bindings[i].default_text)
    }

    /// Whether the user changed the shortcuts of `id` (also to none).
    pub fn is_changed(&self, id: u32) -> bool {
        self.index(id)
            .is_some_and(|i| self.bindings[i].user.is_some())
    }

    /// What a menu item prints for `id`: its first shortcut the way the platform spells it, or
    /// "" when it has none. For `menu::Item::hint`.
    pub fn hint(&self, id: u32) -> &'static str {
        match self.keys(id).split(", ").next() {
            Some(first) if !first.is_empty() => keys::label(first),
            _ => "",
        }
    }

    /// All the shortcuts of `id` the way the platform spells them, for the palette and for a
    /// list of shortcuts: "Ctrl+W, Ctrl+F4".
    pub fn hint_all(&self, id: u32) -> &'static str {
        match self.keys(id) {
            "" => "",
            all => keys::label(all),
        }
    }

    /// Gives `id` the shortcuts of `keys` in place of the ones it has. Returns whether that
    /// was done: not for an id that is not in the table, and not when `keys` holds no shortcut
    /// that parses (to take a shortcut away, [`unbind`](Self::unbind)). Parts that do not
    /// parse are left out. Shortcuts equal to the default put the command back at its default.
    /// Nothing is taken from another command that has the key: see
    /// [`conflicts`](Self::conflicts).
    pub fn bind(&mut self, id: u32, keys: &str) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        let (text, chords) = read_keys(keys);
        if chords.is_empty() {
            return false;
        }
        let b = &mut self.bindings[i];
        b.user = (chords != b.default).then_some((text, chords));
        true
    }

    /// Leaves `id` with no shortcut at all, whatever its default. Returns false for an id that
    /// is not in the table.
    pub fn unbind(&mut self, id: u32) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        let b = &mut self.bindings[i];
        b.user = (!b.default.is_empty()).then(|| (String::new(), Vec::new()));
        true
    }

    /// Puts `id` back at its default. Returns whether anything changed.
    pub fn reset(&mut self, id: u32) -> bool {
        self.index(id)
            .is_some_and(|i| self.bindings[i].user.take().is_some())
    }

    /// Puts every command back at its default, and forgets the lines kept for commands this
    /// table does not have.
    pub fn reset_all(&mut self) {
        for b in &mut self.bindings {
            b.user = None;
        }
        self.unknown.clear();
    }

    /// The keys more than one command has, each with its commands, in the order of the first
    /// command of each. Empty when every key means one thing. A shortcut editor shows these;
    /// nothing here resolves them, since which of two commands should lose is the user's call.
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut found: Vec<(Chord, Conflict)> = Vec::new();
        for (c, b) in self.commands.iter().zip(&self.bindings) {
            for chord in b.chords() {
                match found.iter_mut().find(|(seen, _)| same_press(seen, chord)) {
                    Some((_, conflict)) => {
                        if !conflict.ids.contains(&c.id) {
                            conflict.ids.push(c.id);
                        }
                    }
                    None => found.push((
                        chord.clone(),
                        Conflict {
                            keys: chord_text(&chord.key, chord.mods),
                            ids: vec![c.id],
                        },
                    )),
                }
            }
        }
        found
            .into_iter()
            .map(|(_, conflict)| conflict)
            .filter(|conflict| conflict.ids.len() > 1)
            .collect()
    }

    /// One palette entry per command, in the table's order: the title, the group, and the
    /// shortcuts as the hint. The application filters out what does not apply now and sets
    /// `enabled` and an icon as it likes.
    pub fn entries(&self) -> impl Iterator<Item = Entry> + '_ {
        self.commands.iter().map(|c| Entry {
            hint: self.hint_all(c.id).to_string(),
            group: c.group,
            ..Entry::new(c.id, c.title)
        })
    }

    /// Writes the user's shortcuts as settings lines, `key.<name>=<shortcuts>`, one per command
    /// that is not at its default, in the table's order, then the lines kept for commands this
    /// table does not have. A command the user left without a shortcut is written with an empty
    /// value. Nothing is written for a keymap at its defaults.
    ///
    /// ```text
    /// key.file.close=Ctrl+W, Ctrl+F4
    /// key.view.zoom-in=
    /// ```
    pub fn write(&self, w: &mut Writer) {
        for (c, b) in self.commands.iter().zip(&self.bindings) {
            if let Some((text, _)) = &b.user {
                w.line(
                    &format!("{SETTING_PREFIX}{}", c.name),
                    &settings::escape(text),
                );
            }
        }
        for (name, value) in &self.unknown {
            w.line(&format!("{SETTING_PREFIX}{name}"), value);
        }
    }

    /// The same lines as a text of their own, without a section header: for a test, or a file
    /// that holds only shortcuts.
    pub fn to_settings(&self) -> String {
        let mut w = Writer::new("");
        self.write(&mut w);
        let text = w.finish();
        // Drop the header line of the empty section.
        text.split_once('\n')
            .map_or("", |(_, rest)| rest)
            .to_string()
    }

    /// Takes the user's shortcuts from the text of a settings file: every `key.<name>=` line in
    /// it, the last one of a name winning; other lines are not this module's and are skipped.
    /// The keymap is put back at its defaults first, so the result is what the text says and
    /// nothing else.
    ///
    /// A hand-edited file cannot break it:
    ///
    /// - a line for a name this table does not have is kept as it is and written back by
    ///   [`write`](Self::write);
    /// - an empty value is "no shortcut";
    /// - of a value with several shortcuts, the ones that do not parse are left out;
    /// - a value none of whose shortcuts parse changes nothing: the command keeps its default,
    ///   and the line is gone at the next write.
    pub fn read(&mut self, text: &str) {
        self.reset_all();
        for (key, value) in settings::pairs(text) {
            let Some(name) = key.strip_prefix(SETTING_PREFIX) else {
                continue;
            };
            let Some(i) = self.commands.iter().position(|c| c.name == name) else {
                self.unknown.retain(|(n, _)| n != name);
                self.unknown.push((name.to_string(), value.to_string()));
                continue;
            };
            let id = self.commands[i].id;
            let value = settings::unescape(value);
            if value.trim().is_empty() {
                self.unbind(id);
            } else if !self.bind(id, &value) {
                self.reset(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::MAC;

    const OPEN: u32 = 1;
    const CLOSE: u32 = 2;
    const FIND: u32 = 3;
    const ZOOM_IN: u32 = 4;
    const ABOUT: u32 = 5;
    const REPLACE: u32 = 6;

    const TABLE: &[Command] = &[
        Command::new(OPEN, "file.open", "Open file…").keys("Ctrl+O"),
        Command::new(CLOSE, "file.close", "Close tab").keys("Ctrl+W, Ctrl+F4"),
        Command::new(FIND, "edit.find", "Find")
            .keys("Ctrl+F")
            .group(1),
        Command::new(ZOOM_IN, "view.zoom-in", "Zoom in")
            .keys("Ctrl++")
            .group(2),
        Command::new(ABOUT, "help.about", "About").group(3),
        Command::new(REPLACE, "edit.replace", "Replace")
            .keys("Ctrl+H")
            .group(1),
    ];

    fn ctrl() -> Mods {
        Mods {
            ctrl: true,
            ..Mods::default()
        }
    }

    fn ch(c: char) -> KeyName {
        KeyName::Char(c)
    }

    #[test]
    fn the_table_is_sound_and_its_defaults_are_found() {
        let map = Keymap::new(TABLE);
        assert_eq!(map.table_faults(), Vec::<String>::new());
        assert_eq!(map.commands().len(), 6);
        assert_eq!(map.command(FIND).map(|c| c.title), Some("Find"));
        assert_eq!(map.command(99), None);
        assert_eq!(map.lookup(&ch('o'), ctrl()), Some(OPEN));
        assert_eq!(map.lookup(&ch('w'), ctrl()), Some(CLOSE));
        assert_eq!(map.lookup(&KeyName::F(4), ctrl()), Some(CLOSE));
        // "Ctrl++" is typed with or without Shift, and as Ctrl+= on a US keyboard.
        assert_eq!(map.lookup(&ch('+'), ctrl()), Some(ZOOM_IN));
        assert_eq!(map.lookup(&ch('='), ctrl()), Some(ZOOM_IN));
        // Not a shortcut: the bare letter, another modifier, a key nobody has.
        assert_eq!(map.lookup(&ch('o'), Mods::default()), None);
        let shifted = Mods {
            shift: true,
            ..ctrl()
        };
        assert_eq!(map.lookup(&ch('o'), shifted), None);
        assert_eq!(map.lookup(&KeyName::F(9), Mods::default()), None);
        assert_eq!(map.keys(CLOSE), "Ctrl+W, Ctrl+F4");
        assert_eq!(map.keys(ABOUT), "");
        assert_eq!(map.keys(99), "");
        assert!(!map.is_changed(CLOSE));
        assert_eq!(map.conflicts(), vec![]);
        assert_eq!(map.to_settings(), "");
    }

    #[test]
    fn a_faulty_table_is_told_apart_in_a_test() {
        let bad = [
            Command::new(1, "a", "A").keys("Ctrl+Nope"),
            Command::new(1, "b", "B"),
            Command::new(2, "a", "A again").keys("Ctrl+K, Hyper+J"),
            Command::new(3, "x=y", "Equals"),
            Command::new(4, "", "Nameless"),
        ];
        let faults = Keymap::new(&bad).table_faults();
        assert_eq!(faults.len(), 6, "{faults:?}");
        assert!(faults[0].contains("Ctrl+Nope"));
        assert!(faults[1].contains("id 1"));
        assert!(faults[2].contains("name a"));
        assert!(faults[3].contains("Hyper+J"));
        // The default that does not parse is left out, the rest of the list stands, and the
        // first of two commands with one id is the one found.
        let map = Keymap::new(&bad);
        assert_eq!(map.keys(1), "");
        assert_eq!(map.keys(2), "Ctrl+K");
        assert_eq!(map.command(1).map(|c| c.name), Some("a"));
    }

    #[test]
    fn the_users_keys_replace_the_default_and_can_be_taken_away_and_put_back() {
        let mut map = Keymap::new(TABLE);
        assert!(map.bind(FIND, "ctrl+shift+f"));
        assert_eq!(map.keys(FIND), "Ctrl+Shift+F", "written in one spelling");
        assert!(map.is_changed(FIND));
        let shifted = Mods {
            shift: true,
            ..ctrl()
        };
        assert_eq!(map.lookup(&ch('f'), shifted), Some(FIND));
        assert_eq!(map.lookup(&ch('f'), ctrl()), None, "the default is gone");
        assert_eq!(map.default_keys(FIND), "Ctrl+F");

        // Several at once; what does not parse is left out, and a repeat counts once.
        assert!(map.bind(ABOUT, "F1, Bogus+Q, Alt+F1, f1"));
        assert_eq!(map.keys(ABOUT), "F1, Alt+F1");
        assert_eq!(map.lookup(&KeyName::F(1), Mods::default()), Some(ABOUT));
        // Nothing that parses, an unknown id: refused, and nothing changes.
        assert!(!map.bind(ABOUT, "Bogus+Q"));
        assert!(!map.bind(ABOUT, ""));
        assert!(!map.bind(99, "F2"));
        assert_eq!(map.keys(ABOUT), "F1, Alt+F1");

        // Taken away: no key runs it, and that is a change of its own.
        assert!(map.unbind(OPEN));
        assert_eq!(map.keys(OPEN), "");
        assert!(map.is_changed(OPEN));
        assert_eq!(map.lookup(&ch('o'), ctrl()), None);
        assert!(!map.unbind(99));
        // A command with no default that is unbound is simply at its default.
        assert!(map.unbind(ABOUT));
        assert!(!map.is_changed(ABOUT));

        // Put back, by reset and by binding the default again.
        assert!(map.reset(OPEN));
        assert!(!map.reset(OPEN), "nothing left to reset");
        assert!(!map.reset(99));
        assert_eq!(map.lookup(&ch('o'), ctrl()), Some(OPEN));
        assert!(map.bind(FIND, "Ctrl+F"));
        assert!(!map.is_changed(FIND));
        map.bind(CLOSE, "F9");
        map.reset_all();
        assert_eq!(map.to_settings(), "");
        assert_eq!(map.keys(CLOSE), "Ctrl+W, Ctrl+F4");
    }

    #[test]
    fn keys_given_to_two_commands_are_reported_and_the_users_command_wins() {
        let mut map = Keymap::new(TABLE);
        // The user's key against another command's default.
        map.bind(FIND, "Ctrl+O");
        assert_eq!(
            map.conflicts(),
            vec![Conflict {
                keys: "Ctrl+O".into(),
                ids: vec![OPEN, FIND],
            }]
        );
        assert_eq!(map.lookup(&ch('o'), ctrl()), Some(FIND), "the user's word");
        // Two of the user's keys against each other, and "+" against "=".
        map.bind(ABOUT, "F1, Ctrl+=");
        map.bind(REPLACE, "F1");
        let conflicts = map.conflicts();
        assert_eq!(conflicts.len(), 3, "{conflicts:?}");
        assert_eq!(conflicts[1].ids, vec![ZOOM_IN, ABOUT]);
        assert_eq!(conflicts[1].keys, "Ctrl++");
        assert_eq!(
            conflicts[2],
            Conflict {
                keys: "F1".into(),
                ids: vec![ABOUT, REPLACE],
            }
        );
        // Between two the user bound, the first in the table.
        assert_eq!(map.lookup(&KeyName::F(1), Mods::default()), Some(ABOUT));
        // Resolved by the user: one of them is given up.
        map.unbind(OPEN);
        map.reset(REPLACE);
        map.bind(ABOUT, "F1");
        assert_eq!(map.conflicts(), vec![]);
        // A command that lists one key twice does not conflict with itself.
        let twice = [Command::new(1, "a", "A").keys("Ctrl+K, Ctrl+K")];
        assert_eq!(Keymap::new(&twice).conflicts(), vec![]);
    }

    #[test]
    fn a_key_press_is_written_the_way_it_is_read_back() {
        let all = Mods {
            ctrl: true,
            shift: true,
            alt: true,
            control: false,
        };
        assert_eq!(chord_text(&ch('f'), all), "Ctrl+Alt+Shift+F");
        assert_eq!(chord_text(&ch('+'), ctrl()), "Ctrl++");
        assert_eq!(chord_text(&ch(','), ctrl()), "Ctrl+,");
        assert_eq!(chord_text(&ch(' '), ctrl()), "Ctrl+Space");
        assert_eq!(chord_text(&KeyName::F(12), Mods::default()), "F12");
        let keys = [
            ch('a'),
            ch('é'),
            ch('1'),
            ch('+'),
            ch('='),
            ch(','),
            ch('\\'),
            ch('['),
            KeyName::F(1),
            KeyName::F(24),
            KeyName::Tab,
            KeyName::Escape,
            KeyName::Enter,
            KeyName::Space,
            KeyName::Backspace,
            KeyName::Delete,
            KeyName::Home,
            KeyName::End,
            KeyName::PageUp,
            KeyName::PageDown,
            KeyName::Up,
            KeyName::Down,
            KeyName::Left,
            KeyName::Right,
        ];
        for key in &keys {
            for bits in 0..8u8 {
                let mods = Mods {
                    ctrl: bits & 1 != 0,
                    shift: bits & 2 != 0,
                    alt: bits & 4 != 0,
                    control: false,
                };
                let text = chord_text(key, mods);
                let back = keys::parse(&text).unwrap_or_else(|| panic!("{text:?} is not read"));
                assert_eq!((&back.key, back.mods), (key, mods), "{text}");
            }
        }
        // The Mac's Control key has a name of its own there, and is the primary one elsewhere.
        let control = Mods {
            control: true,
            ..Mods::default()
        };
        assert_eq!(chord_text(&KeyName::Tab, control), "Control+Tab");
        let back = keys::parse("Control+Tab").unwrap();
        assert_eq!(back.mods.control, MAC);
        assert_eq!(back.mods.ctrl, !MAC);
    }

    #[test]
    fn the_hint_is_the_first_shortcut_as_the_platform_spells_it() {
        let mut map = Keymap::new(TABLE);
        assert_eq!(map.hint(ABOUT), "");
        assert_eq!(map.hint_all(ABOUT), "");
        assert_eq!(map.hint(99), "");
        if MAC {
            assert_eq!(map.hint(OPEN), "\u{2318}O");
            assert_eq!(map.hint(CLOSE), "\u{2318}W");
            assert_eq!(map.hint_all(CLOSE), "\u{2318}W, \u{2318}F4");
            // The Mac keeps Command+H: Replace has moved, by `keys::for_platform`.
            assert_eq!(map.default_keys(REPLACE), "Ctrl+Alt+F");
        } else {
            assert_eq!(map.hint(OPEN), "Ctrl+O");
            assert_eq!(map.hint(CLOSE), "Ctrl+W");
            assert_eq!(map.hint_all(CLOSE), "Ctrl+W, Ctrl+F4");
            assert_eq!(map.default_keys(REPLACE), "Ctrl+H");
        }
        map.bind(OPEN, "F2, F3");
        assert_eq!(map.hint(OPEN), "F2");
        map.unbind(OPEN);
        assert_eq!(map.hint(OPEN), "");
    }

    #[test]
    fn the_palette_gets_one_entry_a_command_with_its_keys_as_the_hint() {
        let mut map = Keymap::new(TABLE);
        map.bind(ABOUT, "F1");
        let entries: Vec<Entry> = map.entries().collect();
        assert_eq!(entries.len(), TABLE.len());
        for (entry, command) in entries.iter().zip(TABLE) {
            assert_eq!(entry.id, command.id);
            assert_eq!(entry.label, command.title);
            assert_eq!(entry.group, command.group);
            assert_eq!(entry.hint, map.hint_all(command.id));
            assert!(entry.enabled);
        }
        assert_eq!(entries[4].hint, "F1");
        assert_eq!(entries[2].group, 1);
    }

    #[test]
    fn only_what_the_user_changed_is_written_and_it_reads_back_the_same() {
        let mut map = Keymap::new(TABLE);
        map.bind(CLOSE, "Ctrl+Q, F9");
        map.unbind(ZOOM_IN);
        map.bind(ABOUT, "Ctrl+,, Ctrl+=, Ctrl+\\, Ctrl+%");
        let text = map.to_settings();
        // The settings layer's own escaping: a percent sign is the one character it rewrites.
        assert_eq!(
            text,
            "key.file.close=Ctrl+Q, F9\n\
             key.view.zoom-in=\n\
             key.help.about=Ctrl+,, Ctrl+=, Ctrl+\\, Ctrl+%25\n"
        );
        let mut back = Keymap::new(TABLE);
        back.read(&text);
        for c in TABLE {
            assert_eq!(back.keys(c.id), map.keys(c.id), "{}", c.name);
            assert_eq!(back.is_changed(c.id), map.is_changed(c.id), "{}", c.name);
        }
        assert_eq!(back.to_settings(), text);
        assert_eq!(back.lookup(&ch('\\'), ctrl()), Some(ABOUT));
        assert_eq!(back.lookup(&ch(','), ctrl()), Some(ABOUT));
        assert_eq!(back.lookup(&ch('%'), ctrl()), Some(ABOUT));
        assert_eq!(back.lookup(&ch('='), ctrl()), Some(ABOUT));
        assert_eq!(back.lookup(&ch('+'), ctrl()), None, "Zoom in gave it up");

        // Inside an application's settings file, between its other lines.
        let mut w = Writer::new("mdtest");
        w.line("theme", "dark");
        map.write(&mut w);
        w.line("zoom", "110");
        let file = w.finish();
        assert!(file.starts_with("[mdtest]\ntheme=dark\nkey.file.close=Ctrl+Q, F9\n"));
        let mut from_file = Keymap::new(TABLE);
        from_file.read(&file);
        assert_eq!(from_file.to_settings(), text);
        // Reading is not adding: what the text does not say is back at its default.
        from_file.read("theme=dark\n");
        assert_eq!(from_file.to_settings(), "");
    }

    #[test]
    fn a_hand_edited_settings_file_does_no_harm() {
        let mut map = Keymap::new(TABLE);
        map.read(
            "[app]\n\
             key.file.open=Bogus+Q\n\
             key.file.close=Nope, F9, , ++, Ctrl+\n\
             key.edit.find=   \n\
             key.plugin.run=Ctrl+R\n\
             key.plugin.run=Ctrl+Shift+R\n\
             key.=F3\n\
             key.view.zoom-in\n\
             keyboard=qwerty\n\
             key.help.about=F1\n\
             key.help.about=F2\n\
             =\n\
             key.edit.replace=\\q\\\n",
        );
        // Nothing that parses: the default stays, and the line is not written back.
        assert_eq!(map.keys(OPEN), "Ctrl+O");
        assert!(!map.is_changed(OPEN));
        // The one part that parses is kept: a modifier with no key ("Ctrl+"), two keys ("++")
        // and an empty part are not shortcuts.
        assert_eq!(map.keys(CLOSE), "F9");
        // Blank: no shortcut.
        assert_eq!(map.keys(FIND), "");
        assert!(map.is_changed(FIND));
        // A line without "=" is no line; the last of two lines wins.
        assert_eq!(map.keys(ZOOM_IN), "Ctrl++");
        assert_eq!(map.keys(ABOUT), "F2");
        assert_eq!(map.keys(REPLACE), map.default_keys(REPLACE));
        // Names this table does not know are kept, the last of each, and written back.
        assert_eq!(
            map.to_settings(),
            "key.file.close=F9\n\
             key.edit.find=\n\
             key.help.about=F2\n\
             key.plugin.run=Ctrl+Shift+R\n\
             key.=F3\n"
        );
        // Long, strange and binary-looking input: no panic, nothing bound.
        let mut noise = String::new();
        for i in 0..2000u32 {
            let c = char::from_u32(i * 37 % 0x2fff + 1).unwrap_or('?');
            noise.push_str(&format!("key.file.open={c}{c}+{c}, \\x{i:02}\u{0}+\n"));
        }
        noise.push_str(&"key.file.close=".repeat(500));
        noise.push_str(&"+".repeat(10_000));
        let mut map = Keymap::new(TABLE);
        map.read(&noise);
        assert_eq!(map.keys(OPEN), "Ctrl+O");
        map.read("");
        assert_eq!(map.to_settings(), "");
    }
}
