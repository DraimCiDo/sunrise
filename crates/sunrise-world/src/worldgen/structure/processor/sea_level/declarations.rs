impl KeepWorldBlock {
    pub(super) fn decode(value: &Value) -> Result<Self, DecodeError> {
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
