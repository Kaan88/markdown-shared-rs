//! The settings file layer: `key=value` lines under a `[section]` header, text escaping that keeps
//! a value on one line (paths included, byte for byte on Unix), an atomic save and the per-user
//! data folder. What the keys mean is up to the application.

use std::io;
use std::path::{Path, PathBuf};

/// The per-user application data folder for `app`: `%LOCALAPPDATA%\<app>` on Windows,
/// `$XDG_DATA_HOME/<app>` or `~/.local/share/<app>` elsewhere. `None` if no base is known.
pub fn data_dir(app: &str) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.map(|b| b.join(app))
}

/// `settings.ini` in [`data_dir`].
pub fn default_settings_path(app: &str) -> Option<PathBuf> {
    data_dir(app).map(|d| d.join("settings.ini"))
}

/// The `(key, value)` pair of each line that has an `=`. The key is trimmed, the value is not.
/// Section headers and other lines are skipped.
pub fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value))
}

/// Builds the text of a settings file.
pub struct Writer {
    out: String,
}

impl Writer {
    /// Starts with a `[section]` header line.
    pub fn new(section: &str) -> Writer {
        Writer {
            out: format!("[{section}]\n"),
        }
    }

    /// Adds `key=value`. The value must already be one line: see [`escape`] and [`encode_path`].
    pub fn line(&mut self, key: &str, value: &str) {
        self.out.push_str(key);
        self.out.push('=');
        self.out.push_str(value);
        self.out.push('\n');
    }

    pub fn finish(self) -> String {
        self.out
    }
}

/// Writes through a temporary file and rename, so a crash cannot leave half a settings file.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        let (valid, invalid) = match std::str::from_utf8(rest) {
            Ok(v) => (v, &[][..]),
            Err(e) => {
                let (a, b) = rest.split_at(e.valid_up_to());
                (std::str::from_utf8(a).unwrap_or_default(), b)
            }
        };
        for c in valid.chars() {
            match c {
                '%' | '\n' | '\r' => out.push_str(&format!("%{:02X}", c as u32)),
                c => out.push(c),
            }
        }
        match invalid.first() {
            Some(b) => {
                out.push_str(&format!("%{b:02X}"));
                rest = &invalid[1..];
            }
            None => rest = &[],
        }
    }
    out
}

fn unescape_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%')
            .then(|| b.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| {
                Some(char::from(h[0]).to_digit(16)? * 16 + char::from(h[1]).to_digit(16)?)
            });
        match hex {
            Some(v) => {
                out.push(v as u8);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    out
}

pub fn escape(s: &str) -> String {
    escape_bytes(s.as_bytes())
}

pub fn unescape(s: &str) -> String {
    String::from_utf8_lossy(&unescape_bytes(s)).into_owned()
}

#[cfg(unix)]
pub fn encode_path(p: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    escape_bytes(p.as_os_str().as_bytes())
}

#[cfg(unix)]
pub fn decode_path(s: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(unescape_bytes(s)))
}

#[cfg(not(unix))]
pub fn encode_path(p: &Path) -> String {
    escape(&p.to_string_lossy())
}

#[cfg(not(unix))]
pub fn decode_path(s: &str) -> PathBuf {
    PathBuf::from(unescape(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_round_trips_line_breaks_and_percent() {
        let text = "a%b\nc\r\nd";
        let escaped = escape(text);
        assert!(!escaped.contains(['\n', '\r']));
        assert_eq!(unescape(&escaped), text);
        assert_eq!(unescape("100%"), "100%", "a stray percent is kept");
    }

    #[test]
    fn paths_round_trip() {
        let path = PathBuf::from("dir with 50% and = signs").join("a.md");
        assert_eq!(decode_path(&encode_path(&path)), path);
    }

    #[test]
    fn writer_and_pairs_agree() {
        let mut w = Writer::new("app");
        w.line("zoom", "120");
        w.line("tab", &encode_path(Path::new("x.md")));
        let text = w.finish();
        assert!(text.starts_with("[app]\n"));
        let got: Vec<_> = pairs(&text).collect();
        assert_eq!(got, [("zoom", "120"), ("tab", "x.md")]);
    }

    #[test]
    fn a_save_replaces_the_file_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("mdshared-settings-{}", std::process::id()));
        let path = dir.join("sub").join("settings.ini");
        write_atomic(&path, "a=1\n").unwrap();
        write_atomic(&path, "a=2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a=2\n");
        assert!(!path.with_extension("ini.tmp").exists());
        _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_data_folder_is_named_after_the_application() {
        if let Some(p) = default_settings_path("some-app") {
            assert!(p.ends_with(Path::new("some-app").join("settings.ini")));
        }
    }
}
