//! Authenticate both ends before sending the existing application message. The client's
//! first flight is only a fresh nonce. Proofs bind both nonces and the speaker's role,
//! so a listener cannot echo a sender's proof or reuse one from an earlier connection.

use super::record::random;
use crate::sister::{MAX_TEXT, doc_id};
use crate::update::Sha256;
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;

type Block = [u8; 32];

fn proof(key: &Block, role: u8, client: &Block, server: &Block) -> Block {
    // HMAC-SHA256 with the 32-byte key padded to SHA-256's 64-byte block size.
    let hash = |pad, parts: &[&[u8]]| {
        let mut block = [pad; 64];
        for (byte, key) in block.iter_mut().zip(key) {
            *byte ^= key;
        }
        let mut hash = Sha256::new();
        hash.update(&block);
        for part in parts {
            hash.update(part);
        }
        hash.finish()
    };
    hash(0x5c, &[&hash(0x36, &[&[role], client, server])])
}

fn matches(actual: &Block, expected: Block) -> bool {
    // Keep each step opaque to the optimizer so it cannot replace this with an
    // early-exit byte comparison. Always compare all 32 bytes.
    actual
        .iter()
        .zip(expected)
        .fold(0, |acc, (a, b)| std::hint::black_box(acc | (a ^ b)))
        == 0
}

fn block(r: &mut impl Read) -> Option<Block> {
    let mut bytes = [0; 32];
    r.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

pub(super) fn challenge(r: &mut impl BufRead, w: &mut impl Write, key: &Block) -> Option<Block> {
    let client = random()?;
    w.write_all(&client).ok()?;
    let server = block(r)?;
    if !matches(&block(r)?, proof(key, b'S', &client, &server)) {
        return None;
    }
    Some(proof(key, b'C', &client, &server))
}

pub(super) fn reply(r: &mut impl BufRead) -> Option<String> {
    line(r, 32).map(|s| s.trim().to_owned())
}

#[cfg(test)]
fn exchange(
    r: &mut impl BufRead,
    w: &mut impl Write,
    key: &Block,
    message: &[u8],
) -> Option<String> {
    let proof = challenge(r, w, key)?;
    w.write_all(&proof).ok()?;
    w.write_all(message).ok()?;
    reply(r)
}

pub(super) fn authenticate(r: &mut impl Read, w: &mut impl Write, key: &Block) -> Option<()> {
    let client = block(r)?;
    let server = random()?;
    w.write_all(&[server, proof(key, b'S', &client, &server)].concat())
        .ok()?;
    matches(&block(r)?, proof(key, b'C', &client, &server)).then_some(())
}

const MAX_LINE: usize = 64 * 1024;
const MAX_HEADER: usize = 1024 * 1024;
const MAX_FIELDS: usize = 256;

/// Keep the sender's batches within the same line, header and field caps as the reader.
pub(super) fn send_paths(
    app: &str,
    paths: &[PathBuf],
    mut send: impl FnMut(&[u8]) -> bool,
) -> bool {
    let valid_line = |line: &str| line.len() < MAX_LINE && !line.contains(['\r', '\n']);
    if !valid_line(app) {
        return false;
    }
    let mut message = format!("{app}\n");
    let prefix = message.len();
    let mut fields = 0;
    for path in paths {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        let line = absolute.to_string_lossy();
        if !valid_line(&line) {
            return false;
        }
        if fields == MAX_FIELDS || message.len() + line.len() + 2 > MAX_HEADER {
            message.push('\n');
            if !send(message.as_bytes()) {
                return false;
            }
            message.truncate(prefix);
            fields = 0;
        }
        message.push_str(&line);
        message.push('\n');
        fields += 1;
    }
    message.push('\n');
    send(message.as_bytes())
}

/// A complete UTF-8 line; the limit includes the LF. EOF or a limit is never a terminator.
fn line(r: &mut impl BufRead, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    r.take(limit as u64).read_until(b'\n', &mut bytes).ok()?;
    (bytes.pop()? == b'\n').then_some(())?;
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes).ok()
}

pub(super) fn header(r: &mut impl BufRead, app: &str) -> Option<Vec<String>> {
    let mut header = r.take(MAX_HEADER as u64);
    if line(&mut header, MAX_LINE)? != app {
        return None;
    }
    let mut body = Vec::new();
    loop {
        let field = line(&mut header, MAX_LINE)?;
        if field.is_empty() {
            return Some(body);
        }
        if body.len() == MAX_FIELDS {
            return None;
        }
        body.push(field);
    }
}

