// Generated fixtures exercise every shape; TODO(#2273): make the generator emit clippy-clean code.
#![allow(clippy::all, unused_comparisons, unreachable_patterns)]
use mod_api::__private::points::register;
use mod_points::{
    __private::{SlotListener, TABLE, seal},
    PointId,
};

#[test]
fn point_priority_preserves_dependency_order_and_orders_injects_within_a_mod() {
    mod_api::__private::initialize().expect("the point fixture initializes its platform runtime");
    struct Row {
        place: PointId,
        priorities: [i32; 4],
        one_owner: bool,
        expected: [&'static str; 4],
    }
    let rows = [
        Row {
            place: PointId::Tick,
            priorities: [0; 4],
            one_owner: false,
            expected: ["z", "a", "m", "b"],
        },
        Row {
            place: PointId::TickReturn,
            priorities: [0, 10, 0, 0],
            one_owner: false,
            expected: ["a", "z", "m", "b"],
        },
        Row {
            place: PointId::TickWorld,
            priorities: [0; 4],
            one_owner: true,
            expected: ["a", "b", "m", "z"],
        },
    ];
    fn a() {
        std::hint::black_box("a");
    }
    fn b() {
        std::hint::black_box("b");
    }
    fn m() {
        std::hint::black_box("m");
    }
    fn z() {
        std::hint::black_box("z");
    }
    for row in &rows {
        for (index, (owner, inject)) in [("z", "z"), ("a", "a"), ("m", "m"), ("b", "b")]
            .into_iter()
            .enumerate()
        {
            let owner = if row.one_owner { "same_mod" } else { owner };
            // SAFETY: these synthetic slots inspect kind and identity; they never invoke the pointers.
            let function = [z as fn(), a, m, b][index];
            let listener =
                unsafe { SlotListener::api(owner, row.place.place(), function as *const ()) };
            register(&listener.ordered(row.priorities[index], inject)).unwrap();
        }
    }
    for owner in 0..64 {
        let id = Box::leak(format!("owner_{owner}").into_boxed_str());
        // SAFETY: this fixture inspects identity without invoking the pointer.
        let listener =
            unsafe { SlotListener::api(id, PointId::TickWorldReturn.place(), a as *const ()) };
        register(&listener.ordered(owner % 3, "large")).unwrap();
    }
    seal();
    let mut owners = Vec::new();
    TABLE.fire_api(PointId::TickWorldReturn as usize, &mut |_| {
        owners.push(mod_api::content::crash::current_mod().unwrap().to_owned());
    });
    let expected: Vec<_> = (0..3)
        .rev()
        .flat_map(|priority| {
            (0..64)
                .filter(move |owner| owner % 3 == priority)
                .map(|owner| format!("owner_{owner}"))
        })
        .collect();
    assert_eq!(
        owners, expected,
        "equal priorities retain dependency order at scale"
    );
    for row in rows {
        let mut heard = Vec::new();
        TABLE.fire_api(row.place as usize, &mut |function| {
            heard.push(if row.one_owner {
                [
                    (a as *const (), "a"),
                    (b as *const (), "b"),
                    (m as *const (), "m"),
                    (z as *const (), "z"),
                ]
                .into_iter()
                .find(|(pointer, _)| *pointer == function.as_ptr())
                .unwrap()
                .1
            } else {
                mod_api::content::crash::current_mod().unwrap()
            });
        });
        assert_eq!(heard, row.expected);
    }
}
