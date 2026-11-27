//! Checks remote request ordering without fetching anything.

use super::{ImageStore, QUEUE_PROBES, Slot};

#[test]
fn cancelling_and_draining_remote_requests_updates_membership_without_reordering() {
    let mut store = ImageStore::new(1 << 20);
    for url in ["a", "b", "a", "c"] {
        store.remote(url, true);
    }
    store.set_remote("b", Slot::Pending, 1.0);
    assert_eq!(store.take_remote(), ["a", "c"]);
    for url in ["c", "b", "a"] {
        store.remote(url, true);
    }
    assert_eq!(store.take_remote(), ["c", "a"]);
    assert!(store.take_remote().is_empty());

    let urls: Vec<_> = (0..4096).map(|i| format!("image-{i}")).collect();
    for url in &urls {
        store.remote(url, true);
    }
    QUEUE_PROBES.set(0);
    assert_eq!(store.take_remote(), urls);
    let probes = QUEUE_PROBES.get();
    assert!(
        (urls.len()..=8 * urls.len()).contains(&probes),
        "{probes} drain probes"
    );

    // Even with a large table already allocated, a singleton touches just its own membership.
    for _ in 0..2 {
        store.remote("a", true);
        QUEUE_PROBES.set(0);
        assert_eq!(store.take_remote(), ["a"]);
        let probes = QUEUE_PROBES.get();
        assert!((1..=8).contains(&probes), "{probes} singleton drain probes");
    }
    QUEUE_PROBES.set(0);
    assert!(store.take_remote().is_empty());
    assert_eq!(QUEUE_PROBES.get(), 0);
}
