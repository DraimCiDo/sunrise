#!/usr/bin/env python3
"""Apply the transaction-scope cutover's mechanical journal observations."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
GAME = Path("crates/sunrise-client/src/game")
VOCAB = Path("crates/sunrise-mod-vocab/src")


def replace(path, pattern, replacement, expected):
    path = ROOT / path
    source, count = re.subn(pattern, replacement, path.read_text())
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} changes for {pattern}, found {count}")
    path.write_text(source)


def main():
    for path, count in (
        ("modding/grindstone_decisions_tests.rs", 2),
        ("modding/mod_menu_decisions_tests.rs", 2),
        ("modding/slot_take_tests.rs", 1),
        ("network/lan_trade_decisions_tests.rs", 1),
    ):
        replace(
            GAME / path,
            r"host\s*\.\s*transaction_current\(\)\s*\.\s*is_some\(\)",
            "!host.transaction_matches(None)",
            count,
        )

    for path, count in (
        (GAME / "blocks/machines/contents_changed_tests.rs", 1),
        (VOCAB / "item/storage.rs", 2),
    ):
        replace(
            path,
            r"assert_eq!\(host\s*\.\s*transaction_current\(\)\s*\.\s*as_ref\(\), Some\((\w+)\)\);",
            r"assert!(host.transaction_matches(Some(\1)));",
            count,
        )
    replace(
        GAME / "blocks/machines/contents_changed_tests.rs",
        r"assert_eq!\(host\.transaction_current\(\), None\);",
        "assert!(host.transaction_matches(None));",
        1,
    )
    for path in (
        "blocks/machines/contents_changed_tests.rs",
        "blocks/machines/storage_attempt_tests.rs",
    ):
        replace(
            GAME / path,
            r"assert_eq!\(host\.transaction_depth\((\w+)\), Some\((\d+)\)\);",
            r"assert_eq!(\1.depth(), \2);",
            2,
        )
    replace(
        VOCAB / "content/resource.rs",
        r"host\.transaction_current\(\).as_ref\(\) != Some\(tx\)",
        "!host.transaction_matches(Some(tx))",
        1,
    )
    replace(
        VOCAB / "content/resource_util.rs",
        r"host\.transaction_current\(\).as_ref\(\) == tx",
        "host.transaction_matches(tx)",
        1,
    )


if __name__ == "__main__":
    main()
