//! The documents the plan card's demonstrations and tests are cut from: built here, so nothing
//! on disk is read. mdedit's `--plan-demo` opens them.

const PARTS: [(&str, &[&str]); 6] = [
    ("Planning", &["Planning the route", "Packing and money"]),
    (
        "Tokyo",
        &[
            "Arriving at Haneda",
            "Asakusa and the river",
            "Shibuya at night",
            "Tsukiji breakfast",
            "Ueno and the museums",
            "Day trip to Nikko",
            "Shinjuku in the rain",
        ],
    ),
    (
        "Kyoto",
        &[
            "The night train",
            "Fushimi Inari before dawn",
            "Kinkaku-ji",
            "Gion in the evening",
            "Arashiyama bamboo",
            "The philosopher's path",
        ],
    ),
    (
        "Osaka",
        &[
            "Dotonbori",
            "Osaka castle",
            "Street food, again",
            "The aquarium",
        ],
    ),
    ("Nara", &["Deer in the park", "Todai-ji"]),
    ("Home", &["The last day", "What I would do differently"]),
];

const SENTENCES: [&str; 12] = [
    "We left early and the train was already full of people going to work.",
    "The river was grey and flat, and the boats went past without a sound.",
    "Lunch was a bowl of noodles at a counter with eight seats.",
    "It rained for an hour, then the street steamed in the sun.",
    "I wrote this on the platform while we waited for the next one.",
    "Every shop had a small shrine above the door, with fresh flowers.",
    "We walked until the maps stopped helping and then walked further.",
    "The evening was warm, and the lanterns came on one by one.",
    "A woman at the stall showed us how to fold the paper properly.",
    "The ticket machine had no English, so we guessed, and guessed right.",
    "Tomorrow is a long day, so tonight is for sleeping early.",
    "Nothing here was planned, and that was the best of it.",
];

/// The Japan trip journal: a title and a contents list before the first heading, `#` parts
/// with `##` chapters under them, some `###` headings, and a sign-off after the last chapter.
pub fn japan() -> String {
    let mut s = String::new();
    s.push_str("**Japan trip journal**\n\nNotes from two weeks in October, written on trains and in\nguesthouses.\n\nContents\n");
    for (part, chapters) in PARTS {
        s.push_str(&format!("- {part} ({} chapters)\n", chapters.len()));
    }
    s.push('\n');
    let mut n = 0usize;
    for (part, chapters) in PARTS {
        s.push_str(&format!("# {part}\n\n"));
        for title in chapters {
            s.push_str(&format!("## {title}\n\n"));
            for p in 0..(3 + n % 4) {
                let a = SENTENCES[(n * 5 + p * 3) % SENTENCES.len()];
                let b = SENTENCES[(n * 7 + p * 5 + 1) % SENTENCES.len()];
                s.push_str(&format!("{a}\n{b}\n\n"));
            }
            if n.is_multiple_of(3) {
                s.push_str("### What we did\n\n");
                for k in 0..3 {
                    s.push_str(&format!("- {}\n", SENTENCES[(n + k * 4) % SENTENCES.len()]));
                }
                s.push('\n');
            }
            n += 1;
        }
    }
    s.push_str(
        "# Afterword\n\nThanks for reading this far.\nSee the photographs in the other folder.\n",
    );
    s
}

/// Recipes: every line is in a note, so the strip is full.
pub fn recipes() -> String {
    let names = [
        "Lemon tart",
        "Lentil soup",
        "Flatbread",
        "Pickled onions",
        "Granola",
        "Plum jam",
        "Shakshuka",
        "Carrot cake",
        "Miso aubergine",
        "Sourdough starter",
        "Hummus",
        "Rice pudding",
    ];
    let mut s = String::new();
    for (i, name) in names.iter().enumerate() {
        s.push_str(&format!("## {name}\n\n### Ingredients\n\n"));
        for k in 0..(3 + i % 3) {
            s.push_str(&format!(
                "- {} g of something good ({})\n",
                50 + k * 25,
                k + 1
            ));
        }
        s.push_str("\n### Method\n\n");
        for p in 0..(1 + i % 3) {
            s.push_str(&format!(
                "{}\n{}\n\n",
                SENTENCES[(i * 3 + p) % SENTENCES.len()],
                SENTENCES[(i * 5 + p + 2) % SENTENCES.len()]
            ));
        }
    }
    s
}

/// One long note, a code block with a heading inside it, and two short ones.
pub fn log() -> String {
    let mut s = String::from("## Decisions\n\n");
    for k in 0..34 {
        s.push_str(&format!("{}\n", SENTENCES[k % SENTENCES.len()]));
    }
    s.push_str("\n```\n## not a heading, a line of the build log\nstep 1 ok\nstep 2 ok\nstep 3 ok\n```\n\n");
    for k in 0..10 {
        s.push_str(&format!("{}\n", SENTENCES[(k * 2) % SENTENCES.len()]));
    }
    s.push_str("\n## Todo\n\n- call the printer\n- book the room\n\n## Links\n\n- the wiki\n- the tracker\n");
    s
}

/// A hundred and sixty small notes.
pub fn many() -> String {
    let words = [
        "Harbour", "Orchard", "Lantern", "Bridge", "Garden", "Market", "Station", "Temple",
        "Meadow", "Quarry",
    ];
    let mut s = String::new();
    for i in 0..160usize {
        s.push_str(&format!(
            "## {} {}\n\n",
            words[i % words.len()],
            words[(i / words.len() + 3) % words.len()]
        ));
        for k in 0..(2 + (i * 7) % 6) {
            s.push_str(&format!("{}\n", SENTENCES[(i + k) % SENTENCES.len()]));
        }
        s.push('\n');
    }
    s
}
