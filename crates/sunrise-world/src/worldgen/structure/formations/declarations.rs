impl Placement {
    pub fn decode(name: &str) -> Result<Self, DecodeError> {
        Ok(match name {
            "surface" => Self::Surface,
            "ceiling" => Self::Ceiling,
            "on_water" => Self::OnWater,
            "on_lava" => Self::OnLava,
            "underground" => Self::Underground,
            "underground_surface" => Self::UndergroundSurface,
            "underground_ceiling" => Self::UndergroundCeiling,
            "underground_buried" => Self::UndergroundBuried,
            "underground_on_lava" => Self::UndergroundOnLava,
            _ => return Err(DecodeError::Field("formations placement")),
        })
    }
}

impl Def {
    pub fn decode(v: &Value) -> Result<Self, DecodeError> {
        Ok(Self {
            // An inline element is never written by Formations' generators.
            pool: v["template"]
                .as_str()
                .ok_or(DecodeError::Field("formations template"))?
                .to_owned(),
            placement: Placement::decode(v["placement"].as_str().unwrap_or_default())?,
            pieced: match v.get("sunrise:pieced") {
                None => None,
                Some(p) => {
                    let int = |k: &'static str| {
                        p[k].as_i64().map(|n| n as i32).ok_or(DecodeError::Field(k))
                    };
                    Some((int("max_depth")?, int("max_distance_from_center")?))
                }
            },
        })
    }
}
