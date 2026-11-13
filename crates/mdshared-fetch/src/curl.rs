//! One HTTP GET through the system libcurl, loaded with `dlopen` at the first request, so the
//! binary carries no TLS stack and has no link-time dependency on libcurl: where the library is
//! missing, [`Session::new`] fails with [`Error::Unsupported`] and every image just shows as failed.
//!
//! The contract is that of `winhttp.rs`. What this module sets on every request: no automatic
//! redirects (`download` follows them), protocols limited to http and https, no cookie engine, no
//! netrc, no authentication, no `Accept-Encoding` (so no decompression), `Accept: image/*` (or what
//! [`Session::accepting`] set), the fixed user agent, TLS verification on, and the overall time
//! limit set from the deadline. One easy handle is made per request and dropped after it, so a
//! [`Session`] may be used from any thread and nothing is kept alive between requests.
//!
//! Every `unsafe` block of the crate on this platform is in this module.

use std::ffi::{CStr, CString, c_char, c_int, c_long, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::time::Instant;

use libloading::Library;

use crate::url::Url;
use crate::{Error, Response};

type Handle = *mut c_void;

// Option numbers from <curl/curl.h>: the base is 0 for long, 10000 for pointers and strings, 20000
// for callbacks and 30000 for 64-bit offsets.
const CURLOPT_WRITEDATA: c_int = 10001;
const CURLOPT_URL: c_int = 10002;
const CURLOPT_USERAGENT: c_int = 10018;
const CURLOPT_HTTPHEADER: c_int = 10023;
const CURLOPT_HEADERDATA: c_int = 10029;
const CURLOPT_WRITEFUNCTION: c_int = 20011;
const CURLOPT_HEADERFUNCTION: c_int = 20079;
const CURLOPT_NOPROGRESS: c_int = 43;
const CURLOPT_NETRC: c_int = 51;
const CURLOPT_FOLLOWLOCATION: c_int = 52;
const CURLOPT_SSL_VERIFYPEER: c_int = 64;
const CURLOPT_FORBID_REUSE: c_int = 75;
const CURLOPT_SSL_VERIFYHOST: c_int = 81;
const CURLOPT_NOSIGNAL: c_int = 99;
const CURLOPT_TIMEOUT_MS: c_int = 155;
const CURLOPT_CONNECTTIMEOUT_MS: c_int = 156;
const CURLOPT_PROTOCOLS: c_int = 181;
const CURLOPT_REDIR_PROTOCOLS: c_int = 182;
const CURLOPT_MAXFILESIZE_LARGE: c_int = 30117;
const CURLOPT_PROTOCOLS_STR: c_int = 10318;
const CURLOPT_REDIR_PROTOCOLS_STR: c_int = 10319;
const CURLOPT_HTTPAUTH: c_int = 107;
const CURLAUTH_NONE: c_long = 0;
/// `CURLPROTO_HTTP | CURLPROTO_HTTPS`, for libcurl older than 7.85 which lacks the `_STR` options.
const CURLPROTO_HTTP_HTTPS: c_long = 1 | 2;

const CURL_GLOBAL_DEFAULT: c_long = 3;
const CURLE_OPERATION_TIMEDOUT: c_int = 28;
const CURLE_FILESIZE_EXCEEDED: c_int = 63;
const CURLE_WRITE_ERROR: c_int = 23;

/// Longest `ETag`, `Last-Modified` or `Location` value accepted, as in the WinHTTP transport.
const MAX_HEADER_VALUE: usize = 8 * 1024;

type SetOpt = unsafe extern "C" fn(Handle, c_int, ...) -> c_int;
type WriteCb = extern "C" fn(*const u8, usize, usize, *mut c_void) -> usize;

/// The libcurl entry points this module uses. The library is never unloaded.
struct Api {
    easy_init: unsafe extern "C" fn() -> Handle,
    easy_cleanup: unsafe extern "C" fn(Handle),
    easy_setopt: SetOpt,
    easy_perform: unsafe extern "C" fn(Handle) -> c_int,
    slist_append: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    slist_free_all: unsafe extern "C" fn(*mut c_void),
    _lib: Library,
}

// SAFETY: the struct holds function pointers into a library that is never unloaded; libcurl's
// easy interface may be used from several threads as long as each handle is used by one at a time,
// which `Session::get` guarantees by making a handle per call.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(load).as_ref()
}

