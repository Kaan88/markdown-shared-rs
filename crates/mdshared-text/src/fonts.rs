use parley::FontContext;

thread_local! {
    /// System fonts are enumerated once per thread; every renderer starts from a clone of this. A
    /// window makes several renderers (one per tab and a spare for `mem::take`), and enumerating
    /// fonts for each cost about 35 ms apiece at launch.
    static FONTS: std::cell::RefCell<Option<FontContext>> = const { std::cell::RefCell::new(None) };
}

/// The per-thread font context every renderer clones. A shell that draws its own text takes a
/// clone too, so system fonts are still enumerated once.
pub fn shared_fonts() -> FontContext {
    FONTS.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(FontContext::new)
            .clone()
    })
}
