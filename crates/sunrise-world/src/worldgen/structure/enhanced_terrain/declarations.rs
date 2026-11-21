//! stable-line: content-declarations
//! Carried enhanced terrain adaptation declarations on structures and pieces.

use super::{Adaptation, Band, StructureAdaptation};
use crate::worldgen::DecodeError;
use serde_json::Value;
use std::sync::Arc;
use sunrise_core::density::TerrainKernel;

impl Adaptation {
    pub fn decode(v: &Value) -> Result<Self, DecodeError> {
        let int = |v: &Value, key: &'static str| -> Result<i32, DecodeError> {
            v[key]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or(DecodeError::Field(key))
        };
        let optional = |v: &Value, key: &str| v[key].as_i64().map_or(0, |n| n as i32);
        let action = |key: &'static str| match v[key].as_str() {
            Some("bury") => Ok(1),
            Some("carve") => Ok(-1),
            Some("none") => Ok(0),
            _ => Err(DecodeError::Field(key)),
        };
        let size = int(v, "kernel_size")?;
        let distance = int(v, "kernel_distance")?;
        if size < 0 || distance < 0 {
            return Err(DecodeError::Field("kernel_size"));
        }
        let padding = &v["padding"];
        let band = match v.get("band") {
            Some(b) => Some(Band {
                bottom: int(b, "bottom")?,
                top: int(b, "top")?,
                heights: b["piece_heights"].as_array().map(|h| {
                    h.iter()
                        .filter_map(Value::as_i64)
                        .map(|n| n as i32)
                        .collect()
                }),
            }),
            None => None,
        };
        Ok(Self {
            kernel: Arc::new(TerrainKernel::new(
                size,
                distance,
                action("top")?,
                action("bottom")?,
            )),
            offset: v["bottom_offset"].as_f64().unwrap_or(0.) as i32,
            padding: ["x", "top", "bottom", "z"].map(|k| optional(padding, k)),
            band,
        })
    }
}

impl StructureAdaptation {
    pub fn decode(v: &Value) -> Result<Self, DecodeError> {
        Ok(Self {
            adaptation: Arc::new(Adaptation::decode(v)?),
            rigid_junctions: v["rigid_junctions"].as_bool().unwrap_or(false),
            inflate: v["inflate"].as_bool().unwrap_or(false),
        })
    }
}
