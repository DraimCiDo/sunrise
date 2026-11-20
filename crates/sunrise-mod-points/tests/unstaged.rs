//! Plain builds must not accept listeners that can never fire.
use mod_api::__private::points::{Forward, Place, PointHost, SlotListener};
use mod_points::RegisterError;

/// A source tree's build has no point catalog: it compiles no point, so no listener can name one,
/// and its table says it has no sites, which a wasm mod's load is refused by.
#[test]
fn a_plain_build_has_no_points_and_no_sites() {
    if mod_points::STAGED {
        return;
    }
    assert_eq!(mod_points::POINT_COUNT, 0);
    assert!(!mod_points::__private::TABLE.has_sites());
}

/// A place built outside its table is refused by name, staged or not: no index is trusted.
#[test]
fn a_place_outside_the_table_is_refused() {
    struct Never;
    impl Forward for Never {
        fn forward(&self, _: u64, _: PointHost<'_>, _: &mut dyn mod_api::wire::Record) {}
    }
    let place = Place::new(&mod_points::__private::TABLE, usize::MAX);
    let slot = SlotListener::forwarded("outside_mod", place, &Never, 0);
    let error = mod_api::__private::points::register(&slot).unwrap_err();
    assert_eq!(
        error,
        RegisterError::NoPlace {
            id: "outside_mod",
            index: usize::MAX
        }
    );
    assert_eq!(
        error.to_string(),
        format!(
            "outside_mod: place {} is outside its point table",
            usize::MAX
        )
    );
}
