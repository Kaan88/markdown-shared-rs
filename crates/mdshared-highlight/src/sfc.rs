//! Vue and Svelte single-file components: HTML with `<script>` (TypeScript), `<style>` (CSS, or
//! SCSS and Less by `lang`) and a template. Vue's template has `{{ expression }}` and directive
//! attributes (`v-if`, `:prop`, `@event`, `#slot`) whose quoted values are expressions; Svelte's
//! has `{expression}`, the block tags `{#if}` `{:else}` `{/if}` `{@html}`, `{...}` in tags and
//! directives such as `on:click` and `bind:value`.
//!
//! The same line-at-a-time design as the other scanners: [`Sfc`] is what one line hands the next
//! (which part of the file it is in, the open tag, an open quote, the brace depth), a small `Copy`
//! value, and every zone is walked once, left to right. The embedded languages reuse the
//! TypeScript and style scanners of [`generic_at`] over a slice of the line, so nothing here
//! rescans text: the closing delimiter of a zone is searched for once, and the expression up to it
//! is painted once.
//!
//! This is not an HTML parser. An unquoted `>` in an attribute value, a `<` in text that looks
//! like a tag, and expressions with strings that run over lines are read in the simplest way that
//! never loops or backs up.

use super::{
    CSS_SPEC, Colors, Lang, Painter, STYLE_SPEC, Spec, State, TS_SPEC, find, generic_at, ident_end,
};

/// Where in the file a line starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Zone {
    /// Template text between tags.
    #[default]
    Text,
    /// Inside `<!-- -->`.
    Comment,
    /// Inside an open or closing tag, between attributes.
    Tag,
    /// Inside a quoted attribute value that runs over lines.
    Value,
    /// Vue `{{ }}`.
    Mustache,
    /// Svelte `{ }`, at a brace depth.
    Braces,
    /// Between `<script>` and `</script>`.
    Script,
    /// Between `<style>` and `</style>`.
    Style,
}

/// What kind of tag the tag zone is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Other,
    Script,
    Style,
    Close,
}

/// The language of a `<style>` block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sheet {
    #[default]
    Css,
    Scss,
    Less,
}

/// What the expression scanner carries over a line break: an open block comment or a template
/// literal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Nested {
    #[default]
    Normal,
    Block,
    Str(u8),
}

impl Nested {
    fn state(self) -> State {
        match self {
            Nested::Normal => State::Normal,
            Nested::Block => State::Block(1),
            Nested::Str(hashes) => State::Str { hashes },
        }
    }

    fn of(state: &State) -> Nested {
        match state {
            State::Block(_) => Nested::Block,
            State::Str { hashes } => Nested::Str(*hashes),
            _ => Nested::Normal,
        }
    }
}

/// The state a component's scanner carries from one line to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sfc {
    zone: Zone,
    kind: Kind,
    sheet: Sheet,
    /// The quote of an open attribute value, and whether the value is an expression.
    quote: u8,
    expr: bool,
    /// Svelte: unclosed braces, and whether they began in a tag rather than in text.
    depth: u16,
    in_tag: bool,
    nested: Nested,
}

/// Which scanner an expression is read by.
#[derive(Clone, Copy)]
enum Dialect {
    Ts,
    Sheet(Sheet),
}

impl Dialect {
    fn parts(self) -> (&'static Spec, Lang) {
        match self {
            Dialect::Ts => (&TS_SPEC, Lang::TypeScript),
            Dialect::Sheet(Sheet::Css) => (&CSS_SPEC, Lang::Scss),
            Dialect::Sheet(Sheet::Scss) => (&STYLE_SPEC, Lang::Scss),
            Dialect::Sheet(Sheet::Less) => (&STYLE_SPEC, Lang::Less),
        }
    }
}

/// Paints `line[from..end]` as `dialect`, with `nested` as the carry-over (read only when `from`
/// is 0, as in [`generic_at`]) and updated.
fn code(
    dialect: Dialect,
    c: &Colors,
    line: &str,
    from: usize,
    end: usize,
    nested: &mut Nested,
    p: &mut Painter,
) {
    let (spec, lang) = dialect.parts();
    let mut state = nested.state();
    generic_at(spec, lang, c, &line[..end], from, &mut state, p);
    *nested = Nested::of(&state);
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// The end of a tag or attribute name.
fn name_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'-' | b'_' | b'.' | b':'))
    {
        i += 1;
    }
    i
}

