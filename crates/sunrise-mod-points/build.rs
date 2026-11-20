fn main() {
    generate_fixture_readers();
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    sunrise_mod_api_gen::generate_points(&root, &out_dir).expect("generating the points");
}

// Use staging's emitter so the integration tests exercise the shipped cell reads.
fn generate_fixture_readers() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for (file, readers) in [
        (
            "replace_readers.rs",
            &[
                ("__fnr_read_slot_0", 0, "fn(&mut Option<String>, String)"),
                ("__fnr_read_body_0", 1, "fn(&mut Option<String>, String)"),
                ("__fnr_read_slot_2", 2, "fn(u32, u32) -> u32"),
                ("__fnr_read_body_2", 3, "fn(u32, u32) -> u32"),
            ][..],
        ),
        (
            "point_expansion_readers.rs",
            &[
                ("__fnr_read_slot_0", 0, "fn(i64) -> i64"),
                ("__fnr_read_body_0", 1, "fn(i64) -> i64"),
            ][..],
        ),
        (
            "wire_loopback_readers.rs",
            &[
                ("__fnr_read_slot_0", 0, "fn(f32, usize) -> f32"),
                ("__fnr_read_body_0", 1, "fn(f32, usize) -> f32"),
            ][..],
        ),
    ] {
        let mut code = String::new();
        for (name, cell, ty) in readers {
            code.push_str(
                &sunrise_mod_api_gen::replace::reader(
                    &syn::parse_str(name).expect("fixture reader name"),
                    *cell,
                    &ty.parse().expect("fixture function pointer type"),
                )
                .to_string(),
            );
        }
        sunrise_mod_api_gen::write_if_changed(out.join(file), code)
            .expect("writing fixture replacement readers");
    }
}