fn load() -> Option<Api> {
    // SAFETY: loading libcurl runs its initializers, which are benign; the names are the SONAME
    // of the stable ABI (`libcurl.so.4`) and the development symlink.
    let lib = ["libcurl.so.4", "libcurl.so"]
        .iter()
        .find_map(|name| unsafe { Library::new(name) }.ok())?;
    // SAFETY: each symbol is looked up by its documented name and given its documented C
    // signature (`curl_easy_setopt` is variadic).
    unsafe {
        let init: unsafe extern "C" fn(c_long) -> c_int = *lib.get(b"curl_global_init\0").ok()?;
        let api = Api {
            easy_init: *lib.get(b"curl_easy_init\0").ok()?,
            easy_cleanup: *lib.get(b"curl_easy_cleanup\0").ok()?,
            easy_setopt: *lib.get(b"curl_easy_setopt\0").ok()?,
            easy_perform: *lib.get(b"curl_easy_perform\0").ok()?,
            slist_append: *lib.get(b"curl_slist_append\0").ok()?,
            slist_free_all: *lib.get(b"curl_slist_free_all\0").ok()?,
            _lib: lib,
        };
        // Once per process; this is the only caller, run inside a `OnceLock`.
        if init(CURL_GLOBAL_DEFAULT) != 0 {
            return None;
        }
        Some(api)
    }
}

/// An easy handle and its header list, freed on drop.
struct Easy {
    api: &'static Api,
    handle: Handle,
    headers: *mut c_void,
}

impl Drop for Easy {
    fn drop(&mut self) {
        // SAFETY: the handle came from `curl_easy_init` and is cleaned up once; the header list
        // is freed after the handle that used it is gone.
        unsafe {
            (self.api.easy_cleanup)(self.handle);
            if !self.headers.is_null() {
                (self.api.slist_free_all)(self.headers);
            }
        }
    }
}

impl Easy {
    fn new(api: &'static Api) -> Result<Easy, Error> {
        // SAFETY: plain constructor.
        let handle = unsafe { (api.easy_init)() };
        if handle.is_null() {
            return Err(Error::Unsupported);
        }
        Ok(Easy {
            api,
            handle,
            headers: null_mut(),
        })
    }

    /// Sets a `long` option; the curl result code is returned.
    fn long(&self, opt: c_int, v: c_long) -> c_int {
        // SAFETY: the handle is open and `opt` takes a long.
        unsafe { (self.api.easy_setopt)(self.handle, opt, v) }
    }

    /// Sets a string option; libcurl copies the string.
    fn text(&self, opt: c_int, v: &CStr) -> c_int {
        // SAFETY: the handle is open, `opt` takes a NUL-terminated string and v is one.
        unsafe { (self.api.easy_setopt)(self.handle, opt, v.as_ptr()) }
    }

    /// Sets a pointer option. The pointee must outlive the transfer.
    fn ptr(&self, opt: c_int, v: *mut c_void) -> c_int {
        // SAFETY: the handle is open; the caller keeps `v` alive for the transfer.
        unsafe { (self.api.easy_setopt)(self.handle, opt, v) }
    }

    fn add_header(&mut self, line: &str) {
        let Ok(c) = CString::new(line) else { return };
        // SAFETY: the list is null or a list from a previous append; libcurl copies the string.
        let list = unsafe { (self.api.slist_append)(self.headers, c.as_ptr()) };
        if !list.is_null() {
            self.headers = list;
        }
    }
}

/// What the callbacks collect during one transfer.
struct Transfer {
    max_bytes: usize,
    status: u16,
    etag: Option<String>,
    last_modified: Option<String>,
    location: Option<String>,
    content_length: Option<u64>,
    body: Vec<u8>,
    /// Set by a callback that stopped the transfer on purpose.
    stopped: Option<Error>,
    /// A body that is not wanted (status other than 200) was refused.
    skipped: bool,
}

