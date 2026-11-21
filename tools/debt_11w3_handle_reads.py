#!/usr/bin/env python3
"""One-shot caller and exception-row migration for the debt 11w3 scoped reads.

    python3 tools/debt_11w3_handle_reads.py menu --dry-run
    python3 tools/debt_11w3_handle_reads.py menu

Use one selector in the commit that cuts its declarations over. Add any
coordinated declaration hunks to that selector before applying it. The
allowlist matches exact source, not names across the tree: each receiver was
traced to the corresponding API handle. Engine implementations, grouped reads
and mutation/reentry boundaries belong to the semantic cut.
"""

import argparse
import re
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GAME = Path("crates/sunrise-client/src/game")
MODDING = GAME / "modding"
REMOTE = GAME / "network/remote"
VOCAB = Path("crates/sunrise-mod-vocab/src")
UNWINDING_METHODS = Path("crates/sunrise-mod-api-gen/unwinding-methods.txt")


@dataclass(frozen=True)
class Edit:
    old: str
    new: str
    count: int


def swap(anchor: str, replacement: str, count: int) -> Edit:
    """Replace the backtick-marked fragment, matching the entire literal anchor."""
    before, old, after = anchor.split("`")
    return Edit(before + old + after, before + replacement + after, count)


# Fallible fields still return their own Option. E.g. a menu slot's Some(None)
# means an empty slot; flatten just the resolution result, not the slot itself.
# Fields known at resolution (such as the block entity position) need no flatten.
EDITS = {
    "menu": {
        MODDING / "mod_menu_decisions_tests.rs": (
            swap("                    `player.container_menu(game).unwrap().slot_count(game)`,",
                 "player.container_menu(game).unwrap().with_view(game, |view| view.slot_count()).flatten()", 1),
        ),
        MODDING / "native_menu_inventory_tests.rs": (
            swap("                    `player.container_menu(game).unwrap().container_id(game)`,",
                 "player.container_menu(game).unwrap().with_view(game, |view| view.container_id()).flatten()", 1),
        ),
        MODDING / "native_guest_turn_tests.rs": (
            swap("            assert_eq!(`a_menu.slot_count(game)`, Some(46));",
                 "a_menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("            assert_eq!(`b_menu.slot_count(game)`, Some(46));",
                 "b_menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("                assert_eq!(`b_menu.slot_count(game)`, Some(46));",
                 "b_menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("                `b_menu.slot(game, 36)`,",
                 "b_menu.with_view(game, |view| view.slot(36)).flatten()", 1),
            swap("            `b_menu.slot(game, 36)`,",
                 "b_menu.with_view(game, |view| view.slot(36)).flatten()", 1),
        ),
        MODDING / "native_item_places.rs": (
            swap("            assert_eq!(`stale.carried(game)`, None);",
                 "stale.with_view(game, |view| view.carried()).flatten()", 1),
            swap("                Ok::<_, ()>(`menu.slot(host, 0)`)",
                 "menu.with_view(host, |view| view.slot(0)).flatten()", 2),
            Edit("            assert_eq!(menu.slot_holds(host, 1, &bucket), Some(1));\n"
                 "            assert_eq!(menu.slot_holds(host, 2, &bucket), Some(0));",
                 "            assert_eq!(menu.with_view(host, |view| view.slot_holds(1, &bucket)).flatten(), Some(1));\n"
                 "            assert_eq!(menu.with_view(host, |view| view.slot_holds(2, &bucket)).flatten(), Some(0));", 1),
            swap("            assert_eq!(`menu.slot_holds(host, 38, &bucket)`, Some(16));",
                 "menu.with_view(host, |view| view.slot_holds(38, &bucket)).flatten()", 1),
            swap("            assert_eq!(`menu.slot_holds(host, 39, &bucket)`, None);",
                 "menu.with_view(host, |view| view.slot_holds(39, &bucket)).flatten()", 1),
        ),
        MODDING / "native_menu_refs.rs": (
            swap("            assert_ne!(`menu.container_id(game)`, Some(0));",
                 "menu.with_view(game, |view| view.container_id()).flatten()", 1),
            swap("            assert_eq!(`menu.slot_count(game)`, None);",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("                `menu.slot(game, 2)`,",
                 "menu.with_view(game, |view| view.slot(2)).flatten()", 1),
            swap("            assert_eq!(`inventory.container_id(game)`, Some(0));",
                 "inventory.with_view(game, |view| view.container_id()).flatten()", 2),
            swap("            assert_eq!(`inventory.slot_count(game)`, Some(46));",
                 "inventory.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("            assert_eq!(`inventory.slot_count(game)`, None);",
                 "inventory.with_view(game, |view| view.slot_count()).flatten()", 3),
            swap("            assert_eq!(`inventory.slot(game, 0)`, Some(None));",
                 "inventory.with_view(game, |view| view.slot(0)).flatten()", 1),
            swap("            assert_eq!(`inventory.slot(game, 46)`, None);",
                 "inventory.with_view(game, |view| view.slot(46)).flatten()", 1),
            swap("            assert_eq!(`newer.slot_count(game)`, None);",
                 "newer.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("                `overworld_menu.slot(game, 0)`,",
                 "overworld_menu.with_view(game, |view| view.slot(0)).flatten()", 1),
            swap("                `nether_menu.slot(game, 0)`,",
                 "nether_menu.with_view(game, |view| view.slot(0)).flatten()", 1),
            Edit("                menu.slot(game, 0),\n                Some(Some(stone.try_into_api().unwrap()))",
                 "                menu.with_view(game, |view| view.slot(0)).flatten(),\n                Some(Some(stone.try_into_api().unwrap()))", 2),
            Edit("            assert_eq!(inventory.kind(game), Some(None));",
                 "            assert_eq!(\n                inventory.with_view(game, |view| assert_eq!(view.kind(), Some(None))),\n                Some(())\n            );", 1),
            swap("                let count = `menu.slot_count(host)`?;",
                 "menu.with_view(host, |view| view.slot_count()).flatten()", 1),
            *tuple(swap(f"                `game.menu_slot_backing({menu}, {slot})`,",
                        f"{menu}.with_view(game, |view| view.slot_backing({slot})).flatten()", count)
                   for menu, slot, count in [("inventory", 36, 1), ("inventory", 45, 1),
                                             ("menu", 0, 2), ("menu", 2, 1), ("menu", 9, 1),
                                             ("menu", 36, 1), ("overworld_menu", 0, 1),
                                             ("nether_menu", 0, 1)]),
            swap("            assert_eq!(`game.menu_slot_backing(menu, 45)`, None);",
                 "menu.with_view(game, |view| view.slot_backing(45)).flatten()", 1),
        ),
        MODDING / "native_menus.rs": (
            swap("                        assert_eq!(`menu.carried(host)`, Some(None));",
                 "menu.with_view(host, |view| view.carried()).flatten()", 1),
            swap("            assert_eq!(`menu.container_id(host)`, Some(1));",
                 "menu.with_view(host, |view| view.container_id()).flatten()", 1),
            swap("                            `menu.slot(host, 3)`,",
                 "menu.with_view(host, |view| view.slot(3)).flatten()", 1),
            swap("                            `menu.slot(host, 0)`,",
                 "menu.with_view(host, |view| view.slot(0)).flatten()", 1),
            swap("                        assert_eq!(`menu.slot_count(game)`, Some(0));",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("                        assert!(`menu.slot(game, 0)`.is_none());",
                 "menu.with_view(game, |view| view.slot(0)).flatten()", 1),
            swap("                            assert!(`game.menu_slot_count(menu)`.is_some());",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            Edit("                            assert_eq!(game.menu_carried(menu), Some(cursor));",
                 "                            assert_eq!(menu.with_view(game, |view| view.carried()).flatten(),\n"
                 "                                Some(cursor.try_into_api().unwrap()));", 1),
            *tuple(swap(f"                            assert_eq!(`game.menu_slot({menu}, 0)`, None);",
                        f"{menu}.with_view(game, |view| view.slot(0)).flatten()", 1)
                   for menu in ["stale", "wrong_window"]),
            Edit("                        assert_eq!(\n"
                 "                            game.menu_kind(game.menu(123, old).unwrap()),\n"
                 '                            Some(Some("minecraft:generic_9x3".into()))\n'
                 "                        );",
                 "                        assert_eq!(game.menu(123, old).unwrap().with_view(game, |view| {\n"
                 '                            assert_eq!(view.kind(), Some(Some("minecraft:generic_9x3")));\n'
                 "                        }), Some(()));", 1),
        ),
        MODDING / "native_menu_places.rs": (
            swap("            assert_eq!(`menu.slot_backing(host, 0)`, Some(MenuSlotBacking::Native(0)));",
                 "menu.with_view(host, |view| view.slot_backing(0)).flatten()", 2),
            swap("                `menu.slot_backing(host, 0)`,",
                 "menu.with_view(host, |view| view.slot_backing(0)).flatten()", 2),
            swap("            assert_eq!(`menu.slot_backing(host, 1)`, None);",
                 "menu.with_view(host, |view| view.slot_backing(1)).flatten()", 1),
        ),
        REMOTE / "client_menu_tests.rs": (
            swap("            assert_eq!(`menu.slot(game, 36)`.unwrap().unwrap().count(), 3);",
                 "menu.with_view(game, |view| view.slot(36)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 36)`, Some(None));",
                 "menu.with_view(game, |view| view.slot(36)).flatten()", 3),
            swap("        assert_eq!(`menu.slot(game, 36)`.unwrap().unwrap().count(), 3);",
                 "menu.with_view(game, |view| view.slot(36)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 36)`.unwrap().unwrap().count(), 2);",
                 "menu.with_view(game, |view| view.slot(36)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 37)`.unwrap().unwrap().count(), 3);",
                 "menu.with_view(game, |view| view.slot(37)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 37)`.unwrap().unwrap().count(), 2);",
                 "menu.with_view(game, |view| view.slot(37)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 37)`, Some(None));",
                 "menu.with_view(game, |view| view.slot(37)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 9)`.unwrap().unwrap().count(), 2);",
                 "menu.with_view(game, |view| view.slot(9)).flatten()", 1),
            swap("        assert_eq!(`menu.slot(game, 9)`, Some(None));",
                 "menu.with_view(game, |view| view.slot(9)).flatten()", 1),
            swap("            assert_eq!(`menu.carried(game)`, Some(None));",
                 "menu.with_view(game, |view| view.carried()).flatten()", 1),
            swap("        assert_eq!(`menu.carried(game)`.unwrap().unwrap().count(), 3);",
                 "menu.with_view(game, |view| view.carried()).flatten()", 2),
            swap("        assert_eq!(`menu.carried(game)`, Some(None));",
                 "menu.with_view(game, |view| view.carried()).flatten()", 2),
            swap("        assert_eq!(`chest.slot(game, 2)`, Some(None));",
                 "chest.with_view(game, |view| view.slot(2)).flatten()", 1),
            swap("        assert_eq!(`chest.slot(game, 2)`.unwrap().unwrap().count(), 4);",
                 "chest.with_view(game, |view| view.slot(2)).flatten()", 1),
            swap("        assert_eq!(`chest.carried(game)`.unwrap().unwrap().count(), 4);",
                 "chest.with_view(game, |view| view.carried()).flatten()", 1),
            swap("        assert_eq!(`chest.carried(game)`, Some(None));",
                 "chest.with_view(game, |view| view.carried()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(menu)`, Some(46));",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(menu)`, None);",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(crafting)`, Some(46));",
                 "crafting.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(crafting)`, None);",
                 "crafting.with_view(game, |view| view.slot_count()).flatten()", 2),
            swap("        assert_eq!(`game.menu_slot_count(chest)`, Some(63));",
                 "chest.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(inventory)`, None);",
                 "inventory.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(chest)`, None);",
                 "chest.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(before_replacement)`, None);",
                 "before_replacement.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("        assert_eq!(`game.menu_slot_count(before_logout)`, None);",
                 "before_logout.with_view(game, |view| view.slot_count()).flatten()", 2),
            swap("        assert_eq!(`game.menu_slot_count(reconnected)`, Some(46));",
                 "reconnected.with_view(game, |view| view.slot_count()).flatten()", 1),
            *tuple(swap(f"        assert_eq!(`game.menu_slot({menu}, {slot})`, {expected});",
                        f"{menu}.with_view(game, |view| view.slot({slot})).flatten()", 1)
                   for menu, slot, expected in [("menu", 46, "None"), ("menu", 36, "Some(None)"),
                                                ("chest", 3, "Some(None)")]),
            *tuple(Edit(f"        assert_eq!(game.menu_slot({menu}, {slot}).unwrap().unwrap().count, {count});",
                        f"        assert_eq!({menu}.with_view(game, |view| view.slot({slot})).flatten().unwrap().unwrap().count(), {count});", 1)
                   for menu, slot, count in [("menu", 36, 3), ("crafting", 1, 3),
                                             ("crafting", 0, 3), ("chest", 2, 4)]),
            Edit("        assert_eq!(game.menu_carried(menu).unwrap().unwrap().count, 3);",
                 "        assert_eq!(menu.with_view(game, |view| view.carried()).flatten().unwrap().unwrap().count(), 3);", 1),
        ),
        MODDING / "native_small_reads_tests.rs": (
            swap("            assert_eq!(`inventory.merchant_offers(game)`, Ok(None));",
                 "inventory.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("            assert_eq!(`menu.merchant_offers(game)`, Ok(Some(vec![])));",
                 "menu.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("                assert_eq!(`menu.merchant_offers(game)`, Err(LevelReadError::Unavailable));",
                 "menu.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("            let offers = `menu.merchant_offers(game)`.unwrap().unwrap();",
                 "menu.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("            assert_eq!(`menu.merchant_offers(game)`, Ok(None));",
                 "menu.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("            assert_eq!(`foreign.merchant_offers(game)`, Ok(None));",
                 "foreign.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("            assert_eq!(`replacement.merchant_offers(game)`, Ok(Some(vec![])));",
                 "replacement.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            swap("            assert_eq!(`replacement.merchant_offers(game)`, Ok(None));",
                 "replacement.with_client_view(game, |view| view.merchant_offers()).and_then(|scope| scope.transpose()).map(Option::flatten)", 1),
            *tuple(swap(f"            assert_eq!(`game.menu_slot_count({menu})`, {expected});",
                        f"{menu}.with_view(game, |view| view.slot_count()).flatten()", 1)
                   for menu, expected in [("inventory", "Some(46)"), ("inventory", "None"),
                                          ("open", "Some(63)"), ("open", "None")]),
            swap("                `game.menu_slot_count(game.player_container_menu(player).unwrap())`,",
                 "game.player_container_menu(player).unwrap().with_view(game, |view| view.slot_count()).flatten()", 1),
            Edit("            assert_eq!(game.menu_slot(inventory, 36), Some(Some(stack.clone())));\n"
                 "            assert_eq!(game.menu_carried(inventory), Some(Some(stack.clone())));",
                 "            assert_eq!(inventory.with_view(game, |view| view.slot(36)).flatten(),\n"
                 "                Some(Some(stack.try_into_api().unwrap())));\n"
                 "            assert_eq!(inventory.with_view(game, |view| view.carried()).flatten(),\n"
                 "                Some(Some(stack.try_into_api().unwrap())));", 1),
            Edit("            assert_eq!(game.menu_slot(open, 54), Some(Some(stack)));",
                 "            assert_eq!(open.with_view(game, |view| view.slot(54)).flatten(),\n"
                 "                Some(Some(stack.try_into_api().unwrap())));", 1),
        ),
        VOCAB / "hooks_tests.rs": (
            Edit("fn menu_slot_boxes_take_a_client_host(\n    host: &dyn ClientHost,\n    menu: MenuRef,\n) -> Option<Vec<Option<[f32; 4]>>> {\n    menu.slot_boxes(host)\n}",
                 "fn menu_slot_boxes_take_a_client_host(\n    host: &dyn ClientHost,\n    menu: MenuRef,\n) -> Option<Vec<Option<[f32; 4]>>> {\n    menu.with_client_view(host, |view| view.slot_boxes()).ok().flatten().flatten()\n}", 1),
        ),
        REMOTE / "native_entity_tests.rs": (
            swap("    assert_eq!(`game.menu_slot_count(inventory)`, Some(46));",
                 "inventory.with_view(&game, |view| view.slot_count()).flatten()", 1),
            swap("    assert_eq!(`game.menu_slot_count(inventory)`, None);",
                 "inventory.with_view(&game, |view| view.slot_count()).flatten()", 1),
            swap("    assert_eq!(`game.menu_slot_count(menu)`, Some(45));",
                 "menu.with_view(&game, |view| view.slot_count()).flatten()", 1),
            Edit("    let gem = game\n        .menu_slot(menu, 0)\n        .flatten()\n"
                 '        .expect("the chest\'s first slot");\n'
                 '    assert_eq!((&*gem.item, gem.count), ("minecraft:diamond", 2));\n'
                 '    assert_eq!(gem.custom_name().as_deref(), Some("Gem"));\n'
                 "    assert_eq!(game.menu_slot(menu, 1), Some(None));\n"
                 "    let stone = game\n        .menu_slot(menu, 36)\n        .flatten()\n"
                 '        .expect("the hotbar\'s first slot");\n'
                 '    assert_eq!((&*stone.item, stone.count), ("minecraft:stone", 5));',
                 "    let gem = menu.with_view(&game, |view| view.slot(0)).flatten().flatten()\n"
                 '        .expect("the chest\'s first slot");\n'
                 '    assert_eq!((gem.item().id(&game).as_str(), gem.count()), ("minecraft:diamond", 2));\n'
                 '    assert_eq!(gem.components().and_then(|patch| patch.as_nbt().get("minecraft:custom_name")),\n'
                 '        Some(&Tag::String("Gem".into())));\n'
                 "    assert_eq!(menu.with_view(&game, |view| view.slot(1)).flatten(), Some(None));\n"
                 "    let stone = menu.with_view(&game, |view| view.slot(36)).flatten().flatten()\n"
                 '        .expect("the hotbar\'s first slot");\n'
                 '    assert_eq!((stone.item().id(&game).as_str(), stone.count()), ("minecraft:stone", 5));', 1),
        ),
        MODDING / "native_menu_slot_kinds.rs": (
            swap("            assert_eq!(`game.menu_slot_kinds(menu)`, None);",
                 "menu.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 2),
            swap("            let inventory = `game.menu_slot_kinds(menu)`.unwrap();",
                 "menu.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 1),
            swap("            assert_eq!(inventory.len(), `game.menu_slot_count(menu)`.unwrap());",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("            assert_eq!(`game.menu_slot_kinds(menu)`, Some(inventory));",
                 "menu.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 1),
            swap("            assert_eq!(`game.menu_slot_kinds(stale)`, None);",
                 "stale.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 1),
            swap("            assert_eq!(`game.menu_slot_kinds(other)`, None);",
                 "other.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 1),
            swap("                let kinds = `game.menu_slot_kinds(menu)`.unwrap();",
                 "menu.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 1),
            swap("                assert_eq!(Some(kinds.len()), `game.menu_slot_count(menu)`);",
                 "menu.with_view(game, |view| view.slot_count()).flatten()", 1),
            swap("            assert_eq!(`game.menu_slot_kinds(missing)`, None);",
                 "missing.with_client_view(game, |view| view.slot_kinds()).ok().flatten().flatten()", 1),
            swap("            assert_eq!(`game.menu_slot_count(missing)`, None);",
                 "missing.with_view(game, |view| view.slot_count()).flatten()", 1),
        ),
        GAME / "runtime/transactions.rs": (
            swap("                `menu.slot(game, 1)`,",
                 "menu.with_view(game, |view| view.slot(1)).flatten()", 2),
            swap("                assert_eq!(`menu.slot(game, 1)`.unwrap().unwrap().count(), 9);",
                 "menu.with_view(game, |view| view.slot(1)).flatten()", 1),
        ),
        Path("crates/sunrise-client/tests/it/mod_api_hooks.rs"): (
            Edit("            let _ = menu.slot_count(host);\n            let _ = menu.slot(host, 0);\n            let _ = menu.carried(host);",
                 "            let _ = menu.with_view(host, |view| {\n                let _ = view.slot_count();\n                let _ = view.slot(0);\n                let _ = view.carried();\n            });", 1),
        ),
    },
    # Mode flags and XP are known once the live player resolves; the enum,
    # abilities, food and derived food predicates can still be unavailable.
    "player": {
        MODDING / "native_entity_commands.rs": (
            swap("            assert_eq!(`player.game_mode(game)`, Some(GameMode::Survival));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
            swap("                        assert_eq!(`player.game_mode(game)`, Some(GameMode::Survival));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
            swap("                        assert_eq!(`player.game_mode(game)`, Some(GameMode::Creative));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
            swap("            assert_eq!(`player.game_mode(game)`, Some(GameMode::Creative));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
        ),
        MODDING / "native_entity_refs.rs": (
            swap("            assert_eq!(`player.game_mode(game)`, Some(GameMode::Survival));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
            swap("            assert_eq!(`player.game_mode(game)`, Some(GameMode::Creative));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
        ),
        MODDING / "native_guest_turn_tests.rs": (
            swap("                assert_eq!(`a.game_mode(game)`, None, \"body remains displaced\");",
                 "a.with_view(game, |view| view.game_mode()).flatten()", 1),
        ),
        REMOTE / "client_player_reads_tests.rs": (
            swap("        assert_eq!(`player.game_mode(game)`, Some(GameMode::Survival));",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 2),
            swap("            assert_eq!(`player.game_mode(game)`, Some(mode), \"{param}\");",
                 "player.with_view(game, |view| view.game_mode()).flatten()", 1),
            swap("        assert_eq!(`player.may_build(game)`, Some(true));",
                 "player.with_view(game, |view| view.may_build())", 1),
            swap("        assert_eq!(`player.abilities(game)`.map(|a| a.may_fly), Some(false));",
                 "player.with_view(game, |view| view.abilities()).flatten()", 1),
            swap("        let food = `player.food(game)`.unwrap();",
                 "player.with_view(game, |view| view.food()).flatten()", 1),
            swap("        assert_eq!(`player.needs_food(game)`, Some(true));",
                 "player.with_view(game, |view| view.needs_food()).flatten()", 1),
            swap("        assert_eq!(`player.has_enough_food(game)`, Some(false));",
                 "player.with_view(game, |view| view.has_enough_food()).flatten()", 1),
            swap("        let xp = `player.experience(game)`.unwrap();",
                 "player.with_view(game, |view| view.experience())", 1),
            swap("        assert_eq!(`player.xp_needed_for_next_level(game)`, Some(13));",
                 "player.with_view(game, |view| view.xp_needed_for_next_level())", 1),
            swap("        assert_eq!(`other.game_mode(game)`, None);",
                 "other.with_view(game, |view| view.game_mode()).flatten()", 1),
            swap("        assert_eq!(`other.food(game)`, None);",
                 "other.with_view(game, |view| view.food()).flatten()", 1),
        ),
        REMOTE / "player_tick_tests.rs": (
            swap("            xp: `player.experience(host)`.map_or(0, |xp| xp.total),",
                 "player.with_view(host, |view| view.experience())", 1),
        ),
        GAME / "items/item_use.rs": (
            swap("                    `event.player.food(host)`.unwrap().level,",
                 "event.player.with_view(host, |view| view.food()).flatten()", 1),
        ),
        MODDING / "native_small_reads_tests.rs": (
            swap("                let abilities = `local.abilities(&*game as &dyn mod_api::ClientHost)`.unwrap();",
                 "local.with_view(&*game as &dyn mod_api::ClientHost, |view| view.abilities()).flatten()", 1),
        ),
    },
    "entity": {
        # Fits::can_equip is below the protected dispenser hunks.
        GAME / "blocks/dispenser.rs": (
            swap("            let at = `event.entity.entity().position(host)`;",
                 "event.entity.entity().with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "blocks/dispenser/decision_tests.rs": (
            swap("        assert_eq!(`event.entity.entity().position(host)`.unwrap()[1], 64.0);",
                 "event.entity.entity().with_view(host, |view| view.position()).flatten()", 1),
            swap("        assert!(`event.entity.entity().kind(host)`.is_some());",
                 "event.entity.entity().with_view(host, |view| view.kind()).flatten()", 1),
        ),
        GAME / "entities/falling_tick_tests.rs": (
            swap("            .push((event.entity.id(), `event.entity.position(host)`));",
                 "event.entity.with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "entities/general/save_load_tests.rs": (
            swap("        assert_eq!(`pig_ref.position(game)`, Some([2.5, 65.25, 1.5]));",
                 "pig_ref.with_view(game, |view| view.position()).flatten()", 1),
            swap("            `leaf_ref.position(game)`,",
                 "leaf_ref.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`root.position(game)`, Some(to));",
                 "root.with_view(game, |view| view.position()).flatten()", 2),
            swap("            assert_eq!(`root.position(game)`, None);",
                 "root.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`rider.position(game)`, Some(to));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`rider.position(game)`, Some(rider_to));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("            assert_eq!(`rider.position(game)`, Some([0.5, 64.0, 0.5]));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`rider.position(game)`, Some([0.5, 64.0, 0.5]));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`rider.rotation(game)`, Some([90.0, 30.0]));",
                 "rider.with_view(game, |view| view.rotation()).flatten()", 3),
            swap("        assert_eq!(`robot.kind(game)`.unwrap().id(game), \"mymod:robot\");",
                 "robot.with_view(game, |view| view.kind()).flatten()", 1),
            swap("        assert_eq!(`robot.position(game)`, None);",
                 "robot.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`ghost.kind(game)`.unwrap().id(game), \"mymod:ghost\");",
                 "ghost.with_view(game, |view| view.kind()).flatten()", 1),
            swap("        assert_eq!(`ghost.position(game)`, Some([0.5, 64.0, 0.5]));",
                 "ghost.with_view(game, |view| view.position()).flatten()", 1),
        ),
        GAME / "entities/lightning.rs": (
            swap("                    .and_then(|entity| `entity.kind(host)`.map(|kind| kind.id(host)));",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
            swap("                    outcome: outcome.and_then(|entity| `entity.kind(host)`.map(|kind| kind.id(host))),",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        GAME / "entities/passengers.rs": (
            swap("                assert!(`removed.position(host)`.is_none());",
                 "removed.with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "entities/text_displays.rs": (
            swap("            client.move_to(`entity.position(game)`.unwrap(), None);",
                 "entity.with_view(game, |view| view.position()).flatten()", 1),
        ),
        GAME / "items/consume.rs": (
            swap("            assert_eq!(`event.consumer.entity().position(host)`, Some(event.from));",
                 "event.consumer.entity().with_view(host, |view| view.position()).flatten()", 1),
            swap("                            assert_eq!(`next.position(host)`, Some([11.5, 64.0, 8.5]));",
                 "next.with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "mobs/creatures/borrowed_panic_tests.rs": (
            swap("            `cx.mob().entity().position(cx.host())`,",
                 "cx.mob().entity().with_view(cx.host(), |view| view.position()).flatten()", 1),
        ),
        GAME / "mobs/creatures/mob_pearl_hook_tests.rs": (
            swap("        assert_eq!(`event.owner.position(host)`, Some(event.from));",
                 "event.owner.with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "mobs/creatures/mob_tick_access_tests.rs": (
            swap("        let own = `event.mob.entity().position(host)`;",
                 "event.mob.entity().with_view(host, |view| view.position()).flatten()", 1),
            swap("            .and_then(|entity| `entity.position(host)`);",
                 "entity.with_view(host, |view| view.position()).flatten()", 3),
            swap("            spawned_position: `spawned.position(host)`,",
                 "spawned.with_view(host, |view| view.position()).flatten()", 1),
            swap("                    assert_eq!(`entity.position(game)`, Some([10.0, 80.0, 0.5]));",
                 "entity.with_view(game, |view| view.position()).flatten()", 1),
        ),
        GAME / "mobs/mob_looting.rs": (
            swap("            let standing = `entity.position(host)`.is_some_and(|[_, y, _]| y >= 64.0);",
                 "entity.with_view(host, |view| view.position()).flatten()", 1),
        ),
        MODDING / "entity_sections/tests.rs": (
            swap("            HostSide::Server(host) => `event.entity.position(host)`,",
                 "event.entity.with_view(host, |view| view.position()).flatten()", 1),
            swap("            HostSide::Client(host) => `event.entity.position(host)`,",
                 "event.entity.with_view(host, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_entities.rs": (
            swap("                    `entity.kind(host)`.map(|kind| kind.id(host)).as_deref(),",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
            swap("                assert_eq!(`entity.position(host)`, Some(event.pos));",
                 "entity.with_view(host, |view| view.position()).flatten()", 1),
            swap("                assert!(handles.iter().all(|e| `e.kind(game)`.is_none()));",
                 "e.with_view(game, |view| view.kind()).flatten()", 1),
            swap("                        .all(|e| `e.position(game)` == Some([5.0, 64.0, 5.0]))",
                 "e.with_view(game, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_entity_moves.rs": (
            swap("            assert_eq!(`entity.position(game)`, Some([5.0, 70.0, -3.0]));",
                 "entity.with_view(game, |view| view.position()).flatten()", 2),
            swap("            assert_eq!(`entity.rotation(game)`, Some([90.0, 90.0]));",
                 "entity.with_view(game, |view| view.rotation()).flatten()", 1),
            swap("            assert_eq!(`entity.rotation(game)`, Some([15.0, 0.0]));",
                 "entity.with_view(game, |view| view.rotation()).flatten()", 1),
            swap("            assert_eq!(`entity.rotation(game)`, Some([0.0, 90.0]));",
                 "entity.with_view(game, |view| view.rotation()).flatten()", 1),
        ),
        MODDING / "native_entity_refs.rs": (
            swap("                assert_eq!(`entity.position(game)`, Some([3.0, 64.0, 5.0]));",
                 "entity.with_view(game, |view| view.position()).flatten()", 1),
            swap("                assert_eq!(`other.position(game)`, Some([3.0, 64.0, 5.0]));",
                 "other.with_view(game, |view| view.position()).flatten()", 1),
            swap("            assert_eq!(`other.position(game)`, Some([3.0, 64.0, 5.0]));",
                 "other.with_view(game, |view| view.position()).flatten()", 1),
            swap("                `other.kind(game)`.map(|kind| kind.id(game)).as_deref(),",
                 "other.with_view(game, |view| view.kind()).flatten()", 1),
        ),
        MODDING / "native_entity_state.rs": (
            swap("                `entity.position(game)`.is_some(),",
                 "entity.with_view(game, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_goal_ticks/live_read_tests.rs": (
            swap("            `player.entity().position(game)`",
                 "player.entity().with_view(game, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_guest_turn_tests.rs": (
            swap("            assert_eq!(`rider.position(game)`, Some([3.0, 64.0, 5.0]));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("            assert_eq!(`rider.position(game)`, Some(before));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("            assert_eq!(`rider.position(game)`, Some(after));",
                 "rider.with_view(game, |view| view.position()).flatten()", 1),
            swap("            assert_eq!(`online[1].entity().position(game)`.unwrap(), [3.0, 64.0, 5.0]);",
                 "online[1].entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("            let own = `a.entity().position(game)`.unwrap();",
                 "a.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("                `a.entity().position(game)`,",
                 "a.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("        let before = `b.entity().position(game)`.unwrap();",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`b.entity().position(game)`, Some(now));",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("        let held = `b.entity().position(game)`;",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`b.entity().position(game)`, held);",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("            let before = `b.entity().position(game)`;",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("            assert_eq!(`b.entity().position(game)`, before);",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 1),
            swap("        let [x, y, z] = `b.entity().position(game)`.unwrap();",
                 "b.entity().with_view(game, |view| view.position()).flatten()", 3),
            swap("            assert_eq!(`vehicle.position(game)`, Some([10.0, 70.0, -4.0]));",
                 "vehicle.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`stand.position(game)`, Some([3.5, 70.0, 0.5]));",
                 "stand.with_view(game, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_item_places.rs": (
            swap("                `cart.kind(host)`.map(|kind| kind.id(host)).as_deref(),",
                 "cart.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        MODDING / "native_local_player_writes.rs": (
            swap("                        `event.entity.position(host)`.unwrap(),",
                 "event.entity.with_view(host, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_player_destroyed_block_tests.rs": (
            swap("            `entity.kind(host)`.map(|kind| kind.id(host)).as_deref() == Some(\"minecraft:item\")",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        MODDING / "native_player_spectating_tests.rs": (
            swap("    `host.entity(u32::try_from(player).ok()?)?.position(host)`",
                 "host.entity(u32::try_from(player).ok()?)?.with_view(host, |view| view.position()).flatten()", 1),
        ),
        MODDING / "native_server.rs": (
            swap("                assert_eq!(`entity.rotation(host)`, Some([123.0, 17.0]));",
                 "entity.with_view(host, |view| view.rotation()).flatten()", 1),
        ),
        GAME / "native_hooks/death_source_tests.rs": (
            swap("        let kind = `entity.kind(host)`.expect(\"dying entity has a type\").id(host);",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        GAME / "native_hooks/farmland_trample.rs": (
            swap("                    `event.entity.position(host)`.unwrap()[1],",
                 "event.entity.with_view(host, |view| view.position()).flatten()", 1),
            swap("                y: `entity.position(host)`.unwrap()[1],",
                 "entity.with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "native_hooks/level_tick_tests.rs": (
            swap("                    .map(|entity| (entity.id(), `entity.position(host)`.unwrap()));",
                 "entity.with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "native_hooks/on_entity_item_update.rs": (
            swap("                assert_eq!(`event.item.entity().position(host)`, None);",
                 "event.item.entity().with_view(host, |view| view.position()).flatten()", 1),
        ),
        GAME / "network/lan_actions/tests/player_entity_tick_tests.rs": (
            swap("                    .filter_map(|&id| Some((id, `host.entity(id)?.position(host)`?))),",
                 "host.entity(id)?.with_view(host, |view| view.position()).flatten()", 1),
        ),
        REMOTE / "client_camera_tests.rs": (
            swap("        assert_eq!(`local.position(game)`, Some([0.0; 3]));",
                 "local.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`local.position(game)`, None, \"old level handles stay stale\");",
                 "local.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`attached.position(game)`, Some([10.0, 64.0, 12.0]));",
                 "attached.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`fresh.position(game)`, Some([0.0; 3]));",
                 "fresh.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`fresh.position(game)`, None);",
                 "fresh.with_view(game, |view| view.position()).flatten()", 1),
        ),
        REMOTE / "client_handles_tests.rs": (
            swap("        assert_eq!(`me.position(game)`, Some([1.5, 65.0, -2.5]));",
                 "me.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`me.position(game)`, None);",
                 "me.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert_eq!(`me.rotation(game)`, Some([90.0, 10.0]));",
                 "me.with_view(game, |view| view.rotation()).flatten()", 1),
            swap("        assert_eq!(`other.position(game)`, Some([10.0, 64.0, 12.0]));",
                 "other.with_view(game, |view| view.position()).flatten()", 1),
            swap("        assert!(`other.rotation(game)`.is_some());",
                 "other.with_view(game, |view| view.rotation()).flatten()", 1),
            swap("        assert_eq!(`untracked.position(game)`, None);",
                 "untracked.with_view(game, |view| view.position()).flatten()", 1),
        ),
        REMOTE / "general.rs": (
            swap("                assert_eq!(`row.position(game)`, Some([2.5, 71.25, 1.5]));",
                 "row.with_view(game, |view| view.position()).flatten()", 1),
            swap("                assert_eq!(`row.position(game)`, Some([2.5 - 1.7, 74.0, 0.5]));",
                 "row.with_view(game, |view| view.position()).flatten()", 1),
            swap("                assert_eq!(`ghast.position(game)`, Some([2.5, 70.0, 0.5]));",
                 "ghast.with_view(game, |view| view.position()).flatten()", 1),
        ),
        GAME / "network/remote_projectiles.rs": (
            swap("                    .map(|&id| `host.entity(id).unwrap().position(*host)`.unwrap())",
                 "host.entity(id).unwrap().with_view(*host, |view| view.position()).flatten()", 1),
        ),
        GAME / "runtime/transaction_entities_tests.rs": (
            swap("            *KIND_OF_PENDING.lock().unwrap() = Some(`entity.kind(host)`.map(|key| key.id(host)));",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        VOCAB / "entity/entity_capability.rs": (
            swap("            if !builtin_player_inventory_type(&`entity.kind(host)`?.id(host)) {",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
            swap("            .then(|| `self.kind(host)`.map(|kind| kind.id(host)))",
                 "self.with_view(host, |view| view.kind()).flatten()", 1),
            Edit("fn kind_of(host: &dyn Host, entity: EntityRef) -> String {\n    entity\n        .kind(host)",
                 "fn kind_of(host: &dyn Host, entity: EntityRef) -> String {\n    entity\n        .with_view(host, |view| view.kind())\n        .flatten()", 1),
        ),
        VOCAB / "parts/entities.rs": (
            swap("        .get(`entity.kind(host)`?.id(host).as_str())?;",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        VOCAB / "payload/typed.rs": (
            swap("                `player.entity().position(host)`.is_some_and(|at| {",
                 "player.entity().with_view(host, |view| view.position()).flatten()", 1),
        ),
        VOCAB / "testing/handle_level_tests.rs": (
            swap("    let kind = `player.entity().kind(&host)`.unwrap();",
                 "player.entity().with_view(&host, |view| view.kind()).flatten()", 1),
        ),
        REMOTE / "native_entity_tests.rs": (
            swap("    assert_eq!(`game.entity_position(me)`, Some([0.5, 65.0, 0.5]));",
                 "me.with_view(&game, |view| view.position()).flatten()", 1),
            swap("    assert_eq!(`game.entity_position(stand)`, Some([1.5, 64.0, 2.5]));",
                 "stand.with_view(&game, |view| view.position()).flatten()", 1),
            swap("    assert_eq!(`game.entity_position(display)`, Some([4.0, 70.0, -3.0]));",
                 "display.with_view(&game, |view| view.position()).flatten()", 1),
            swap("    assert_eq!(`game.entity_position(stand)`, None);",
                 "stand.with_view(&game, |view| view.position()).flatten()", 1),
            swap("    assert_eq!(`game.entity_block_position(stand)`, Some([1, 64, 2]));",
                 "stand.with_view(&game, |view| view.block_position()).flatten()", 1),
            swap("    assert_eq!(`game.entity_rotation(me)`, Some([45.0, 10.0]));",
                 "me.with_view(&game, |view| view.rotation()).flatten()", 1),
            swap("    assert_eq!(`game.entity_rotation(stand)`, Some([90.0, 0.0]));",
                 "stand.with_view(&game, |view| view.rotation()).flatten()", 1),
            swap("        `game.entity_kind(me)`.map(|kind| kind.id(&game)).as_deref(),",
                 "me.with_view(&game, |view| view.kind()).flatten()", 1),
            swap("        `game.entity_kind(stand)`",
                 "stand.with_view(&game, |view| view.kind()).flatten()", 1),
            swap("        `game.entity_kind(display)`",
                 "display.with_view(&game, |view| view.kind()).flatten()", 1),
            swap("    assert_eq!(`game.entity_kind(display)`, None);",
                 "display.with_view(&game, |view| view.kind()).flatten()", 1),
        ),
        Path("crates/sunrise-mod-api-gen/src/dispatch.rs"): (
            swap("                            \"`entity(|| {key}.kind({host}).and_then(`super::content::registry_keys::entity_type_name))\"",
                 "entity(|| {key}.with_view({host}, |view| view.kind()).flatten().and_then(", 1),
        ),
        Path("crates/sunrise-mod-host/examples/native_waypoints.rs"): (
            Edit("                player\n                    .entity()\n                    .position(host)",
                 "                player\n                    .entity()\n                    .with_view(host, |view| view.position())\n                    .flatten()", 1),
        ),
        GAME / "mobs/creatures/enchanted_entity_loot_tests.rs": (
            Edit("                .entity()\n                .kind(&*host)",
                 "                .entity()\n                .with_view(&*host, |view| view.kind())\n                .flatten()", 1),
            swap("                .and_then(`|a| a.kind(&*host)`)",
                 "|a| a.with_view(&*host, |view| view.kind()).flatten()", 1),
        ),
    },
    "block-entity": {
        MODDING / "native_block_entity_item_tests.rs": (
            swap("                let level = `entity.level(host)`.unwrap();",
                 "entity.with_view(host, |view| view.level())", 1),
            swap("            let level = `entity.level(game)`.unwrap();",
                 "entity.with_view(game, |view| view.level())", 11),
            swap("                let level = `entity.level(game)`.unwrap();",
                 "entity.with_view(game, |view| view.level())", 1),
        ),
        MODDING / "native_block_entity_refs.rs": (
            swap("                `chest.kind(game)`.map(|kind| kind.id(game)),",
                 "chest.with_view(game, |view| view.kind()).flatten()", 1),
            swap("                assert_eq!(`chest.position(game)`, None);",
                 "chest.with_view(game, |view| view.position())", 1),
            swap("            assert_eq!(`chest.position(game)`, Some([0, 64, 0]));",
                 "chest.with_view(game, |view| view.position())", 1),
            swap("            `assert_eq!(replacement.data_slots(game), Some(vec![]));`",
                 "assert_eq!(replacement.with_view(game, |view| view.data_slots().map(|slots| slots.is_empty())).flatten(), Some(true));", 1),
        ),
        GAME / "blocks/machines/contents_watch_tests.rs": (
            swap("            assert_eq!(`other.level(game)`, Some(level));",
                 "other.with_view(game, |view| view.level())", 1),
        ),
        GAME / "native_hooks/block_entity_client_tick_tests.rs": (
            swap("    assert_eq!(`event.block_entity.position(host)`, Some(event.pos));",
                 "event.block_entity.with_view(host, |view| view.position())", 1),
            swap("        let pos = `event.entity.position(host)`.unwrap();",
                 "event.entity.with_view(host, |view| view.position())", 1),
        ),
        REMOTE / "block_entity_data_tests.rs": (
            swap("            `event.entity.position(host)`.unwrap(),",
                 "event.entity.with_view(host, |view| view.position())", 1),
        ),
        GAME / "native_hooks/shared.rs": (
            swap("                            assert_eq!(`handle.kind(game)`.as_ref(), keys.vanilla.get());",
                 "handle.with_view(game, |view| view.kind()).flatten()", 1),
            swap("                        assert_eq!(`handle.kind(game)`.as_ref(), keys.shared.get());",
                 "handle.with_view(game, |view| view.kind()).flatten()", 1),
        ),
        GAME / "runtime/stateful_block_entity_tests.rs": (
            swap("        assert_eq!(`handle.kind(game)`.as_ref(), observed.kind.get());",
                 "handle.with_view(game, |view| view.kind()).flatten()", 2),
            swap("        assert_eq!(`second.kind(game)`.as_ref(), observed.kind.get());",
                 "second.with_view(game, |view| view.kind()).flatten()", 1),
        ),
        VOCAB / "item/capability.rs": (
            swap("        `entity.kind(host)`",
                 "entity.with_view(host, |view| view.kind()).flatten()", 1),
        ),
        MODDING / "native_live_holder_tests.rs": (
            swap("            let value = `sink.data_slots(host).unwrap()[0]`;",
                 "sink.with_view(host, |view| view.data_slots().unwrap()[0]).unwrap()", 1),
            swap("            `assert_eq!(entity.data_slots(game), Some(vec![]));`",
                 "assert_eq!(entity.with_view(game, |view| view.data_slots().map(|slots| slots.is_empty())).flatten(), Some(true));", 1),
            swap("            `assert_eq!(entity.data_slots(game), Some(vec![0]));`",
                 "assert_eq!(entity.with_view(game, |view| view.data_slots().map(|slots| slots == [0].as_slice())).flatten(), Some(true));", 2),
            swap("            `assert_eq!(entity.data_slots(game), Some(vec![1]));`",
                 "assert_eq!(entity.with_view(game, |view| view.data_slots().map(|slots| slots == [1].as_slice())).flatten(), Some(true));", 1),
            swap("            `let before = entity.data_slots(game).unwrap();`",
                 "let before = entity.with_view(game, |view| view.data_slots().unwrap().to_vec()).unwrap();", 1),
            swap("            `assert_eq!(entity.data_slots(game), Some(before));`",
                 "assert_eq!(entity.with_view(game, |view| view.data_slots().map(|slots| slots == before.as_slice())).flatten(), Some(true));", 1),
        ),
        MODDING / "block_entity_tick_reply_tests/fixture.rs": (
            swap("            let value = `sink.data_slots(host).unwrap()[1]`;",
                 "sink.with_view(host, |view| view.data_slots().unwrap()[1]).unwrap()", 1),
        ),
        GAME / "blocks/machines/storage_attempt_tests.rs": (
            swap("            `assert_eq!(entity.data_slots(game), Some(vec![1]));`",
                 "assert_eq!(entity.with_view(game, |view| view.data_slots().map(|slots| slots == [1].as_slice())).flatten(), Some(true));", 1),
        ),
        MODDING / "block_entity_part_state_tests.rs": (
            swap("        `assert_eq!(entity.data_slots(game), Some(vec![42]));`",
                 "assert_eq!(entity.with_view(game, |view| view.data_slots().map(|slots| slots == [42].as_slice())).flatten(), Some(true));", 1),
        ),
        Path("crates/sunrise-mod-api-gen/src/dispatch.rs"): (
            swap("                            \"`block_entity_kind(|| {key}.kind({host}).and_then(`super::content::registry_keys::block_entity_type_name))\"",
                 "block_entity_kind(|| {key}.with_view({host}, |view| view.kind()).flatten().and_then(", 1),
        ),
    },
    # A valid unloaded chunk still lends a view. Slime status does not need a
    # column, while ordinary status and palette searches do.
    "chunk": {
        MODDING / "native_chunk_queries.rs": (
            swap("            assert_eq!(`chunk.sections_with_block(game, ore)`, Some(vec![-2, 1]));",
                 "chunk.with_view(game, |view| view.sections_with_block(ore)).flatten()", 1),
            swap("            assert_eq!(`chunk.sections_with_block(game, ore)`, None);",
                 "chunk.with_view(game, |view| view.sections_with_block(ore)).flatten()", 1),
            swap("            assert_eq!(`chunk.sections_with_block(game, emerald)`, Some(vec![]));",
                 "chunk.with_view(game, |view| view.sections_with_block(emerald)).flatten()", 1),
            swap("            assert_eq!(`unloaded.sections_with_block(game, ore)`, None);",
                 "unloaded.with_view(game, |view| view.sections_with_block(ore)).flatten()", 1),
            swap("                assert_eq!(`chunk.is_slime_chunk(game)`, Some(expected));",
                 "chunk.with_view(game, |view| view.is_slime_chunk())", 2),
            swap("            assert_eq!(`invalid.is_slime_chunk(game)`, None);",
                 "invalid.with_view(game, |view| view.is_slime_chunk())", 1),
            swap("            assert_eq!(`parked.is_slime_chunk(game)`, Some(false));",
                 "parked.with_view(game, |view| view.is_slime_chunk())", 1),
            swap("            assert_eq!(`parked.is_slime_chunk(game)`, None);",
                 "parked.with_view(game, |view| view.is_slime_chunk())", 1),
            swap("            assert_eq!(`active.is_slime_chunk(game)`, Some(true));",
                 "active.with_view(game, |view| view.is_slime_chunk())", 1),
            swap("            assert_eq!(`active.is_slime_chunk(game)`, None);",
                 "active.with_view(game, |view| view.is_slime_chunk())", 2),
            swap("            assert_eq!(`ChunkRef::new(unopened, [0, 0]).is_slime_chunk(game)`, None);",
                 "ChunkRef::new(unopened, [0, 0]).with_view(game, |view| view.is_slime_chunk())", 1),
            swap("            assert_eq!(`ChunkRef::new(level, [0, 0]).is_slime_chunk(game)`, None);",
                 "ChunkRef::new(level, [0, 0]).with_view(game, |view| view.is_slime_chunk())", 1),
        ),
        MODDING / "native_entity_refs.rs": (
            swap("            assert!(`chunk.loaded(game)`);",
                 "chunk.with_view(game, |view| view.loaded()).unwrap_or(false)", 1),
            swap("            assert!(!`chunk.loaded(game)`);",
                 "chunk.with_view(game, |view| view.loaded()).unwrap_or(false)", 2),
            swap("            assert_eq!(`chunk.status(game)`, Some(ChunkStatus::Full));",
                 "chunk.with_view(game, |view| view.status()).flatten()", 2),
            swap("            assert_eq!(`chunk.status(game)`, None);",
                 "chunk.with_view(game, |view| view.status()).flatten()", 2),
        ),
        MODDING / "native_level_refs.rs": (
            swap("            assert!(!`game.chunk_loaded(far)`);",
                 "far.with_view(game, |view| view.loaded()).unwrap_or(false)", 1),
            swap("            assert!(!`game.chunk_loaded(pending)`);",
                 "pending.with_view(game, |view| view.loaded()).unwrap_or(false)", 3),
            swap("            assert!(!`game.chunk_loaded(queued)`);",
                 "queued.with_view(game, |view| view.loaded()).unwrap_or(false)", 1),
            swap("            assert!(!`game.chunk_loaded(queued)`, \"paused streaming does no work\");",
                 "queued.with_view(game, |view| view.loaded()).unwrap_or(false)", 1),
            swap("            assert!(!`game.chunk_loaded(chunk)`);",
                 "chunk.with_view(game, |view| view.loaded()).unwrap_or(false)", 1),
            swap("                `game.chunk_loaded(chunk)`,",
                 "chunk.with_view(game, |view| view.loaded()).unwrap_or(false)", 1),
        ),
    },
}

# An exception row is an exact, complete line. Only declarations this cut
# removes belong here (menu_merchant_offers, for example, has no exception).
RETIRED = {
    "menu": (
        "menus::menu_carried",
        "menus::menu_container_id",
        "menus::menu_kind",
        "menus::menu_slot",
        "menus::menu_slot_backing",
        "menus::menu_slot_boxes",
        "menus::menu_slot_count",
        "menus::menu_slot_gives",
        "menus::menu_slot_holds",
        "menus::menu_slot_kinds",
    ),
    "player": (
        "player_experience::player_experience",
        "player_experience::player_xp_needed_for_next_level",
        "player_food::player_can_eat",
        "player_food::player_food",
        "player_food::player_has_enough_food",
        "player_food::player_needs_food",
        "player_modes::player_abilities",
        "player_modes::player_game_mode",
        "player_modes::player_is_creative",
        "player_modes::player_is_spectator",
        "player_modes::player_may_build",
    ),
    "entity": (
        "entity_identity::entity_kind",
        "entity_position::entity_block_position",
        "entity_position::entity_position",
        "entity_position::entity_rotation",
    ),
    "block-entity": (
        "block_entities::block_entity_data_slots",
        "block_entities::block_entity_kind",
        "block_entities::block_entity_level",
        "block_entities::block_entity_position",
    ),
    "chunk": (
        "level_chunks::chunk_is_slime_chunk",
        "level_chunks::chunk_loaded",
        "level_chunks::chunk_sections_with_block",
        "level_chunks::chunk_status",
    ),
}


def plan(path: Path, edits: tuple[Edit, ...]) -> tuple[str, int]:
    source = (ROOT / path).read_bytes().decode("utf-8")
    spans = []
    for edit in edits:
        # Match complete lines, even for a multi-line hunk. Lookahead also
        # finds overlapping duplicates; do not accept an arbitrary first hit.
        old = edit.old + "\n"
        matches = list(re.finditer(rf"(?=^{re.escape(old)})", source, re.MULTILINE))
        if len(matches) != edit.count:
            raise SystemExit(
                f"{path}: expected {edit.count} matches, found {len(matches)} for {edit.old!r}"
            )
        replacement = edit.new + "\n" if edit.new else ""
        spans.extend((m.start(), m.start() + len(old), replacement) for m in matches)

    parts = []
    cursor = 0
    for start, end, replacement in sorted(spans):
        if start < cursor:
            line = source.count("\n", 0, start) + 1
            raise SystemExit(f"{path}:{line}: overlapping migration hunks")
        parts.extend((source[cursor:start], replacement))
        cursor = end
    parts.append(source[cursor:])
    return "".join(parts), len(spans)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("family", choices=EDITS, help="cut over only this handle family")
    parser.add_argument("--dry-run", action="store_true", help="check and list hunks without writing")
    args = parser.parse_args()
    edits = EDITS[args.family] | {
        UNWINDING_METHODS: tuple(Edit(row, "", 1) for row in RETIRED[args.family])
    }
    # Preflight the whole selector against the unchanged inputs before writing any file.
    updates = {path: plan(path, edits[path]) for path in sorted(edits)}
    for path, (source, count) in updates.items():
        if not args.dry_run:
            (ROOT / path).write_bytes(source.encode("utf-8"))
        print(f"{path}: {count}")


if __name__ == "__main__":
    main()
