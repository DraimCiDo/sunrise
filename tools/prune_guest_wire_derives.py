#!/usr/bin/env python3
"""Reproduce the derive removals for local records in the native-only mod API."""

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VOCAB = Path("crates/sunrise-mod-vocab/src")
RECORDS = {
    VOCAB / "client/arm_pose.rs": ("ArmAngles",),
    VOCAB / "client/types.rs": ("ClientPacket",),
    VOCAB / "content/packet_builders.rs": ("PacketBuildError",),
    VOCAB / "content/packets.rs": (
        "ServerboundPlayPacket", "SendPacketError", "HoldId", "HoldError", "DisconnectError",
    ),
    VOCAB / "game.rs": ("NbtUpgradeError", "Config", "ConfigFile", "DataMapType", "ModMismatch"),
    Path("crates/sunrise-mod-package/src/content.rs"): ("FluidFile", "WorldFluid", "FluidLook"),
}

for file, names in RECORDS.items():
    path = ROOT / file
    source = path.read_text()
    for name in names:
        record = re.compile(
            rf"(?m)^((?:#\[[^\n]*\]\n|//[^\n]*\n)+)((?:pub )?(?:enum|struct) {name}\b)"
        )
        match, = record.finditer(source)
        attrs = match[1].replace(", sunrise_mod_api_macros::Wire", "")
        attrs = attrs.replace("#[derive(sunrise_mod_api_macros::Wire)]\n", "")
        attrs = attrs.replace("#[wire(crate = ::mod_vocab::wire)]\n", "")
        source = source[:match.start(1)] + attrs + source[match.end(1):]
    path.write_text(source)
