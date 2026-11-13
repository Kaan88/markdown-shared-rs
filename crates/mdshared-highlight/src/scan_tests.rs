use super::*;
use crate::{theme, tokens};
use std::time::Instant;

fn colour_of(source: &str, name: &str, needle: &str, dark: bool) -> [u8; 3] {
    let tokens = tokens(source, Some(name), dark).expect(name);
    let at = source.find(needle).unwrap_or_else(|| panic!("{needle}"));
    tokens.iter().find(|t| t.range.contains(&at)).unwrap().color
}

fn plain(dark: bool) -> [u8; 3] {
    scope_color(theme(dark), "source")
}

/// Each `coloured` needle's first byte must come out in a colour other than the theme's plain
/// one, each `plain` needle's in exactly it; the tokens must tile the source.
fn check(name: &str, source: &str, coloured: &[&str], plain_needles: &[&str]) {
    for dark in [false, true] {
        for needle in coloured {
            assert_ne!(
                colour_of(source, name, needle, dark),
                plain(dark),
                "{name} {needle}"
            );
        }
        for needle in plain_needles {
            assert_eq!(
                colour_of(source, name, needle, dark),
                plain(dark),
                "{name} {needle}"
            );
        }
        let all = tokens(source, Some(name), dark).unwrap();
        assert_eq!(all[0].range.start, 0);
        assert_eq!(all.last().unwrap().range.end, source.len());
        for pair in all.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start);
        }
    }
}

const KOTLIN: &str = "package app\n\n@file:JvmName(\"Main\")\n/* block\n   /* nested */ still */\n@Serializable\ndata class User(val name: String, var age: Int = 0x1F)\n\nfun greet(user: User): String? {\n    val s = \"Hi ${user.name} and $x\\n\" // tail\n    val raw = \"\"\"\n    multi ${1 + 2}\n    \"\"\"\n    val c = 'x'\n    return if (user.age > 1_000L) null else s\n}\n";

#[test]
fn kotlin_is_highlighted() {
    check(
        "kotlin",
        KOTLIN,
        &[
            "package",
            "@file:JvmName",
            "block",
            "still",
            "@Serializable",
            "data",
            "User(val",
            "val name",
            "String,",
            "0x1F",
            "greet",
            "\"Hi",
            "$x",
            "\\n",
            "// tail",
            "multi",
            "'x'",
            "return",
            "1_000L",
            "null",
        ],
        &["user: User", "= 0", "1 + 2"],
    );
    assert_eq!(
        colour_of(KOTLIN, "kotlin", "multi", false),
        colour_of(KOTLIN, "kotlin", "\"Hi", false)
    );
    // After the raw string closes, code is code again.
    assert_ne!(
        colour_of(KOTLIN, "kotlin", "val c", false),
        colour_of(KOTLIN, "kotlin", "multi", false)
    );
    assert_ne!(
        colour_of(KOTLIN, "kotlin", "greet", false),
        colour_of(KOTLIN, "kotlin", "fun", false)
    );
}

const SWIFT: &str = "import Foundation\n\n@main\nstruct App: Codable {\n    var n: Int = 0b101\n    let s = \"a \\(n + 1) b\\t\" // c\n    let r = #\"raw \\#(n) \"q\" \\n\"#\n    let m = \"\"\"\n      multi \\(n)\n      \"\"\"\n    /* x /* y */ z */\n    func run() async throws -> Bool { return nil == nil || true }\n    #if DEBUG\n    #endif\n}\n";

#[test]
fn swift_is_highlighted() {
    check(
        "swift",
        SWIFT,
        &[
            "import", "@main", "struct", "App", "Int", "0b101", "\"a ", "\\(", "\\t", "// c",
            "#\"raw", "\\#(n)", "multi", " x ", " z ", "func", "run", "async", "Bool", "nil",
            "true", "#if",
        ],
        &["n + 1"],
    );
    // In a raw string `\n` is no escape, so it matches the string around it.
    assert_eq!(
        colour_of(SWIFT, "swift", "\\n\"#", false),
        colour_of(SWIFT, "swift", "\"q", false)
    );
    assert_ne!(
        colour_of(SWIFT, "swift", "multi", false),
        colour_of(SWIFT, "swift", "func", false)
    );
}

const POWERSHELL: &str = "<# help\n   more #>\n[CmdletBinding()]\nparam([string]$Name = 'x''y', [int]$N = 0x10)\n# comment\nfunction Get-Thing {\n    $items = Get-ChildItem -Path $env:TEMP -Recurse | Where-Object { $_.Length -gt 1kb }\n    Write-Host \"Hello $Name and $($items.Count) `\"q`\" ${weird name}\"\n    $h = @\"\ntext $Name\n\"@\n    if ($true -and $null -ne $x) { return $false }\n}\n";

