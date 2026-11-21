//! stable-line: content-declarations
//! The carried `sunrise:matter_phase` replacement predicate.

use super::{MatterPhase, Phase};
use crate::worldgen::{DecodeError, Object, block_feature::FeatureBlockBindings, required};
use std::collections::BTreeSet;

impl MatterPhase {
    pub(super) fn decode(
        object: &Object,
        bindings: &FeatureBlockBindings<'_>,
    ) -> Result<Self, DecodeError> {
        let name: String = serde_json::from_str(required(object, "phase_to_test_for")?.get())?;
        let phase = match name.as_str() {
            "SOLID" => Phase::Solid,
            "LIQUID" => Phase::Liquid,
            "AIR" => Phase::Air,
            "AIR_RAIL_OR_CHAIN" => Phase::AirRailOrChain,
            "LIQUID_RAIL_OR_CHAIN" => Phase::LiquidRailOrChain,
            _ => return Err(DecodeError::Field("matter phase")),
        };
        let invert = match object.get("invert_condition") {
            Some(v) => serde_json::from_str(v.get())?,
            None => false,
        };
        let can_occlude = match phase {
            Phase::Solid => Some(
                bindings
                    .can_occlude
                    .clone()
                    .ok_or(DecodeError::Field("matter phase block properties"))?,
            ),
            _ => None,
        };
        let mut rail_or_chain = BTreeSet::new();
        for name in ["minecraft:rail", "minecraft:iron_chain"] {
            rail_or_chain.extend(bindings.blocks.get(name).into_iter().flat_map(|s| s.iter()));
        }
        Ok(Self {
            phase,
            invert,
            air: bindings.air,
            fluids: bindings.fluids.table().clone(),
            can_occlude,
            rail_or_chain,
        })
    }
}
