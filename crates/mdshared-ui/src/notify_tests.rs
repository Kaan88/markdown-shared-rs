use super::*;

#[test]
fn a_notification_is_built_by_parts() {
    let n = Notification::new("Now playing", "Song — Artist")
        .action("next", "Next")
        .tagged("now");
    assert_eq!(n.title, "Now playing");
    assert_eq!(n.actions, [Action::new("next", "Next")]);
    assert_eq!(n.tag.as_deref(), Some("now"));
    assert_eq!(n.urgency, Urgency::Normal);
    assert!(!n.silent && n.image.is_none());
}

#[test]
fn only_the_first_three_whole_buttons_are_shown() {
    let n = Notification::new("t", "")
        .action("a", "A")
        .action("", "no id")
        .action("b", "")
        .action("c", "C")
        .action("d", "D")
        .action("e", "E");
    let ids: Vec<&str> = n.shown_actions().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["a", "c", "d"]);
}

#[test]
fn xml_text_is_escaped_and_control_characters_dropped() {
    assert_eq!(
        xml_escape("Tom & \"Jerry\" <b>'s</b>"),
        "Tom &amp; &quot;Jerry&quot; &lt;b&gt;&apos;s&lt;/b&gt;"
    );
    assert_eq!(xml_escape("a\u{1}b\tc\nd"), "ab\tc\nd");
    assert_eq!(xml_escape("Ünïcødé ♫"), "Ünïcødé ♫");
}

#[test]
fn the_toast_says_what_the_notification_says() {
    let plain = toast_xml(&Notification::new("Title", ""));
    assert_eq!(
        plain,
        "<toast launch=\"click\"><visual><binding template=\"ToastGeneric\">\
         <text>Title</text></binding></visual></toast>"
    );
    let mut n = Notification::new("A & B", "line <2>").action("next", "Next >");
    n.image = Some(PathBuf::from(r"C:\Covers\a b.png"));
    n.silent = true;
    let x = toast_xml(&n);
    assert!(
        x.contains("<text>A &amp; B</text><text>line &lt;2&gt;</text>"),
        "{x}"
    );
    assert!(x.contains("src=\"file:///C:/Covers/a%20b.png\""), "{x}");
    assert!(
        x.contains("<action content=\"Next &gt;\" arguments=\"action:next\" activationType=\"foreground\"/>"),
        "{x}"
    );
    assert!(x.contains("<audio silent=\"true\"/>"), "{x}");
    assert!(!x.contains("scenario"), "{x}");
}

#[test]
fn a_critical_toast_stays_and_has_a_way_to_go() {
    let mut n = Notification::new("Copy failed", "");
    n.urgency = Urgency::Critical;
    let x = toast_xml(&n);
    assert!(x.contains("scenario=\"reminder\""), "{x}");
    assert!(x.contains("activationType=\"system\""), "{x}");
    // With buttons of its own it needs no other.
    let x = toast_xml(&n.action("retry", "Retry"));
    assert!(!x.contains("activationType=\"system\""), "{x}");
    let mut low = Notification::new("Done", "");
    low.urgency = Urgency::Low;
    assert!(toast_xml(&low).contains("silent"));
}

#[test]
fn activations_become_events() {
    assert_eq!(activation(Id(4), CLICK), Event::Clicked(Id(4)));
    assert_eq!(activation(Id(4), ""), Event::Clicked(Id(4)));
    assert_eq!(
        activation(Id(4), "action:next"),
        Event::Action(Id(4), "next".into())
    );
    // A button id that itself holds the prefix keeps the rest whole.
    assert_eq!(
        activation(Id(4), "action:action:x"),
        Event::Action(Id(4), "action:x".into())
    );
}

#[test]
fn paths_become_file_urls() {
    assert_eq!(
        file_url(std::path::Path::new("/run/user/1000/cover #1.png")),
        "file:///run/user/1000/cover%20%231.png"
    );
    assert_eq!(
        file_url(std::path::Path::new(r"C:\Users\Ä\c.png")),
        "file:///C:/Users/%C3%84/c.png"
    );
}

#[test]
fn ids_count_up_and_tags_remember_the_last() {
    // The bookkeeping, without a system: a Notifier over no backend.
    #[cfg(not(all(any(windows, target_os = "linux"), feature = "notify")))]
    {
        let identity = Identity {
            name: "test",
            app_id: "Test.App",
            icon: None,
        };
        let e = Notifier::new(&identity, || {}).err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
        let mut n = Notifier {
            inbox: Arc::new(Inbox {
                events: Mutex::new(Vec::new()),
                wake: Box::new(|| {}),
            }),
            next: 1,
            tags: HashMap::new(),
            backend: (),
        };
        let a = n.show(&Notification::new("a", "").tagged("now")).unwrap();
        let b = n.show(&Notification::new("b", "").tagged("now")).unwrap();
        let c = n.show(&Notification::new("c", "")).unwrap();
        assert_eq!((a, b, c), (Id(1), Id(2), Id(3)));
        assert_eq!(n.tags.get("now"), Some(&b));
        n.close(b).unwrap();
        assert!(n.tags.is_empty());
        assert!(n.events().is_empty());
    }
}
