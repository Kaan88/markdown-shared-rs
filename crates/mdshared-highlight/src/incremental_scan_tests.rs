use super::*;
use std::time::Duration;

struct Text(Vec<String>);

impl Lines for Text {
    fn len_lines(&self) -> usize {
        self.0.len()
    }
    fn line(&self, index: usize, out: &mut String) {
        out.clear();
        out.push_str(&self.0[index]);
    }
}

fn text(source: &str) -> Text {
    Text(source.split('\n').map(str::to_owned).collect())
}

fn far() -> Instant {
    Instant::now() + Duration::from_secs(120)
}

fn bytes(text: &Text) -> usize {
    text.0.iter().map(|l| l.len() + 1).sum()
}

fn spans(language: Language, dark: bool, text: &Text) -> Vec<Vec<Span>> {
    let mut h = LineHighlighter::new(language, dark, text.len_lines(), bytes(text));
    h.spans(text, 0..text.len_lines(), far()).unwrap()
}

const SAMPLES: [(&str, &str); 4] = [
    (
        "kotlin",
        "package a\n/* open\n  still /* in */ comment */\nval s = \"\"\"\nraw ${x}\n\"\"\"\nfun f() = 1 // c\n",
    ),
    (
        "swift",
        "import X\n/* open\n  still */\nlet s = #\"\"\"\nraw \\#(x)\n\"\"\"#\nfunc f() {} // c\n",
    ),
    (
        "powershell",
        "<# open\n   still #>\n$h = @'\nraw\n'@\nfunction F { 1 } # c\n",
    ),
    (
        "dockerfile",
        "FROM a\nRUN x \\\n  y\nRUN <<EOT\nbody\nEOT\nCMD [\"a\"] # c\n",
    ),
];

#[test]
fn scanned_languages_are_found_by_token_and_path() {
    let by_path = |p: &str, first: &str| Language::for_path(Path::new(p), first);
    for (token, name) in [
        ("kotlin", "Kotlin"),
        ("kt", "Kotlin"),
        ("kts", "Kotlin"),
        ("swift", "Swift"),
        ("powershell", "PowerShell"),
        ("pwsh", "PowerShell"),
        ("ps1", "PowerShell"),
        ("psm1", "PowerShell"),
        ("psd1", "PowerShell"),
        ("dockerfile", "Dockerfile"),
        ("docker", "Dockerfile"),
        ("Containerfile", "Dockerfile"),
    ] {
        assert_eq!(Language::by_token(token).unwrap().name(), name, "{token}");
    }
    assert_eq!(Language::by_token("kt"), Language::by_token("kotlin"));
    assert_ne!(Language::by_token("kt"), Language::by_token("swift"));
    for (path, name) in [
        ("Main.kt", "Kotlin"),
        ("build.gradle.kts", "Kotlin"),
        ("App.swift", "Swift"),
        ("a/b/setup.ps1", "PowerShell"),
        ("M.psm1", "PowerShell"),
        ("M.psd1", "PowerShell"),
        ("Dockerfile", "Dockerfile"),
        ("docker/Dockerfile.dev", "Dockerfile"),
        ("Containerfile", "Dockerfile"),
        ("x.dockerfile", "Dockerfile"),
    ] {
        assert_eq!(by_path(path, "").unwrap().name(), name, "{path}");
        assert_eq!(by_path(path, "").unwrap().limits(), Limits::CODE);
    }
    assert_eq!(
        by_path("run", "#!/usr/bin/env pwsh").unwrap().name(),
        "PowerShell"
    );
    assert_eq!(by_path("notes.txt", ""), None);
}

