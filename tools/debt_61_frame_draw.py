#!/usr/bin/env python3
"""Bundle the existing mob draw inputs/output and wrap the animation map values.

Run on the source before debt 61's frame-draw commit. Check every replacement
before writing any of the updated files.
"""

from pathlib import Path

SRC = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src"


def main():
    updated = {}

    def replace(file, old, new, count=1):
        path = SRC / file
        text = updated[path] if path in updated else path.read_text()
        found = text.count(old)
        if found != count:
            raise SystemExit(f"{file}: expected {count} matches, got {found}: {old!r}")
        updated[path] = text.replace(old, new)

    render = "mobs/render.rs"
    invisible = "mobs/invisible_body.rs"
    pose_tests = "mobs/render_pose_tests.rs"
    living = "game/mobs/creatures/living_draw.rs"
    perspective = "game/interface/perspective.rs"
    gui = "game/interface/gui_entity.rs"
    setup_tests = "game/modding/native_humanoid_setup_anim_tests.rs"
    anim = "mobs/anim.rs"

    for file, count in [
        (render, 6), (invisible, 2), (pose_tests, 2), (living, 1),
        (perspective, 1), (gui, 1), (setup_tests, 2),
    ]:
        replace(file, "render_with_hidden", "render_with_frame", count)

    replace(
        render,
        "/// Render a mob with its variant model, texture and layers.\n",
        """/// The per-draw model inputs and optional pose captured for the mods' post hooks and layers.
/// Start with a fresh value for each draw.
#[derive(Default)]
pub struct FrameDraw {
    /// Parts the pre hooks hid on the body and the layers over its own model.
    pub hidden: mod_api::HiddenParts,
    /// Whether to capture the body's humanoid pose for this draw.
    pub capture_parts: bool,
    /// The captured pose, if requested and the draw produced one.
    pub parts: Option<mod_api::PosedParts>,
}

/// Render a mob with its variant model, texture and layers.
""",
    )
    replace(
        render,
        """        extra,
        Default::default(),
        None,
        out,
""",
        "        extra,\n        &mut FrameDraw::default(),\n        out,\n",
    )
    replace(
        render,
        """/// every part. `parts`, given while the mods' post hooks want them, gets the humanoid parts as the
/// body was drawn ([`PosedParts`](mod_api::PosedParts)); it stays None for a mob that isn't a
/// humanoid or isn't drawn.
""",
        """/// every part. When requested, `frame.parts` gets the humanoid parts as the body was drawn; it
/// stays empty for a mob that isn't a humanoid or isn't drawn.
""",
    )
    replace(render, "/// `hidden` and `parts` as for [`render_with_frame`].", "/// `frame` as for [`render_with_frame`].")
    replace(
        render,
        """/// (`HumanoidArmorLayer`: the armour in `extra`). `parts` as for [`render_with_frame`], posed as
/// the layers draw over the unseen body.
""",
        """/// (`HumanoidArmorLayer`: the armour in `extra`). Captures the unhidden body's pose if requested;
/// the parts hidden by the pre hooks do not affect this route.
""",
    )
    for file, mutable, count in [(render, "mut ", 1), (render, "", 2), (invisible, "", 1)]:
        replace(
            file,
            f"    hidden: mod_api::HiddenParts,\n    {mutable}parts: Option<&mut Option<mod_api::PosedParts>>,\n",
            "    frame: &mut FrameDraw,\n",
            count,
        )
    replace(render, "    parts: Option<&mut Option<mod_api::PosedParts>>,\n", "    frame: &mut FrameDraw,\n")
    replace(
        render,
        "                    hidden,\n                    parts.as_deref_mut(),\n",
        "                    frame,\n",
    )
    for file, indent, count in [(render, 8, 2), (render, 12, 1), (invisible, 8, 1)]:
        spaces = " " * indent
        replace(file, f"{spaces}hidden,\n{spaces}parts,\n", f"{spaces}frame,\n", count)
    replace(render, "        None,\n        Default::default(),\n        parts,\n", "        None,\n        frame,\n")
    replace(
        render,
        "        Some(body_color(m, alpha)),\n        Default::default(),\n        None,\n",
        "        Some(body_color(m, alpha)),\n        &mut FrameDraw::default(),\n",
    )
    replace(
        render,
        "    let body_poses: &Poses = if hidden.is_empty() {",
        "    // The layers-only route always uses the unhidden poses.\n"
        "    let body_poses: &Poses = if paint.is_none() || frame.hidden.is_empty() {",
    )
    replace(render, "hide_parts(&mut hidden_poses, &hidden)", "hide_parts(&mut hidden_poses, &frame.hidden)")
    replace(
        render,
        """    if let Some(parts) = parts
        && anim::extracts_humanoid(m)
    {
        *parts = posed_parts(body_poses);
    }
""",
        """    if frame.capture_parts && anim::extracts_humanoid(m) {
        frame.parts = posed_parts(body_poses);
    }
""",
    )
    replace(invisible, "    Atlas, Mob, Part,", "    Atlas, FrameDraw, Mob, Part,")
    replace(invisible, "m, alpha, camera, atlas, light, data, extra, hidden, parts, out,", "m, alpha, camera, atlas, light, data, extra, frame, out,")

    replace(
        render,
        "            let (mut out, mut parts) = (Vec::new(), None);\n",
        """            let mut out = Vec::new();
            let mut frame_draw = FrameDraw {
                hidden,
                capture_parts: true,
                parts: None,
            };
""",
    )
    replace(render, "                hidden,\n                Some(&mut parts),\n", "                &mut frame_draw,\n")
    replace(render, '(out.len(), parts.expect("a humanoid"))', '(out.len(), frame_draw.parts.expect("a humanoid"))')

    for file in (living, perspective, gui):
        if file == living:
            replace(file, "use super::*;\n", "use super::*;\nuse sunrise_client::mobs::render::FrameDraw;\n")
        elif file == perspective:
            replace(file, "    mobs::{Anim, Brain, Mob, MobType},", "    mobs::{Anim, Brain, Mob, MobType, render::FrameDraw},")
        else:
            replace(file, "use sunrise_client::{hud::HudBatch, mesher::Vertex, mobs::Mob};", "use sunrise_client::{hud::HudBatch, mesher::Vertex, mobs::{Mob, render::FrameDraw}};")

    replace(
        living,
        "        let (mut skinned, mut parts, added) = (None, None, self.mod_layers(mob));\n",
        """        let (mut skinned, added) = (None, self.mod_layers(mob));
        let mut frame_draw = FrameDraw {
            hidden,
            capture_parts: listened || added.is_some(),
            parts: None,
        };
""",
    )
    for indent, count in [(16, 1), (20, 2)]:
        spaces = " " * indent
        replace(
            living,
            f"{spaces}hidden,\n{spaces}(listened || added.is_some()).then_some(&mut parts),\n",
            f"{spaces}&mut frame_draw,\n",
            count,
        )
    replace(living, "                    (listened || added.is_some()).then_some(&mut parts),\n", "                    &mut frame_draw,\n")
    replace(living, "        // AvatarRenderer.shouldRenderLayers:", "        let parts = frame_draw.parts;\n        // AvatarRenderer.shouldRenderLayers:")

    replace(
        perspective,
        "        let (mut parts, layers) = (None, self.mod_layers(m));\n",
        """        let layers = self.mod_layers(m);
        let mut frame_draw = FrameDraw {
            hidden,
            capture_parts: listened || layers.is_some(),
            parts: None,
        };
""",
    )
    replace(perspective, "                hidden,\n                (listened || layers.is_some()).then_some(&mut parts),\n", "                &mut frame_draw,\n")
    replace(perspective, "                (listened || layers.is_some()).then_some(&mut parts),\n", "                &mut frame_draw,\n")
    replace(perspective, "        // AvatarRenderer.shouldRenderLayers: a spectator draws no layers.\n", "        let parts = frame_draw.parts;\n        // AvatarRenderer.shouldRenderLayers: a spectator draws no layers.\n")
    replace(perspective, "data, &armour, None, &mut verts,", "data, &armour, &mut FrameDraw::default(), &mut verts,")
    replace(perspective, "&data, &armour, None, &mut out,", "&data, &armour, &mut FrameDraw::default(), &mut out,")
    replace(perspective, "                armour,\n                None,\n                out,\n", "                armour,\n                &mut FrameDraw::default(),\n                out,\n")

    replace(
        gui,
        "        let mut parts = None;\n",
        """        let mut frame_draw = FrameDraw {
            capture_parts: added.is_some(),
            ..Default::default()
        };
""",
    )
    replace(gui, "                Default::default(),\n                added.is_some().then_some(&mut parts),\n", "                &mut frame_draw,\n")
    replace(gui, "self.mod_layer_geometry(mob, None, parts, added, partial, camera, FULL_BRIGHT, out);", "self.mod_layer_geometry(mob, None, frame_draw.parts, added, partial, camera, FULL_BRIGHT, out);")

    replace(pose_tests, "        let (mut out, mut parts) = (Vec::new(), None);\n", "        let mut out = Vec::new();\n        let mut frame_draw = FrameDraw { capture_parts: keep, ..Default::default() };\n")
    replace(pose_tests, "            Default::default(),\n            keep.then_some(&mut parts),\n", "            &mut frame_draw,\n")
    replace(pose_tests, "            parts,\n", "            frame_draw.parts,\n")
    replace(pose_tests, "    let mut skinned = None;", "    let mut skinned = FrameDraw { capture_parts: true, ..Default::default() };")
    replace(pose_tests, "        Default::default(),\n        Some(&mut skinned),\n", "        &mut skinned,\n")
    replace(pose_tests, "assert_eq!(skinned,", "assert_eq!(skinned.parts,")
    replace(pose_tests, "    let mut layered = None;", "    let mut layered = FrameDraw { capture_parts: true, ..Default::default() };")
    replace(pose_tests, "        Some(&mut layered),\n", "        &mut layered,\n")
    replace(pose_tests, "assert_eq!(layered,", "assert_eq!(layered.parts,")
    for indent, count in [(12, 2), (16, 1)]:
        spaces = " " * indent
        replace(pose_tests, f"{spaces}hidden,\n{spaces}None,\n", f"{spaces}&mut FrameDraw {{ hidden, ..Default::default() }},\n", count)

    replace("mobs/skin.rs", "                        Default::default(),\n                        None,\n                        &mut gpu,\n", "                        &mut render::FrameDraw::default(),\n                        &mut gpu,\n")

    replace(setup_tests, "                    let mut parts = None;", "                    let mut frame_draw = render::FrameDraw { capture_parts: true, ..Default::default() };")
    replace(setup_tests, "                            Default::default(),\n                            Some(&mut parts),\n", "                            &mut frame_draw,\n", 2)
    replace(setup_tests, "                            Some(&mut parts),\n", "                            &mut frame_draw,\n")
    replace(setup_tests, "                    if let Some(parts) = parts {", "                    if let Some(parts) = frame_draw.parts {")
    replace(setup_tests, "                                Default::default(),\n                                None,\n                                &mut baseline,\n", "                                &mut render::FrameDraw::default(),\n                                &mut baseline,\n")

    replace(
        anim,
        "/// Part poses for one draw, keyed by part name.\n",
        """#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq))]
struct PoseEntry {
    pose: [f32; 9],
}

/// Part poses for one draw, keyed by part name.
""",
    )
    replace(anim, "FxHashMap<u64, [f32; 9]>", "FxHashMap<u64, PoseEntry>", 2)
    replace(anim, "map.entry(key(&p.name)).or_insert(p.pose);", "map.entry(key(&p.name)).or_insert(PoseEntry { pose: p.pose });")
    replace(anim, "            Some(p) => p,", "            Some(p) => &mut p.pose,")
    replace(anim, "self.map.get(&key(name)).copied()", "self.map.get(&key(name)).map(|entry| entry.pose)")
    replace(anim, "self.map.get(&key(&part.name)).copied().unwrap_or(part.pose)", "self.pose_of(&part.name).unwrap_or(part.pose)")
    replace(anim, ".map(|n| (key(n), [0.0; 9]))", ".map(|n| (key(n), PoseEntry { pose: [0.0; 9] }))", 10)
    replace(anim, ".insert(key(part), [0.0; 9]);", ".insert(key(part), PoseEntry { pose: [0.0; 9] });", 2)
    for key in ("name", '"head"'):
        pose = "[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]"
        replace(anim, f".insert(key({key}), {pose});", f".insert(key({key}), PoseEntry {{ pose: {pose} }});")

    for path, text in updated.items():
        path.write_text(text)


if __name__ == "__main__":
    main()
