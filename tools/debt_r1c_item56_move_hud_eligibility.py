#!/usr/bin/env python3
"""Move eligibility from the HUD paint sites into the original slot bodies (r1c-item56).

Run at the repository root on the status-slot version of Game::hud_layers, then
rustfmt --edition 2024 --config skip_children=true crates/sunrise-client/src/game.rs.
"""

from pathlib import Path
from textwrap import indent


path = Path("crates/sunrise-client/src/game.rs")
source = path.read_text()
start = source.index("        // Hud.extractRenderState: F1 (keyToggleGui)", source.index("    fn hud_layers("))
end = source.index("        // Screen.extractBlurredBackground", start)
frame = source[start:end]


def replace(old: str, new: str) -> None:
    global frame
    if frame.count(old) != 1:
        raise SystemExit(f"expected one HUD site starting with {old!r}")
    frame = frame.replace(old, new)


if frame.count("dispatch_status") != 3:
    raise SystemExit("expected the three status dispatch references")
frame = frame.replace("dispatch_status", "dispatch_hud")
replace(
    "        // RenderGuiEvent.Pre can veto the status slots even when F1 hides their vanilla paint.\n",
    "        // GuiLayerManager posts RenderGuiEvent.Pre even when F1 hides vanilla paint.\n",
)
replace(
    "        // The veto covers status slots even under F1, but not mandatory diagnostics.\n",
    "        // The veto covers the HUD pass, not mandatory diagnostics.\n",
)
replace(
    "        // The mods' layers (`register_gui_layers`) draw among vanilla's while the HUD shows.\n"
    "        self.hud_layer_next = show_hud.then_some(0);\n",
    "        let draw_sleep = &mut |g: &mut Self, batch: &mut HudBatch| {\n"
    "            let draws = hud_overlays::sleep_overlay(wi, hi, g.sleep_timer as i32);\n"
    "            hud_layout::paint(batch, &g.gui.atlas, &draws);\n"
    "        };\n",
)
replace(
    "            guide_from = batch.vertices.len();\n            // Hud.extractCrosshair",
    "            guide_from = batch.vertices.len();\n        }\n"
    "        if dispatch_hud {\n"
    "            self.hud_layer_next = Some(0);\n"
    "            // Hud.extractCrosshair",
)
replace(
    "            if self.camera_type.is_first_person()\n"
    "                && (!spectator || self.spectator_crosshair())\n"
    "                && !self.debug_crosshair()\n"
    "                && !self.overlay_holds_view()\n"
    "            {\n",
    "            self.gui_layer(VanillaGuiLayer::Crosshair, &mut batch, &mut |g, batch| {\n"
    "                if !show_hud\n"
    "                    || !g.camera_type.is_first_person()\n"
    "                    || (spectator && !g.spectator_crosshair())\n"
    "                    || g.debug_crosshair()\n"
    "                    || g.overlay_holds_view()\n"
    "                {\n"
    "                    return;\n"
    "                }\n",
)
crosshair_start = frame.index("                self.gui_layer(VanillaGuiLayer::Crosshair")
crosshair_end = frame.index("            self.gui_layer(VanillaGuiLayer::Hotbar", crosshair_start)
old = frame[crosshair_start:crosshair_end]
body = old.splitlines(keepends=True)[1:-2]
replace(old, "".join(line[4:] for line in body) + "            });\n")
replace("self.crosshair_living()", "g.crosshair_living()")
replace(
    "            self.gui_layer(VanillaGuiLayer::Hotbar, &mut batch, &mut |g, batch| {\n",
    "            self.gui_layer(VanillaGuiLayer::Hotbar, &mut batch, &mut |g, batch| {\n"
    "                if !show_hud {\n"
    "                    return;\n"
    "                }\n",
)
replace("                if !g.pvp_effects_shown() {", "                if show_hud && !g.pvp_effects_shown() {")
replace(
    "                g.draw_boss_bars(batch);\n"
    "            });\n"
    "            self.draw_extras(&mut batch);\n"
    "        }\n"
    "        if dispatch_hud {\n",
    "                if show_hud {\n"
    "                    g.draw_boss_bars(batch);\n"
    "                }\n"
    "            });\n"
    "            if show_hud {\n"
    "                self.draw_extras(&mut batch);\n"
    "            }\n",
)
replace("        }\n        if show_hud {\n            // Hud.nextContextualInfoState", "\n            // Hud.nextContextualInfoState")
replace(
    "                &mut |g, batch| {\n                    if contextual == ContextualBar::JumpableVehicle {",
    "                &mut |g, batch| {\n"
    "                    if !show_hud {\n"
    "                        return;\n"
    "                    }\n"
    "                    if contextual == ContextualBar::JumpableVehicle {",
)
paint = frame.index("            if !spectator {\n                self.gui_layer(\n                    VanillaGuiLayer::SelectedItemName")
message = frame.index("        let atlas = &self.gui.atlas;", paint)
frame = frame[:paint] + """            self.gui_layer(VanillaGuiLayer::SelectedItemName, &mut batch, &mut |g, batch| {
                if show_hud && !spectator {
                    g.draw_selected_name(batch, !g.creative);
                }
            });
            // Hud.extractSleepOverlay paints even when F1 hides the other vanilla bodies.
            self.gui_layer(VanillaGuiLayer::SleepOverlay, &mut batch, draw_sleep);
            self.gui_layer(VanillaGuiLayer::OverlayMessage, &mut batch, &mut |g, batch| {
                if show_hud {
                    g.draw_overlay_message(batch);
                }
            });
            self.gui_layer(VanillaGuiLayer::Title, &mut batch, &mut |g, batch| {
                if show_hud {
                    g.draw_title(batch);
                }
            });
            self.gui_layer(VanillaGuiLayer::ScoreboardSidebar, &mut batch, &mut |g, batch| {
                if show_hud {
                    g.draw_scoreboard(batch);
                }
            });
            self.gui_layer(VanillaGuiLayer::SubtitleOverlay, &mut batch, &mut |g, batch| {
                if subtitles != Subtitles::Hidden {
                    g.draw_subtitles(batch);
                }
            });
""" + frame[message:]
message = frame.index("        let atlas = &self.gui.atlas;", paint)
chat = frame.index("        // The chat screen draws the focused chat itself", message)
frame = frame[:message] + indent(frame[message:chat], "    ") + """            // The chat screen draws focused chat; the background slot still dispatches.
            self.gui_layer(VanillaGuiLayer::Chat, &mut batch, &mut |g, batch| {
                if show_hud && g.screen != Screen::Chat && !g.overlay_holds_view() {
                    g.draw_chat(batch);
                }
            });
            self.gui_layer(VanillaGuiLayer::TabList, &mut batch, &mut |g, batch| {
                if show_hud && g.screen != Screen::Chat {
                    g.draw_tab_list(batch);
                }
            });
            self.draw_mod_gui_layers(None, &mut batch);
            self.hud_layer_next = None;
        } else if replaying {
            // Replay's HUD-free view still draws the sleep tint and deferred screen subtitles.
            draw_sleep(self, &mut batch);
            if subtitles == Subtitles::Deferred {
                self.draw_subtitles(&mut batch);
            }
        }
"""
path.write_text(source[:start] + frame + source[end:])