/// The first `</name` at or after `from` that is a whole tag name, compared without case.
fn find_close(b: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let mut at = from;
    while let Some(k) = find(b, at, b"</") {
        let start = k + 2;
        let end = start + name.len();
        if b.get(start..end)
            .is_some_and(|w| w.eq_ignore_ascii_case(name))
            && !b
                .get(end)
                .is_some_and(|x| x.is_ascii_alphanumeric() || *x == b'-')
        {
            return Some(k);
        }
        at = k + 2;
    }
    None
}

/// The `}` that closes a Svelte expression whose braces are `depth` deep at `from`, skipping
/// quoted strings. `None` when the line ends first (`depth` is then the depth at its end), or at
/// a `//` comment.
fn brace_end(b: &[u8], from: usize, depth: &mut u32) -> Option<usize> {
    let mut j = from;
    while j < b.len() {
        match b[j] {
            b'{' => {
                *depth += 1;
                j += 1;
            }
            b'}' => {
                *depth -= 1;
                if *depth == 0 {
                    return Some(j);
                }
                j += 1;
            }
            quote @ (b'"' | b'\'' | b'`') => {
                j += 1;
                while j < b.len() && b[j] != quote {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                j += 1;
            }
            b'/' if b.get(j + 1) == Some(&b'/') => return None,
            _ => j += 1,
        }
    }
    None
}

/// Svelte's attribute prefixes that make `prefix:name` a directive.
fn svelte_directive(name: &str) -> bool {
    name.split_once(':').is_some_and(|(prefix, _)| {
        matches!(
            prefix,
            "on" | "bind"
                | "class"
                | "style"
                | "use"
                | "transition"
                | "in"
                | "out"
                | "animate"
                | "let"
        )
    })
}

/// A quoted Svelte attribute value, `"a {b} c"`, whose quotes are at `quote` and `close`: the
/// string painted as a string and each `{...}` in it as an expression.
fn svelte_string(c: &Colors, line: &str, quote: usize, close: usize, p: &mut Painter) {
    let b = &line.as_bytes()[..close];
    let mut seg = quote;
    let mut j = quote + 1;
    while j < close {
        if b[j] != b'{' {
            j += 1;
            continue;
        }
        let mut depth = 1;
        let Some(end) = brace_end(b, j + 1, &mut depth) else {
            break;
        };
        p.paint(seg, j, c.string);
        code(Dialect::Ts, c, line, j + 1, end, &mut Nested::Normal, p);
        seg = end + 1;
        j = end + 1;
    }
    p.paint(seg, close + 1, c.string);
}

/// One line. The spans painted cover it, and `st` is left for the next.
pub(crate) fn sfc_line(lang: Lang, c: &Colors, line: &str, st: &mut Sfc, p: &mut Painter) {
    let svelte = lang == Lang::Svelte;
    let n = line.len();
    let mut i = 0;
    while i < n {
        i = match st.zone {
            Zone::Text => text(svelte, c, line, i, st, p),
            Zone::Comment => match find(line.as_bytes(), i, b"-->") {
                Some(k) => {
                    p.paint(i, k + 3, c.comment);
                    st.zone = Zone::Text;
                    k + 3
                }
                None => {
                    p.paint(i, n, c.comment);
                    n
                }
            },
            Zone::Tag => tag(svelte, c, line, i, st, p),
            Zone::Value => value(c, line, i, st, p),
            Zone::Mustache => {
                let found = find(line.as_bytes(), i, b"}}");
                code(
                    Dialect::Ts,
                    c,
                    line,
                    i,
                    found.unwrap_or(n),
                    &mut st.nested,
                    p,
                );
                match found {
                    Some(k) => {
                        st.zone = Zone::Text;
                        st.nested = Nested::Normal;
                        k + 2
                    }
                    None => n,
                }
            }
            Zone::Braces => {
                let mut depth = u32::from(st.depth).max(1);
                let found = brace_end(line.as_bytes(), i, &mut depth);
                code(
                    Dialect::Ts,
                    c,
                    line,
                    i,
                    found.unwrap_or(n),
                    &mut st.nested,
                    p,
                );
                match found {
                    Some(k) => {
                        st.zone = if st.in_tag { Zone::Tag } else { Zone::Text };
                        st.depth = 0;
                        st.nested = Nested::Normal;
                        k + 1
                    }
                    None => {
                        st.depth = depth.min(u32::from(u16::MAX)) as u16;
                        n
                    }
                }
            }
            Zone::Script | Zone::Style => {
                let (name, dialect): (&[u8], _) = if st.zone == Zone::Script {
                    (b"script", Dialect::Ts)
                } else {
                    (b"style", Dialect::Sheet(st.sheet))
                };
                let found = find_close(line.as_bytes(), i, name);
                code(dialect, c, line, i, found.unwrap_or(n), &mut st.nested, p);
                match found {
                    Some(k) => {
                        // The closing tag is read as a tag from here.
                        st.zone = Zone::Text;
                        st.nested = Nested::Normal;
                        k
                    }
                    None => n,
                }
            }
        };
    }
}

/// Template text from `i`: tags, comments, entities, and the expressions of the template syntax.
fn text(svelte: bool, c: &Colors, line: &str, i: usize, st: &mut Sfc, p: &mut Painter) -> usize {
    let b = line.as_bytes();
    let n = b.len();
    let mut j = i;
    while j < n {
        match b[j] {
            b'<' => {
                if b[j..].starts_with(b"<!--") {
                    return match find(b, j + 4, b"-->") {
                        Some(k) => {
                            p.paint(j, k + 3, c.comment);
                            k + 3
                        }
                        None => {
                            p.paint(j, n, c.comment);
                            st.zone = Zone::Comment;
                            n
                        }
                    };
                }
                let closing = b.get(j + 1) == Some(&b'/');
                let start = j + 1 + usize::from(closing);
                if b.get(start).is_some_and(u8::is_ascii_alphabetic) {
                    let end = name_end(b, start);
                    let name = &line[start..end];
                    st.kind = if closing {
                        Kind::Close
                    } else if name.eq_ignore_ascii_case("script") {
                        Kind::Script
                    } else if name.eq_ignore_ascii_case("style") {
                        Kind::Style
                    } else {
                        Kind::Other
                    };
                    st.sheet = Sheet::Css;
                    st.zone = Zone::Tag;
                    // A component (`<MyButton>`) reads as a type, an element as a tag.
                    let color = if b[start].is_ascii_uppercase() {
                        c.type_
                    } else {
                        c.variable
                    };
                    p.paint(start, end, color);
                    return end;
                }
                j += 1;
            }
            b'{' if !svelte && b.get(j + 1) == Some(&b'{') => {
                st.zone = Zone::Mustache;
                st.nested = Nested::Normal;
                return j + 2;
            }
            b'{' if svelte => {
                st.zone = Zone::Braces;
                st.depth = 1;
                st.in_tag = false;
                st.nested = Nested::Normal;
                // `{#if`, `{:else`, `{/each`, `{@html`: the sigil and the word are the keyword.
                if matches!(b.get(j + 1), Some(b'#' | b':' | b'/' | b'@')) {
                    let end = ident_end(b, j + 2);
                    p.paint(j + 1, end, c.keyword);
                    return end;
                }
                return j + 1;
            }
            b'&' => {
                // `&amp;` `&#169;`
                let mut end = j + 1;
                if b.get(end) == Some(&b'#') {
                    end += 1;
                }
                let first = end;
                while end < n && end - j < 12 && b[end].is_ascii_alphanumeric() {
                    end += 1;
                }
                if end > first && b.get(end) == Some(&b';') {
                    p.paint(j, end + 1, c.escape);
                    j = end + 1;
                } else {
                    j += 1;
                }
            }
            _ => j += 1,
        }
    }
    n
}

/// Inside a tag, from `i`: attributes, then the `>` that ends it.
fn tag(svelte: bool, c: &Colors, line: &str, i: usize, st: &mut Sfc, p: &mut Painter) -> usize {
    let b = line.as_bytes();
    let n = b.len();
    let mut j = i;
    while j < n {
        match b[j] {
            x if is_space(x) => j += 1,
            b'>' => {
                let empty = j > 0 && b[j - 1] == b'/';
                st.zone = match st.kind {
                    Kind::Script if !empty => Zone::Script,
                    Kind::Style if !empty => Zone::Style,
                    _ => Zone::Text,
                };
                st.nested = Nested::Normal;
                return j + 1;
            }
            b'/' | b'=' | b'"' | b'\'' => j += 1,
            b'{' if svelte => {
                st.zone = Zone::Braces;
                st.depth = 1;
                st.in_tag = true;
                st.nested = Nested::Normal;
                return j + 1;
            }
            _ => {
                let start = j;
                while j < n
                    && !is_space(b[j])
                    && !matches!(b[j], b'=' | b'>' | b'/' | b'"' | b'\'')
                    && !(svelte && b[j] == b'{')
                {
                    j += 1;
                }
                let name = &line[start..j];
                let directive = if svelte {
                    svelte_directive(name)
                } else {
                    matches!(b[start], b':' | b'@' | b'#') || name.starts_with("v-")
                };
                p.paint(start, j, if directive { c.keyword } else { c.annotation });
                let mut q = j;
                while q < n && is_space(b[q]) {
                    q += 1;
                }
                if b.get(q) != Some(&b'=') {
                    continue;
                }
                q += 1;
                while q < n && is_space(b[q]) {
                    q += 1;
                }
                let is_lang = name.eq_ignore_ascii_case("lang");
                // Vue's directive values are expressions; Svelte's `={...}` values are handled as
                // braces, and its quoted ones may hold `{...}`.
                let expr = directive && !svelte;
                match b.get(q) {
                    Some(&quote @ (b'"' | b'\'')) => match find(b, q + 1, &[quote]) {
                        Some(k) => {
                            if expr {
                                code(Dialect::Ts, c, line, q + 1, k, &mut Nested::Normal, p);
                            } else if svelte {
                                svelte_string(c, line, q, k, p);
                            } else {
                                p.paint(q, k + 1, c.string);
                            }
                            if is_lang {
                                set_sheet(st, &line[q + 1..k]);
                            }
                            j = k + 1;
                        }
                        None => {
                            st.zone = Zone::Value;
                            st.quote = quote;
                            st.expr = expr;
                            st.nested = Nested::Normal;
                            if expr {
                                code(Dialect::Ts, c, line, q + 1, n, &mut st.nested, p);
                            } else {
                                p.paint(q, n, c.string);
                            }
                            return n;
                        }
                    },
                    Some(b'{') if svelte => {
                        st.zone = Zone::Braces;
                        st.depth = 1;
                        st.in_tag = true;
                        st.nested = Nested::Normal;
                        return q + 1;
                    }
                    Some(_) => {
                        let mut end = q;
                        while end < n && !is_space(b[end]) && b[end] != b'>' {
                            end += 1;
                        }
                        p.paint(q, end, c.string);
                        if is_lang {
                            set_sheet(st, &line[q..end]);
                        }
                        j = end;
                    }
                    None => j = n,
                }
            }
        }
    }
    n
}

/// `lang="scss"` on a `<style>`.
fn set_sheet(st: &mut Sfc, value: &str) {
    if st.kind != Kind::Style {
        return;
    }
    let value = value.trim();
    st.sheet = if value.eq_ignore_ascii_case("scss") {
        Sheet::Scss
    } else if value.eq_ignore_ascii_case("less") {
        Sheet::Less
    } else {
        Sheet::Css
    };
}

/// The rest of an attribute value that began on an earlier line, from `i`.
fn value(c: &Colors, line: &str, i: usize, st: &mut Sfc, p: &mut Painter) -> usize {
    let n = line.len();
    let found = find(line.as_bytes(), i, &[st.quote]);
    let end = found.unwrap_or(n);
    if st.expr {
        code(Dialect::Ts, c, line, i, end, &mut st.nested, p);
    } else {
        p.paint(i, found.map_or(n, |k| k + 1), c.string);
    }
    match found {
        Some(k) => {
            st.zone = Zone::Tag;
            st.nested = Nested::Normal;
            k + 1
        }
        None => n,
    }
}
