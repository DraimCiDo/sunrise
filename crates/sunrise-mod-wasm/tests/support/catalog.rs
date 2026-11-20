//! Write the listener fixture's catalog, which the wasm fixtures build against:
//! `cargo run -p sunrise-mod-api-gen --example listener-fixture-catalog -- crates/sunrise-mod-wasm/tests/fixture/points.json`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let points = sunrise_mod_api_gen::points::fixture_points(&root)?;
    let output = std::env::args_os().nth(1).ok_or("expected output path")?;
    std::fs::write(
        output,
        serde_json::to_vec(&serde_json::json!({
            "schema": point_shape::build_data::SCHEMA,
            "build": {"commit": "fixture"},
            "points": points,
        }))?,
    )?;
    Ok(())
}