#[test]
fn powershell_is_highlighted() {
    check(
        "powershell",
        POWERSHELL,
        &[
            "help",
            "more",
            "[CmdletBinding",
            "param",
            "[string]",
            "$Name",
            "'x''y'",
            "0x10",
            "# comment",
            "function",
            "Get-Thing",
            "$items",
            "Get-ChildItem",
            "-Path",
            "$env:TEMP",
            "-Recurse",
            "Where-Object",
            "-gt",
            "1kb",
            "Write-Host",
            "\"Hello",
            "`\"",
            "${weird name}",
            "text $Name",
            "if",
            "-and",
            "$null",
            "return",
        ],
        &["items.Count) ", " = Get"],
    );
    // The here-string closes at `"@`, and what follows is code.
    assert_ne!(
        colour_of(POWERSHELL, "powershell", "text", false),
        colour_of(POWERSHELL, "powershell", "if", false)
    );
    // Keywords are case-insensitive.
    assert_ne!(colour_of("IF ($a) { }\n", "ps1", "IF", false), plain(false));
}

const DOCKERFILE: &str = "# syntax=docker/dockerfile:1\nFROM --platform=$BUILDPLATFORM rust:1.80 AS build\nARG VERSION=1.0\nENV PATH=\"/opt/bin:${PATH}\" \\\n    LANG=C\nrun cargo build \\\n  # a comment inside\n  --release && echo '$notvar'\nRUN <<EOT\nset -e\nmake install\nEOT\nCOPY --chown=app:app . /app\nCMD [\"/app/run\", \"--flag\"]\nonbuild ADD x y\n";

#[test]
fn dockerfile_is_highlighted() {
    check(
        "dockerfile",
        DOCKERFILE,
        &[
            "# syntax",
            "FROM",
            "--platform",
            "$BUILDPLATFORM",
            "AS",
            "ARG",
            "VERSION",
            "ENV",
            "\"/opt",
            "${PATH}",
            "run",
            "# a comment",
            "--release",
            "'$notvar'",
            "<<EOT",
            "set -e",
            "EOT\nCOPY",
            "COPY",
            "--chown",
            "CMD",
            "\"/app/run\"",
            "onbuild",
            "ADD",
        ],
        &["cargo build", "&&", "rust:1.80", "x y"],
    );
    // A continued line is not an instruction start.
    assert_eq!(
        colour_of("RUN a \\\n  FROM b\n", "docker", "FROM", false),
        plain(false)
    );
    // The heredoc body is a string up to its terminator.
    assert_eq!(
        colour_of(DOCKERFILE, "dockerfile", "make", false),
        colour_of(DOCKERFILE, "dockerfile", "set -e", false)
    );
    assert_ne!(
        colour_of(DOCKERFILE, "dockerfile", "COPY", false),
        colour_of(DOCKERFILE, "dockerfile", "make", false)
    );
}

#[test]
fn crlf_and_non_ascii_text_keep_ranges_whole() {
    for (name, source) in [
        (
            "kotlin",
            "val ad = \"\u{130}stanbul \u{1f600} ${\u{e7}}\" // t\u{fc}rk\u{e7}e\r\nval \u{540d}\u{524d} = '\u{e9}'\r\n",
        ),
        (
            "swift",
            "let ad = \"\u{130} \\(\u{e7}) \\\u{e7}\" // \u{65e5}\r\n",
        ),
        (
            "powershell",
            "$\u{e7} = \"\u{1f600} `\u{e7} $\u{e7}\" # \u{65e5}\r\n",
        ),
        (
            "dockerfile",
            "RUN echo \"\u{e7} \\\u{e7}\" \\\r\n  \u{65e5}\r\n",
        ),
    ] {
        let all = tokens(source, Some(name), false).unwrap();
        assert_eq!(all.last().unwrap().range.end, source.len(), "{name}");
        for pair in all.windows(2) {
            assert_eq!(pair[0].range.end, pair[1].range.start);
        }
        for t in &all {
            assert!(
                source.is_char_boundary(t.range.start),
                "{name} {:?}",
                t.range
            );
            assert!(source.is_char_boundary(t.range.end));
        }
    }
}