/// The announced text length, or `Some(None)` if none was announced. Refuse malformed,
/// over-cap or conflicting declarations before any request goes to a receiver.
pub(super) fn checked_text_length(body: &[String]) -> Option<Option<usize>> {
    let mut length = None;
    for field in body {
        if field == "?text" {
            return None;
        }
        if let Some(value) = field.strip_prefix("?text=") {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let count = value.parse().ok()?;
            if count > MAX_TEXT || length.is_some_and(|first| first != count) {
                return None;
            }
            length = Some(count);
        }
    }
    Some(length)
}

/// Count the requests the header would produce, without cloning its shared strings. A
/// header must also fit the header byte cap after its strings are copied into each request.
/// As in `Request::parse`, the last value for a shared field wins, and only a document
/// keeps a title or supplies a destination when there are no path lines.
pub(super) fn bounded_destinations(body: &[String]) -> Option<usize> {
    let (mut paths, mut path_bytes, mut doc, mut title, mut from) = (0, 0, 0, 0, 0);
    for field in body {
        let path = if let Some(option) = field.strip_prefix('?') {
            let (key, value) = option.split_once('=').unwrap_or((option, ""));
            match key {
                "path" => Some(value),
                "doc" => {
                    doc = doc_id(value).map_or(0, str::len);
                    None
                }
                "title" => {
                    title = value.len();
                    None
                }
                "from" => {
                    from = value.len();
                    None
                }
                _ => None,
            }
        } else {
            Some(field.as_str())
        };
        if let Some(path) = path {
            paths += 1;
            path_bytes += path.len();
        }
    }
    let destinations = paths.max(usize::from(doc != 0));
    let shared = from + doc + if doc == 0 { 0 } else { title };
    (path_bytes + destinations.saturating_mul(shared) <= MAX_HEADER).then_some(destinations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::single::record::Record;
    use std::io::{BufReader, Cursor};

    #[test]
    fn batches_respect_the_byte_cap_and_stop_on_the_first_refusal() {
        let paths: Vec<_> = (0..80)
            .map(|i| PathBuf::from(format!("{i}-{}", "x".repeat(32 * 1024))))
            .collect();
        let mut lengths = Vec::new();
        assert!(!send_paths("app", &paths, |message| {
            assert!(message.len() <= MAX_HEADER);
            let body = header(&mut Cursor::new(message), "app").unwrap();
            assert!(bounded_destinations(&body).is_some());
            assert_eq!(
                body[0],
                std::path::absolute(&paths[lengths.iter().sum::<usize>()])
                    .unwrap_or_else(|_| paths[lengths.iter().sum::<usize>()].clone())
                    .to_string_lossy()
            );
            lengths.push(body.len());
            lengths.len() == 1
        }));
        assert_eq!(lengths.len(), 2);
        assert!(lengths.iter().sum::<usize>() < paths.len());
    }

    #[test]
    fn the_hmac_matches_a_known_answer_and_binds_both_nonces_and_the_role() {
        let key = std::array::from_fn(|i| i as u8);
        let client = std::array::from_fn(|i| i as u8 + 32);
        let server = std::array::from_fn(|i| i as u8 + 64);
        // Python hmac.digest(key, b'S' + client + server, 'sha256').
        let expected = [
            0x67, 0x60, 0xb2, 0x76, 0x1b, 0x9e, 0xa1, 0x2b, 0xd2, 0x9c, 0x6d, 0x4c, 0x40, 0x58,
            0xe3, 0x1c, 0x48, 0xa2, 0x2b, 0x76, 0xa6, 0xb5, 0x72, 0xe0, 0xa4, 0x03, 0x6e, 0xbf,
            0xa7, 0xe2, 0x4d, 0x79,
        ];
        assert!(matches(&expected, proof(&key, b'S', &client, &server)));
        assert!(!matches(&expected, proof(&key, b'C', &client, &server)));
        let mut changed_client = client;
        changed_client[0] ^= 1;
        assert!(!matches(
            &expected,
            proof(&key, b'S', &changed_client, &server)
        ));
        let mut changed_server = server;
        changed_server[31] ^= 1;
        assert!(!matches(
            &expected,
            proof(&key, b'S', &client, &changed_server)
        ));
        for i in 0..32 {
            let mut changed = expected;
            changed[i] ^= 1;
            assert!(!matches(&changed, expected));
        }
    }

    #[test]
    fn a_client_without_the_secret_is_refused() {
        // Valid frame sizes, but the peer knows no key. Authentication must stop before
        // the payload reader can inspect even the application's name.
        let mut input = Cursor::new([&[0x17; 64][..], b"app\n\n"].concat());
        assert!(authenticate(&mut input, &mut Vec::new(), &[0x31; 32]).is_none());
        assert_eq!(input.position(), 64);
    }

    #[test]
    fn the_owner_of_a_stale_port_receives_neither_paths_nor_text() {
        use crate::single::{directory, hold_lock, instance_record};
        let dir = std::env::temp_dir().join(format!("mdshared-reused-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.ini");
        let private = directory(&settings, true).unwrap();
        let (lock, _) = hold_lock(&private, std::thread::sleep)
            .unwrap()
            .publish(&private, 12345)
            .unwrap();
        let cached = instance_record(&settings).unwrap();
        drop(lock);
        assert!(
            instance_record(&settings).is_none(),
            "the port file outlived its owner"
        );
        // The harder race: the sender read the record just before the owner exited.
        // The peer at the saved port now answers, but does not know that listener's key.
        let mut written = Vec::new();
        // A valid-sized but incorrect proof, followed by the normal success reply.
        let mut peer = Cursor::new([&[0; 64][..], b"ok\n"].concat());
        assert!(
            exchange(
                &mut peer,
                &mut written,
                &cached.key,
                b"app\n/file\n?text=7\n\nunsaved"
            )
            .is_none()
        );
        assert_eq!(written.len(), 32, "only the fresh nonce travels");
        drop(private);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn oversized_or_incomplete_headers_are_rejected_as_a_whole() {
        let long = "x".repeat(MAX_LINE);
        assert!(header(&mut format!("{long}\n\n").as_bytes(), &long).is_none());
        assert!(header(&mut format!("app\nok\n{long}\n\n").as_bytes(), "app").is_none());
        assert!(
            header(
                &mut format!("app\n{}\n", "a\n".repeat(MAX_FIELDS + 1)).as_bytes(),
                "app"
            )
            .is_none()
        );
        let wide = ("x".repeat(MAX_LINE - 1) + "\n").repeat(MAX_HEADER / MAX_LINE);
        assert!(header(&mut format!("app\n{wide}\n").as_bytes(), "app").is_none());
        assert!(header(&mut b"app\nok\n".as_slice(), "app").is_none());
        assert!(header(&mut format!("app\n{}\n\n", &long[1..]).as_bytes(), "app").is_some());
    }

    #[test]
    fn two_copies_using_the_private_record_exchange_a_document() {
        let dir = std::env::temp_dir().join(format!("mdshared-auth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.ini");
        let private = super::super::directory(&settings, true).unwrap();
        let path = super::super::port_file(&settings);
        std::fs::write(&path, b"12345").unwrap();
        assert!(
            Record::load(&private).is_none(),
            "never trust a legacy port"
        );
        let listener = Record::publish(&private, 12345).unwrap();
        let sender = Record::load(&private).unwrap();
        assert_eq!(sender.port, listener.port);
        let (sr, mut cw) = std::io::pipe().unwrap();
        let (cr, mut sw) = std::io::pipe().unwrap();
        let receiver = std::thread::spawn(move || {
            let mut sr = BufReader::new(sr);
            authenticate(&mut sr, &mut sw, &listener.key).unwrap();
            assert_eq!(header(&mut sr, "app").unwrap(), ["?doc=d", "?text=4"]);
            let mut text = [0; 4];
            sr.read_exact(&mut text).unwrap();
            assert_eq!(&text, b"note");
            sw.write_all(b"ok doc\n").unwrap();
        });
        assert_eq!(
            exchange(
                &mut BufReader::new(cr),
                &mut cw,
                &sender.key,
                b"app\n?doc=d\n?text=4\n\nnote"
            )
            .as_deref(),
            Some("ok doc")
        );
        receiver.join().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = std::fs::metadata(&path).unwrap().permissions();
            assert_eq!(mode.mode() & 0o077, 0);
            mode.set_mode(0o644);
            std::fs::set_permissions(&path, mode).unwrap();
            assert!(Record::load(&private).is_none());
        }
        drop(private);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