#[test]
fn scanned_languages_give_the_viewers_colours_line_by_line() {
    for (name, source) in SAMPLES {
        for dark in [false, true] {
            let language = Language::by_token(name).unwrap();
            let text = text(source);
            let got = spans(language, dark, &text);
            let expected = crate::tokens(source, Some(name), dark).unwrap();
            let mut offset = 0;
            let mut coloured = 0;
            for (index, line) in text.0.iter().enumerate() {
                let mut at = 0;
                for span in &got[index] {
                    assert_eq!(span.start as usize, at, "{name} contiguous");
                    at = span.end as usize;
                    if span.color != foreground(dark) {
                        coloured += 1;
                    }
                    for byte in span.start..span.end {
                        let token = expected
                            .iter()
                            .find(|t| t.range.contains(&(offset + byte as usize)))
                            .unwrap();
                        assert_eq!(span.color, token.color, "{name} line {index} byte {byte}");
                    }
                }
                if !line.is_empty() {
                    assert_eq!(at, line.len(), "{name} covers {line:?}");
                }
                offset += line.len() + 1;
            }
            assert!(coloured >= 4, "{name}");
        }
    }
}

#[test]
fn scanned_languages_keep_the_incremental_guarantees() {
    for (name, sample) in SAMPLES {
        let language = Language::by_token(name).unwrap();
        let source = sample.repeat(600 / sample.matches('\n').count() + 1);
        let old = text(&source);
        let total = bytes(&old);
        let fresh = |t: &Text| LineHighlighter::new(language, false, t.len_lines(), total);
        let whole = spans(language, false, &old);
        // A resume from a checkpoint equals one pass.
        let mut h = fresh(&old);
        while h.advance(&old, far()) {}
        assert!(h.done());
        assert_eq!(h.checkpoint_count(), (old.len_lines() - 1) / 16 + 1);
        for start in [320, 321, 480] {
            let got = h.spans(&old, start..start + 40, far()).unwrap();
            assert_eq!(got[..], whole[start..start + 40], "{name} from {start}");
        }
        // An edit that opens a comment, string or continuation: rerunning equals a fresh run.
        for (line, replacement) in [
            (100, "/* open"),
            (37, "\"\"\""),
            (50, "<# x"),
            (200, "RUN \\"),
            (16, "@\""),
        ] {
            let mut h = fresh(&old);
            while h.advance(&old, far()) {}
            let mut lines = old.0.clone();
            lines[line] = replacement.into();
            let new = Text(lines);
            h.invalidate_from(line, new.len_lines(), 0);
            assert!(h.frontier() <= line);
            let rerun = h.spans(&new, 0..new.len_lines(), far()).unwrap();
            assert_eq!(rerun, spans(language, false, &new), "{name} edit at {line}");
        }
        // The theme switch keeps the state and changes the colours.
        let mut h = fresh(&old);
        let light = h.spans(&old, 0..30, far()).unwrap();
        h.set_dark(true);
        let dark = h.spans(&old, 0..30, far()).unwrap();
        assert_ne!(light, dark, "{name}");
        assert_eq!(dark[..], spans(language, true, &old)[..30]);
        // A deadline already past still moves one line on.
        let mut h = fresh(&old);
        assert!(h.advance(&old, Instant::now()));
        assert_eq!(h.frontier(), 1);
        // A line over the limit is plain and leaves the state.
        let long = Text(vec!["x".into(), "y".repeat(2000), "z".into()]);
        assert!(spans(language, false, &long)[1].is_empty(), "{name}");
        // A file over the limit is plain.
        let big = LineHighlighter::new(language, false, 10, Limits::CODE.max_file_bytes + 1);
        assert!(big.is_plain());
    }
}

#[test]
fn hostile_lines_through_the_incremental_path_are_fast() {
    let start = Instant::now();
    for (name, line) in [
        ("kotlin", "\"${".repeat(300)),
        ("swift", "\"\\(".repeat(300)),
        ("powershell", "\"$(".repeat(300)),
        ("dockerfile", "\"${".repeat(300)),
    ] {
        let language = Language::by_token(name).unwrap();
        let t = Text(vec![line.clone(); 3000]);
        assert_eq!(spans(language, false, &t).len(), 3000);
    }
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
}