#[test]
fn hostile_input_is_scanned_in_linear_time() {
    let mb = 1 << 20;
    let cases: Vec<(&str, String)> = vec![
        ("kotlin", format!("val s = \"{}", "${".repeat(mb / 2))),
        (
            "kotlin",
            format!("val s = \"\"\"{}", "${x}$".repeat(mb / 5)),
        ),
        ("kotlin", "/*".repeat(mb / 2)),
        ("kotlin", "'".repeat(mb)),
        ("swift", format!("let s = \"{}", "\\(".repeat(mb / 2))),
        (
            "swift",
            format!("let s = #####\"{}", "\\#####(".repeat(mb / 8)),
        ),
        ("swift", "#".repeat(mb)),
        ("swift", "/*".repeat(mb / 2)),
        ("powershell", format!("\"{}", "$(".repeat(mb / 2))),
        ("powershell", format!("@\"\n{}", "`$(${".repeat(mb / 4))),
        ("powershell", "<#".repeat(mb / 2)),
        ("powershell", "[a".repeat(mb / 2)),
        ("powershell", "-".repeat(mb)),
        ("dockerfile", format!("RUN echo \"{}", "${".repeat(mb / 2))),
        ("dockerfile", "RUN <<A\n".repeat(mb / 8)),
        ("dockerfile", "\\\n".repeat(mb / 2)),
        ("dockerfile", "--".repeat(mb / 2)),
        ("dockerfile", "ENV a=b ".repeat(mb / 8)),
    ];
    // The scanner on whole megabytes, line by line.
    let start = Instant::now();
    for (name, source) in &cases {
        let colors = Colors::new(theme(false));
        let lang = Lang::from_token(name).unwrap();
        let mut state = State::default();
        let mut spans = Vec::new();
        for line in source.split('\n') {
            spans.clear();
            scan_line(lang, &colors, line, &mut state, &mut spans);
            let end = spans.last().map_or(0, |s| s.end as usize);
            assert_eq!(end, line.len(), "{name}");
        }
    }
    assert!(
        start.elapsed().as_secs_f64() < 5.0,
        "{:?} for {} megabyte-sized inputs",
        start.elapsed(),
        cases.len()
    );
    // And through the viewer's entry point, at its size cap.
    let start = Instant::now();
    for (name, source) in &cases {
        let cut: String = source.chars().take(200_000).collect();
        assert!(tokens(&cut, Some(name), true).is_some(), "{name}");
    }
    assert!(start.elapsed().as_secs_f64() < 3.0, "{:?}", start.elapsed());
}

#[test]
fn names_and_aliases_resolve() {
    for (token, lang) in [
        ("kotlin", Lang::Kotlin),
        ("Kotlin", Lang::Kotlin),
        ("kt", Lang::Kotlin),
        ("kts", Lang::Kotlin),
        ("swift", Lang::Swift),
        ("Swift", Lang::Swift),
        ("powershell", Lang::PowerShell),
        ("PowerShell", Lang::PowerShell),
        ("pwsh", Lang::PowerShell),
        ("ps1", Lang::PowerShell),
        ("psm1", Lang::PowerShell),
        ("psd1", Lang::PowerShell),
        ("dockerfile", Lang::Dockerfile),
        ("Dockerfile", Lang::Dockerfile),
        ("docker", Lang::Dockerfile),
        ("containerfile", Lang::Dockerfile),
        (" kotlin ", Lang::Kotlin),
    ] {
        assert_eq!(Lang::from_token(token), Some(lang), "{token}");
        assert!(tokens("x\n", Some(token), false).is_some(), "{token}");
    }
    for token in ["", "kot", "ps", "sh", "docker-compose", "swiftui", "rust"] {
        assert_eq!(Lang::from_token(token), None, "{token}");
    }
    for (name, lang) in [
        ("Dockerfile", Lang::Dockerfile),
        ("dockerfile", Lang::Dockerfile),
        ("Containerfile", Lang::Dockerfile),
        ("Dockerfile.dev", Lang::Dockerfile),
        ("Containerfile.prod", Lang::Dockerfile),
        ("app.dockerfile", Lang::Dockerfile),
        ("Main.kt", Lang::Kotlin),
        ("build.gradle.kts", Lang::Kotlin),
        ("App.swift", Lang::Swift),
        ("x.ps1", Lang::PowerShell),
        ("x.PSM1", Lang::PowerShell),
        ("x.psd1", Lang::PowerShell),
    ] {
        let path = std::path::Path::new(name);
        let found = Lang::from_path(
            path.file_name().and_then(|n| n.to_str()),
            path.extension().and_then(|e| e.to_str()),
        );
        assert_eq!(found, Some(lang), "{name}");
    }
    assert_eq!(Lang::from_path(Some("Dockerfiles"), None), None);
    assert_eq!(
        Lang::from_first_line("#!/usr/bin/env pwsh"),
        Some(Lang::PowerShell)
    );
    assert_eq!(Lang::from_first_line("#!/bin/sh"), None);
}