fn header_value(slot: &mut Option<String>, value: &str, stopped: &mut Option<Error>) {
    if value.len() > MAX_HEADER_VALUE {
        *stopped = Some(Error::BadResponse);
    } else if slot.is_none() {
        *slot = Some(value.to_string());
    }
}

extern "C" fn on_header(line: *const u8, size: usize, n: usize, user: *mut c_void) -> usize {
    let len = size.saturating_mul(n);
    // SAFETY: libcurl passes `len` readable bytes and the pointer given as HEADERDATA, which is
    // the `Transfer` on the stack of `get`, used by this thread only.
    let (line, t) = unsafe {
        (
            std::slice::from_raw_parts(line, len),
            &mut *user.cast::<Transfer>(),
        )
    };
    let line = String::from_utf8_lossy(line);
    let line = line.trim_end_matches(['\r', '\n']);
    if line.len() >= 5 && line[..5].eq_ignore_ascii_case("HTTP/") {
        // A new response (a 100 Continue or a proxy's CONNECT reply comes first): start over.
        t.status = line
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        t.etag = None;
        t.last_modified = None;
        t.location = None;
        t.content_length = None;
    } else if let Some((name, value)) = line.split_once(':') {
        let value = value.trim_matches([' ', '\t']);
        let name = name.trim();
        if name.eq_ignore_ascii_case("etag") {
            header_value(&mut t.etag, value, &mut t.stopped);
        } else if name.eq_ignore_ascii_case("last-modified") {
            header_value(&mut t.last_modified, value, &mut t.stopped);
        } else if name.eq_ignore_ascii_case("location") {
            header_value(&mut t.location, value, &mut t.stopped);
        } else if name.eq_ignore_ascii_case("content-length") && t.status == 200 {
            match crate::parse_content_length(value) {
                Some(n) if n > t.max_bytes as u64 => t.stopped = Some(Error::TooLarge),
                Some(n) => t.content_length = Some(n),
                None => t.stopped = Some(Error::BadResponse),
            }
        }
    }
    if t.stopped.is_some() { 0 } else { len }
}

extern "C" fn on_body(data: *const u8, size: usize, n: usize, user: *mut c_void) -> usize {
    let len = size.saturating_mul(n);
    // SAFETY: as in `on_header`.
    let (data, t) = unsafe {
        (
            std::slice::from_raw_parts(data, len),
            &mut *user.cast::<Transfer>(),
        )
    };
    if t.status != 200 {
        t.skipped = true;
        return 0;
    }
    if t.body.len() + len > t.max_bytes {
        t.stopped = Some(Error::TooLarge);
        return 0;
    }
    t.body.extend_from_slice(data);
    len
}

/// A libcurl "session": just the settings every request shares; each request makes its own handle.
pub struct Session {
    api: &'static Api,
    agent: CString,
    /// The `Accept` header's value.
    accept: &'static str,
}

impl Session {
    pub fn new(user_agent: &str) -> Result<Session, Error> {
        let api = api().ok_or(Error::Unsupported)?;
        let agent = CString::new(user_agent.replace('\0', "")).unwrap_or_default();
        Ok(Session {
            api,
            agent,
            accept: "image/*",
        })
    }

    /// The same session asking for `accept` instead of images.
    pub fn accepting(mut self, accept: &'static str) -> Session {
        self.accept = accept;
        self
    }

