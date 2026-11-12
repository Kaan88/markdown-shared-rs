//! Which URLs are being fetched and which failed, so a failure is retried after a delay and not
//! in a tight loop.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchState {
    Pending,
    FailedAt(Instant),
}

/// Remote image requests. A failed URL may be retried after a delay or as soon as the document
/// reloads; the old app's `pending` set never forgot one, so a failure lasted for the page's life.
#[derive(Debug)]
pub struct ImageRequests {
    states: HashMap<String, FetchState>,
    retry_after: Duration,
}

impl Default for ImageRequests {
    fn default() -> Self {
        ImageRequests {
            states: HashMap::new(),
            retry_after: Duration::from_secs(30),
        }
    }
}

impl ImageRequests {
    pub fn with_retry_after(retry_after: Duration) -> Self {
        ImageRequests {
            states: HashMap::new(),
            retry_after,
        }
    }

    /// True if the caller should start a fetch now. The URL then counts as pending.
    pub fn should_fetch(&mut self, url: &str, now: Instant) -> bool {
        let allowed = match self.states.get(url) {
            None => true,
            Some(FetchState::Pending) => false,
            Some(FetchState::FailedAt(t)) => now.duration_since(*t) >= self.retry_after,
        };
        if allowed {
            self.states.insert(url.to_string(), FetchState::Pending);
        }
        allowed
    }

    /// Success forgets the URL, because the decoded image lives in the image cache from then on.
    pub fn finished(&mut self, url: &str, ok: bool, now: Instant) {
        if ok {
            self.states.remove(url);
        } else {
            self.states
                .insert(url.to_string(), FetchState::FailedAt(now));
        }
    }

    /// A reload is an explicit reason to try every failed image again.
    pub fn on_reload(&mut self) {
        self.states.retain(|_, s| matches!(s, FetchState::Pending));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_images_retry() {
        let mut r = ImageRequests::with_retry_after(Duration::from_secs(30));
        let t0 = Instant::now();
        assert!(r.should_fetch("u", t0));
        assert!(!r.should_fetch("u", t0), "one request at a time");
        r.finished("u", false, t0);
        assert!(
            !r.should_fetch("u", t0 + Duration::from_secs(1)),
            "not in a tight loop"
        );
        assert!(r.should_fetch("u", t0 + Duration::from_secs(31)));
        r.finished("u", false, t0);
        r.on_reload();
        assert!(r.should_fetch("u", t0), "reloading clears failures");
    }
}
