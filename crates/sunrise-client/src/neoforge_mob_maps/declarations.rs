impl Kind {
    pub const fn field(self) -> &'static str {
        match self {
            Self::VillagerDistances => "acceptable_villager_distance",
            Self::ParrotImitations => "sound",
            Self::VillagerTypes => "villager_type",
            Self::RaidHeroGifts => "loot_table",
            Self::VibrationFrequencies => "frequency",
            Self::VillagerCompostables => "can_villager_compost",
        }
    }
}

pub const MAPS: [BuiltinMap; 6] = [
    BuiltinMap {
        id: "neoforge:acceptable_villager_distances",
        directory: "entity_type",
        registry: "minecraft:entity_type",
        kind: Kind::VillagerDistances,
    },
    BuiltinMap {
        id: "neoforge:parrot_imitations",
        directory: "entity_type",
        registry: "minecraft:entity_type",
        kind: Kind::ParrotImitations,
    },
    BuiltinMap {
        id: "neoforge:villager_types",
        directory: "worldgen/biome",
        registry: "minecraft:worldgen/biome",
        kind: Kind::VillagerTypes,
    },
    BuiltinMap {
        id: "neoforge:raid_hero_gifts",
        directory: "villager_profession",
        registry: "minecraft:villager_profession",
        kind: Kind::RaidHeroGifts,
    },
    BuiltinMap {
        id: "neoforge:vibration_frequencies",
        directory: "game_event",
        registry: "minecraft:game_event",
        kind: Kind::VibrationFrequencies,
    },
    BuiltinMap {
        id: "neoforge:villager_compostables",
        directory: "item",
        registry: "minecraft:item",
        kind: Kind::VillagerCompostables,
    },
];

fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap_or(value)
}

fn resource(value: &Value, name: &str) -> Result<String, String> {
    let raw = field(value, name)
        .as_str()
        .ok_or_else(|| format!("{name} must be an identifier"))?;
    sunrise_core::identifier::Identifier::parse(raw)
        .map(|id| id.to_string())
        .map_err(|e| format!("{name}: {e}"))
}

fn number(value: &serde_json::value::RawValue) -> Result<&str, String> {
    let text = value.get().trim();
    // RawValue already guarantees JSON syntax; only its value kind is unknown.
    if matches!(text.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        Ok(text)
    } else {
        Err("value must be numeric".into())
    }
}

/// Parse the original decimal straight to f32, without an f64 intermediate.
pub fn decode_distance(value: &serde_json::value::RawValue) -> Result<f32, String> {
    number(value)?
        .parse()
        .map_err(|error| format!("invalid distance: {error}"))
}

/// Registry lookup is part of the sound codec, before file installation.
pub fn decode_sound(value: &Value, known: impl Fn(&str) -> bool) -> Result<&'static str, String> {
    let id = resource(value, "sound")?;
    if !known(&id) {
        return Err(format!("unknown sound {id}"));
    }
    Ok(if id == EMPTY_SOUND {
        ""
    } else {
        crate::mobs::entity_sounds::intern_full_id(local_id(&id))
    })
}

pub fn decode_villager_type(value: &Value) -> Result<u8, String> {
    let id = resource(value, "villager_type")?;
    VILLAGER_TYPES
        .iter()
        .position(|name| id == full_id(name))
        .map(|i| i as u8)
        .ok_or_else(|| format!("unknown villager type {id}"))
}

pub fn decode_gift(value: &Value) -> Result<String, String> {
    resource(value, "loot_table")
}

/// Gson's exact decimal intValue: truncate, retain the low 32 bits, then
/// apply the codec's range. Work is bounded by input length, not its exponent.
pub fn decode_frequency(value: &serde_json::value::RawValue) -> Result<i32, String> {
    let text = number(value)?;
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let exponent_negative = exponent.starts_with('-');
    let exponent = exponent
        .trim_start_matches(['+', '-'])
        .bytes()
        .fold(0i128, |n, digit| {
            n.saturating_mul(10)
                .saturating_add(i128::from(digit - b'0'))
        });
    let exponent = if exponent_negative {
        exponent.saturating_neg()
    } else {
        exponent
    };
    let before = mantissa.find('.').unwrap_or(mantissa.len());
    let count = mantissa.bytes().filter(|byte| *byte != b'.').count() as i128;
    let point = (before as i128).saturating_add(exponent);
    let mut bits = 0u32;
    if point > 0 && point < count + 32 {
        for digit in mantissa
            .bytes()
            .filter(|byte| *byte != b'.')
            .take(point.min(count) as usize)
        {
            bits = bits.wrapping_mul(10).wrapping_add(u32::from(digit - b'0'));
        }
        for _ in 0..(point - count).max(0) {
            bits = bits.wrapping_mul(10);
        }
    }
    if negative {
        bits = bits.wrapping_neg();
    }
    let frequency = bits as i32;
    if (1..=15).contains(&frequency) {
        Ok(frequency)
    } else {
        Err("frequency must truncate and wrap to 1..15".into())
    }
}

pub fn decode_compostable(value: &Value) -> Result<bool, String> {
    field(value, "can_villager_compost")
        .as_bool()
        .ok_or_else(|| "can_villager_compost must be boolean".into())
}
