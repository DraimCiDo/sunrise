//! stable-line: content-declarations
//! The carried structure processor that keeps selected world blocks.

use super::KeepWorldBlock;
use crate::worldgen::DecodeError;
use serde_json::Value;

impl KeepWorldBlock {
    pub(in super::super) fn decode(value: &Value) -> Result<Self, DecodeError> {
        let blocks = value["blocks"]
            .as_array()
            .ok_or(DecodeError::Field("keep_world_block blocks"))?
            .iter()
            .map(|b| {
                let b = b
                    .as_str()
                    .ok_or(DecodeError::Field("keep_world_block blocks"))?;
                Ok(crate::worldgen::tags::identifier(b)?.to_string())
            })
            .collect::<Result<_, DecodeError>>()?;
        Ok(Self { blocks })
    }
}
