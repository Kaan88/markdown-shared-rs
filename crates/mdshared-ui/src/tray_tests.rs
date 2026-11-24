use super::*;

#[test]
fn separators_fall_between_items_only() {
    let items = [
        Item {
            separator_before: true,
            ..Item::new(1, "First")
        },
        Item::new(2, "Second"),
        Item {
            separator_before: true,
            ..Item::new(3, "Third")
        },
        Item {
            separator_before: true,
            ..Item::new(4, "Fourth")
        },
    ];
    let e = entries(&items);
    let kinds: Vec<&str> = e
        .iter()
        .map(|e| match e {
            Entry::Separator => "-",
            Entry::Item { text, .. } => text.as_str(),
        })
        .collect();
    assert_eq!(kinds, ["First", "Second", "-", "Third", "-", "Fourth"]);
}

#[test]
fn an_item_keeps_its_id_access_key_hint_and_state() {
    let items = [
        Item::new(10, "&Play").hint("Space"),
        Item::new(11, "Shuffle").checked(false),
        Item {
            enabled: false,
            ..Item::new(12, "Tom && Jerry")
        },
    ];
    let e = entries(&items);
    let Entry::Item {
        id,
        text,
        access,
        hint,
        enabled,
        checked,
    } = &e[0]
    else {
        panic!("an item");
    };
    assert_eq!((*id, text.as_str(), *access), (10, "Play", Some(0)));
    assert_eq!(hint, "Space");
    assert!(*enabled && checked.is_none());
    assert!(matches!(
        &e[1],
        Entry::Item {
            checked: Some(false),
            ..
        }
    ));
    assert!(
        matches!(&e[2], Entry::Item { enabled: false, access: None, text, .. } if text == "Tom & Jerry")
    );
}

#[test]
fn the_access_mark_is_written_each_systems_way() {
    assert_eq!(marked("Play", Some(0), '&'), "&Play");
    assert_eq!(marked("Tom & Jerry", Some(6), '&'), "Tom && &Jerry");
    assert_eq!(marked("Q_uit", Some(0), '_'), "_Q__uit");
    assert_eq!(marked("Näh", Some(1), '_'), "N_äh");
    assert_eq!(marked("Plain", None, '_'), "Plain");
}

#[test]
fn the_tooltip_is_cut_at_a_chars_edge() {
    assert_eq!(
        tooltip_units("abc", 127),
        "abc".encode_utf16().collect::<Vec<_>>()
    );
    let long = "x".repeat(200);
    assert_eq!(tooltip_units(&long, 127).len(), 127);
    // A character outside the basic plane takes two units and is not split.
    let s = format!("{}🎵", "a".repeat(126));
    assert_eq!(tooltip_units(&s, 127).len(), 126);
}

#[cfg(not(any(windows, target_os = "linux")))]
#[test]
fn without_a_backend_there_is_no_tray() {
    let image = Image::new(1, 1, vec![0; 4]).unwrap();
    let identity = Identity { name: "t", id: "t" };
    assert_eq!(
        Tray::new(&identity, &image, || {}).err().map(|e| e.kind()),
        Some(io::ErrorKind::Unsupported)
    );
}