    /// One GET of `url`, no redirects followed, finished or abandoned by `deadline`.
    /// `extra_headers` are complete `Name: value` lines the caller has checked for CR/LF. The body
    /// is read only for status 200 and fails with [`Error::TooLarge`] as soon as it would exceed
    /// `max_bytes`.
    pub fn get(
        &self,
        url: &Url,
        extra_headers: &[String],
        deadline: Instant,
        max_bytes: usize,
    ) -> Result<Response, Error> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Timeout);
        }
        let ms = left.as_millis().clamp(1, i32::MAX as u128) as c_long;
        let mut easy = Easy::new(self.api)?;
        let target = CString::new(url.to_string()).map_err(|_| Error::BadResponse)?;
        let mut transfer = Transfer {
            max_bytes,
            status: 0,
            etag: None,
            last_modified: None,
            location: None,
            content_length: None,
            body: Vec::new(),
            stopped: None,
            skipped: false,
        };

        let mut rc = 0;
        let mut set = |code: c_int| {
            if rc == 0 {
                rc = code;
            }
        };
        set(easy.long(CURLOPT_NOSIGNAL, 1));
        set(easy.long(CURLOPT_NOPROGRESS, 1));
        set(easy.long(CURLOPT_FOLLOWLOCATION, 0));
        set(easy.long(CURLOPT_NETRC, 0));
        set(easy.long(CURLOPT_HTTPAUTH, CURLAUTH_NONE));
        set(easy.long(CURLOPT_FORBID_REUSE, 1));
        set(easy.long(CURLOPT_SSL_VERIFYPEER, 1));
        set(easy.long(CURLOPT_SSL_VERIFYHOST, 2));
        set(easy.long(CURLOPT_TIMEOUT_MS, ms));
        set(easy.long(CURLOPT_CONNECTTIMEOUT_MS, ms));
        // The size limit, so libcurl can refuse a body that declares too much before reading it.
        // SAFETY: the option takes a curl_off_t (i64), passed as one.
        set(unsafe {
            (self.api.easy_setopt)(
                easy.handle,
                CURLOPT_MAXFILESIZE_LARGE,
                i64::try_from(max_bytes).unwrap_or(i64::MAX),
            )
        });
        // Protocols: http and https only. libcurl before 7.85 knows only the numeric options.
        let http = c"http,https";
        if easy.text(CURLOPT_PROTOCOLS_STR, http) != 0 {
            set(easy.long(CURLOPT_PROTOCOLS, CURLPROTO_HTTP_HTTPS));
        }
        if easy.text(CURLOPT_REDIR_PROTOCOLS_STR, http) != 0 {
            set(easy.long(CURLOPT_REDIR_PROTOCOLS, CURLPROTO_HTTP_HTTPS));
        }
        set(easy.text(CURLOPT_URL, &target));
        set(easy.text(CURLOPT_USERAGENT, &self.agent));
        // No cookie engine (COOKIEFILE is never set), no `Accept-Encoding` (ACCEPT_ENCODING is
        // never set). `Accept` is replaced by the one below; `Expect` is not sent for a GET.
        easy.add_header(&format!("Accept: {}", self.accept));
        for h in extra_headers {
            easy.add_header(h);
        }
        set(easy.ptr(CURLOPT_HTTPHEADER, easy.headers));
        let state: *mut c_void = (&mut transfer as *mut Transfer).cast();
        set(easy.ptr(CURLOPT_HEADERDATA, state));
        set(easy.ptr(CURLOPT_WRITEDATA, state));
        // SAFETY: the callbacks have the signatures libcurl documents; they are passed as
        // function pointers through the variadic option setter.
        unsafe {
            let write: WriteCb = on_body;
            let head: WriteCb = on_header;
            set((self.api.easy_setopt)(
                easy.handle,
                CURLOPT_WRITEFUNCTION,
                write,
            ));
            set((self.api.easy_setopt)(
                easy.handle,
                CURLOPT_HEADERFUNCTION,
                head,
            ));
        }
        if rc != 0 {
            return Err(Error::Os(rc as u32));
        }

        // SAFETY: the handle is fully set up; `transfer` outlives the call and is touched only by
        // the callbacks, on this thread, during it.
        let code = unsafe { (self.api.easy_perform)(easy.handle) };
        drop(easy);

        if let Some(e) = transfer.stopped {
            return Err(e);
        }
        match code {
            0 => {}
            CURLE_WRITE_ERROR if transfer.skipped => {}
            CURLE_OPERATION_TIMEDOUT => return Err(Error::Timeout),
            CURLE_FILESIZE_EXCEEDED => return Err(Error::TooLarge),
            other => return Err(Error::Os(other as u32)),
        }
        if transfer.status == 0 {
            return Err(Error::BadResponse);
        }
        Ok(Response {
            status: transfer.status,
            etag: transfer.etag,
            last_modified: transfer.last_modified,
            location: transfer.location,
            body: transfer.body,
        })
    }
}
