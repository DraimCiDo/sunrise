//! Persistent component codecs, without protocol state or registry-number assumptions.
use super::{Patch, Template, Value};
use sunrise_core::nbt::Tag;

type Result<T, E = String> = std::result::Result<T, E>;
const TEXT_BOOLS: &[&str] = &[
    "bold",
    "italic",
    "underlined",
    "strikethrough",
    "obfuscated",
    "interpret",
    "plain",
];

/// `Identifier.CODEC`: default the namespace, never substitute a registry entry.
pub fn identifier(s: &str) -> Result<String> {
    let s = if s.contains(':') {
        s.to_owned()
    } else {
        format!("minecraft:{s}")
    };
    let Some((namespace, path)) = s.split_once(':') else {
        unreachable!()
    };
    if namespace.is_empty()
        || path.is_empty()
        || !namespace
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
        || !path
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"/_.-".contains(&c))
    {
        return Err(format!("invalid identifier {s}"));
    }
    Ok(s)
}
fn string(t: &Tag) -> Result<String> {
    if let Tag::String(s) = t {
        Ok(s.to_string_lossy())
    } else {
        Err("expected string".into())
    }
}
fn ident(t: &Tag) -> Result<Value> {
    Ok(Value::string(&identifier(&string(t)?)?))
}
fn int(t: &Tag) -> Result<Value> {
    Ok(Value::Int(match t {
        Tag::Long(v) => *v as i32,
        _ => number(t)? as i32,
    }))
}
fn number(t: &Tag) -> Result<f64> {
    Ok(match t {
        Tag::Byte(v) => *v as f64,
        Tag::Short(v) => *v as f64,
        Tag::Int(v) => *v as f64,
        Tag::Long(v) => *v as f64,
        Tag::Float(v) => f32::from_bits(*v) as f64,
        Tag::Double(v) => f64::from_bits(*v),
        _ => return Err("expected number".into()),
    })
}
fn float(t: &Tag) -> Result<Value> {
    Ok(if let Tag::Float(v) = t {
        Value::Float(*v)
    } else {
        Value::Float((number(t)? as f32).to_bits())
    })
}
fn double(t: &Tag) -> Result<Value> {
    Ok(if let Tag::Double(v) = t {
        Value::Double(*v)
    } else {
        Value::Double(number(t)?.to_bits())
    })
}
fn boolean(t: &Tag) -> Result<Value> {
    Ok(Value::Bool(number(t)? != 0.0))
}
fn text(t: &Tag) -> Result<Value> {
    Value::from_nbt(super::compact_text_tag(t), TEXT_BOOLS)
}
fn list(t: &Tag, f: impl Fn(&Tag) -> Result<Value>) -> Result<Value> {
    let values = match t {
        Tag::List(v) => v.iter().map(f).collect::<Result<_>>()?,
        Tag::ByteArray(v) => v
            .iter()
            .map(|v| f(&Tag::Byte(*v as i8)))
            .collect::<Result<_>>()?,
        Tag::IntArray(v) => v.iter().map(|v| f(&Tag::Int(*v))).collect::<Result<_>>()?,
        Tag::LongArray(v) => v.iter().map(|v| f(&Tag::Long(*v))).collect::<Result<_>>()?,
        _ => return Err("expected list".into()),
    };
    Ok(Value::List(values))
}
fn integer_list(t: &Tag) -> Result<Value> {
    if let Tag::IntArray(v) = t {
        return Ok(Value::List(v.iter().copied().map(Value::Int).collect()));
    }
    list(t, int)
}
fn map(t: &Tag, f: impl Fn(&Tag) -> Result<Value>) -> Result<Value> {
    let Tag::Compound(v) = t else {
        return Err("expected map".into());
    };
    Ok(Value::Map(
        v.iter()
            .map(|(k, v)| Ok((Value::String(k.0.clone()), f(v)?)))
            .collect::<Result<_>>()?,
    ))
}
fn holder_set(t: &Tag) -> Result<Value> {
    match t {
        Tag::String(_) => {
            let s = string(t)?;
            Ok(Value::string(&if let Some(s) = s.strip_prefix('#') {
                format!("#{}", identifier(s)?)
            } else {
                identifier(&s)?
            }))
        }
        Tag::List(v) if v.len() == 1 => ident(&v[0]),
        Tag::List(_) => list(t, ident),
        _ => Err("expected registry holder set".into()),
    }
}
fn sound(t: &Tag) -> Result<Value> {
    if matches!(t, Tag::String(_)) {
        return ident(t);
    }
    record(t, |r| {
        r.required("sound_id", ident)?;
        r.optional("range", float)?;
        Ok(())
    })
}
fn holder(t: &Tag, direct: impl Fn(&Tag) -> Result<Value>) -> Result<Value> {
    if matches!(t, Tag::String(_)) {
        ident(t)
    } else {
        direct(t)
    }
}
struct Record<'a> {
    tag: &'a Tag,
    fields: Vec<(Value, Value)>,
}
impl Record<'_> {
    fn required(&mut self, name: &str, f: impl Fn(&Tag) -> Result<Value>) -> Result<()> {
        let v = self
            .tag
            .get(name)
            .ok_or_else(|| format!("missing {name}"))?;
        self.fields.push((Value::string(name), f(v)?));
        Ok(())
    }
    fn optional(&mut self, name: &str, f: impl Fn(&Tag) -> Result<Value>) -> Result<()> {
        if let Some(v) = self.tag.get(name) {
            self.fields.push((Value::string(name), f(v)?));
        }
        Ok(())
    }
    fn default(
        &mut self,
        name: &str,
        default: Value,
        f: impl Fn(&Tag) -> Result<Value>,
    ) -> Result<()> {
        if let Some(v) = self.tag.get(name) {
            let v = f(v)?;
            if v != default {
                self.fields.push((Value::string(name), v));
            }
        }
        Ok(())
    }
    fn i(&mut self, name: &str, default: i32) -> Result<()> {
        self.default(name, Value::Int(default), int)
    }
    fn f(&mut self, name: &str, default: f32) -> Result<()> {
        self.default(name, Value::Float(default.to_bits()), float)
    }
    fn b(&mut self, name: &str, default: bool) -> Result<()> {
        self.default(name, Value::Bool(default), boolean)
    }
    fn id(&mut self, name: &str, default: &str, f: impl Fn(&Tag) -> Result<Value>) -> Result<()> {
        self.default(name, Value::string(default), f)
    }
    fn list(&mut self, name: &str, f: impl Fn(&Tag) -> Result<Value>) -> Result<()> {
        self.default(name, Value::List(vec![]), |v| list(v, &f))
    }
}
fn record(t: &Tag, f: impl FnOnce(&mut Record<'_>) -> Result<()>) -> Result<Value> {
    if !matches!(t, Tag::Compound(_)) {
        return Err("expected record".into());
    }
    let mut r = Record {
        tag: t,
        fields: vec![],
    };
    f(&mut r)?;
    Ok(Value::Map(r.fields))
}
fn filterable(t: &Tag, f: impl Fn(&Tag) -> Result<Value>) -> Result<Value> {
    if t.get("raw").is_some() {
        record(t, |r| {
            r.required("raw", &f)?;
            r.optional("filtered", &f)
        })
    } else {
        Ok(Value::Map(vec![(Value::string("raw"), f(t)?)]))
    }
}
fn string_value(t: &Tag) -> Result<Value> {
    let Tag::String(s) = t else {
        return Err("expected string".into());
    };
    Ok(Value::String(s.0.clone()))
}
fn unit(_: &Tag) -> Result<Value> {
    Ok(Value::Map(vec![]))
}
fn effect_details(t: &Tag, with_id: bool) -> Result<Value> {
    record(t, |r| {
        if with_id {
            r.required("id", ident)?;
        }
        r.default("amplifier", Value::Byte(0), |v| {
            let Value::Int(v) = int(v)? else {
                unreachable!()
            };
            Ok(Value::Byte(v as i8))
        })?;
        r.i("duration", 0)?;
        r.b("ambient", false)?;
        r.b("show_particles", true)?;
        let particles = t
            .get("show_particles")
            .map(boolean)
            .transpose()?
            .unwrap_or(Value::Bool(true));
        let icon = t
            .get("show_icon")
            .map(boolean)
            .transpose()?
            .unwrap_or(particles);
        r.fields.push((Value::string("show_icon"), icon));
        r.optional("hidden_effect", |v| effect_details(v, false))
    })
}
fn consume_effect(t: &Tag) -> Result<Value> {
    let kind = identifier(&string(
        t.get("type").ok_or("missing consume effect type")?,
    )?)?;
    validate_source(kind.strip_prefix("minecraft:").unwrap_or(&kind), t)?;
    record(t, |r| {
        r.fields.push((Value::string("type"), Value::string(&kind)));
        match kind.as_str() {
            "minecraft:apply_effects" => {
                r.required("effects", |v| list(v, |v| effect_details(v, true)))?;
                r.f("probability", 1.0)?;
            }
            "minecraft:remove_effects" => r.required("effects", holder_set)?,
            "minecraft:clear_all_effects" => (),
            "minecraft:teleport_randomly" => {
                r.f("diameter", 16.0)?;
                r.b("directional_particles", true)?;
            }
            "minecraft:play_sound" => r.required("sound", sound)?,
            _ => return Err(format!("unknown consume effect {kind}")),
        }
        Ok(())
    })
}
fn explosion(t: &Tag) -> Result<Value> {
    validate_source("firework_explosion", t)?;
    record(t, |r| {
        r.required("shape", string_value)?;
        r.default("colors", Value::List(vec![]), integer_list)?;
        r.default("fade_colors", Value::List(vec![]), integer_list)?;
        r.b("has_trail", false)?;
        r.b("has_twinkle", false)
    })
}
fn trim_material(t: &Tag) -> Result<Value> {
    record(t, |r| {
        r.required("palette_id", ident)?;
        r.required("description", text)
    })
}
fn trim_pattern(t: &Tag) -> Result<Value> {
    record(t, |r| {
        r.required("asset_id", ident)?;
        r.required("description", text)?;
        if t.get("decal").is_some() {
            r.required("decal", boolean)
        } else {
            r.fields.push((Value::string("decal"), Value::Bool(false)));
            Ok(())
        }
    })
}
fn instrument(t: &Tag) -> Result<Value> {
    validate_source("instrument", t)?;
    record(t, |r| {
        r.required("sound_event", sound)?;
        r.required("use_duration", float)?;
        r.required("range", float)?;
        r.i("durability_damage", 0)?;
        r.required("description", text)
    })
}
fn banner(t: &Tag) -> Result<Value> {
    record(t, |r| {
        r.required("asset_id", ident)?;
        r.required("translation_key", string_value)
    })
}
fn kinetic(t: &Tag) -> Result<Value> {
    record(t, |r| {
        r.required("max_duration_ticks", int)?;
        r.f("min_speed", 0.0)?;
        r.f("min_relative_speed", 0.0)
    })
}
fn resolvable(t: &Tag, f: impl Fn(&Tag) -> Result<Value>) -> Result<Value> {
    if matches!(t, Tag::String(_)) {
        ident(t)
    } else {
        f(t)
    }
}

/// The one bound on every source tree, checked before any recursive conversion of it.
fn check_depth(t: &Tag) -> Result<()> {
    if !sunrise_core::nbt::within_depth(t, super::MAX_TREE_DEPTH) {
        return Err(format!("NBT nesting exceeds {}", super::MAX_TREE_DEPTH));
    }
    Ok(())
}
/// `ItemStackTemplate.CODEC`: source admission, not the signed native stream codec.
pub fn template_from_tag(t: &Tag) -> Result<Template> {
    check_depth(t)?;
    template_tag(t, false)
}
fn template_tag(t: &Tag, native_templates: bool) -> Result<Template> {
    if matches!(t, Tag::String(_)) {
        let item = identifier(&string(t)?)?;
        if item == "minecraft:air" {
            return Err("air is not a template item".into());
        }
        return Ok(Template {
            item,
            count: 1,
            patch: vec![],
        });
    }
    let item = identifier(&string(t.get("id").ok_or("missing template item")?)?)?;
    if item == "minecraft:air" {
        return Err("air is not a template item".into());
    }
    let count = t
        .get("count")
        .map(int)
        .transpose()?
        .unwrap_or(Value::Int(1));
    let Value::Int(count) = count else {
        unreachable!()
    };
    if native_templates {
        if count == 0 {
            return Err("native template count must be nonzero".into());
        }
    } else if !(1..=99).contains(&count) {
        return Err("template count outside 1..99".into());
    }
    let mut patch: Patch = vec![];
    if let Some(c) = t.get("components") {
        let Tag::Compound(c) = c else {
            return Err("template components are not a map".into());
        };
        for (name, value) in c {
            let name = name.to_string_lossy();
            let (remove, name) = name
                .strip_prefix('!')
                .map_or((false, name.as_str()), |n| (true, n));
            let name = identifier(name)?;
            if !super::is_persistent(&name) {
                return Err(format!("{name} has no persistent codec"));
            }
            let value = if remove {
                None
            } else {
                Some(component_tag(&name, value, native_templates)?)
            };
            if let Some(entry) = patch.iter_mut().find(|(n, _)| n == &name) {
                entry.1 = value;
            } else {
                patch.push((name, value));
            }
        }
    }
    Ok(Template { item, count, patch })
}
fn template(t: &Tag, native_templates: bool) -> Result<Value> {
    Ok(Value::Template(Box::new(template_tag(
        t,
        native_templates,
    )?)))
}
fn predicate(t: &Tag, native_templates: bool) -> Result<Value> {
    record(t, |r| {
        only_fields(t, &["blocks", "state", "nbt", "components", "predicates"])?;
        r.optional("blocks", holder_set)?;
        r.optional("state", |v| {
            map(v, |v| {
                if matches!(v, Tag::String(_)) {
                    string_value(v)
                } else {
                    record(v, |r| {
                        only_fields(v, &["min", "max"])?;
                        r.optional("min", string_value)?;
                        r.optional("max", string_value)
                    })
                }
            })
        })?;
        r.optional("nbt", |v| {
            if let Tag::String(_) = v {
                let s = string(v)?;
                let (tag, n) = sunrise_core::nbt::parse_snbt(&s)?;
                if n != s.len() {
                    return Err("trailing predicate SNBT".into());
                }
                Ok(Value::string(&crate::snbt_text::to_snbt(&tag)))
            } else {
                Ok(Value::string(&crate::snbt_text::to_snbt(v)))
            }
        })?;
        r.default("components", Value::Map(vec![]), |v| {
            exact_components(v, native_templates)
        })?;
        r.default("predicates", Value::Map(vec![]), |v| {
            partial_predicates(v, native_templates)
        })
    })
}

/// Decode the actual component codec's schema and remove its Java defaults.
/// Registry identifiers remain names; the caller validates them against its snapshot.
pub fn from_component_tag(id: &str, t: &Tag) -> Result<Value> {
    check_depth(t)?;
    component_tag(id, t, false)
}
/// Datafix output represents native stacks, whose template counts are not bounded by CODEC.
pub fn from_upgraded_component_tag(id: &str, t: &Tag) -> Result<Value> {
    check_depth(t)?;
    component_tag(id, t, true)
}
fn component_tag(id: &str, t: &Tag, native_templates: bool) -> Result<Value> {
    let id = identifier(id)?;
    if !native_templates && !super::is_persistent(&id) {
        return Err(format!("{id} has no persistent codec"));
    }
    let name = id.strip_prefix("minecraft:").unwrap_or(&id);
    validate_source(name, t)?;
    Ok(match name {
        "creative_slot_lock" => unit(t)?,
        "map_post_processing" | "additional_trade_cost" => int(t)?,
        "custom_data" | "bucket_entity_data" => {
            if !matches!(t, Tag::Compound(_)) {
                return Err("custom data is not a compound".into());
            }
            Value::from_nbt(t, &[])?
        }
        "intangible_projectile" => unit(t)?,
        "map_decorations" => map(t, |v| {
            record(v, |r| {
                r.required("type", ident)?;
                r.required("x", double)?;
                r.required("z", double)?;
                r.required("rotation", float)
            })
        })?,
        "debug_stick_state" => {
            let Tag::Compound(entries) = t else {
                return Err("debug stick state is not a map".into());
            };
            Value::Map(
                entries
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            Value::string(&identifier(&k.to_string_lossy())?),
                            string_value(v)?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            )
        }
        "recipes" => list(t, ident)?,
        "lock" => {
            let value = item_predicate(t, native_templates)?;
            let tag = if native_templates {
                value.presentation_tag()?
            } else {
                value.to_tag()?
            };
            Value::from_nbt(&tag, &[])?
        }
        "container_loot" => record(t, |r| {
            r.required("loot_table", ident)?;
            r.default("seed", Value::Long(0), |v| {
                Ok(Value::Long(if let Tag::Long(v) = v {
                    *v
                } else {
                    number(v)? as i64
                }))
            })
        })?,
        "max_stack_size"
        | "max_damage"
        | "damage"
        | "repair_cost"
        | "map_id"
        | "ominous_bottle_amplifier" => int(t)?,
        "dyed_color" => rgb(t)?,
        "minimum_attack_charge" | "potion_duration_scale" => float(t)?,
        "enchantment_glint_override" => boolean(t)?,
        "unbreakable" | "glider" | "waxed" => unit(t)?,
        "custom_name" | "item_name" => text(t)?,
        "lore" => list(t, text)?,
        "rarity"
        | "base_color"
        | "dye"
        | "wolf/collar"
        | "tropical_fish/base_color"
        | "tropical_fish/pattern_color"
        | "cat/collar"
        | "sheep/color"
        | "shulker/color"
        | "cushion/color"
        | "fox/variant"
        | "salmon/size"
        | "parrot/variant"
        | "tropical_fish/pattern"
        | "mooshroom/variant"
        | "rabbit/variant"
        | "horse/variant"
        | "llama/variant"
        | "axolotl/variant" => string_value(t)?,
        "item_model"
        | "damage_type"
        | "tooltip_style"
        | "note_block_sound"
        | "block_transformer"
        | "villager/variant"
        | "wolf/variant"
        | "wolf/sound_variant"
        | "pig/variant"
        | "pig/sound_variant"
        | "cow/variant"
        | "cow/sound_variant"
        | "chicken/variant"
        | "chicken/sound_variant"
        | "zombie_nautilus/variant"
        | "frog/variant"
        | "cat/variant"
        | "cat/sound_variant"
        | "provides_pottery_pattern"
        | "painting/variant"
        | "jukebox_playable" => ident(t)?,
        "enchantments" | "stored_enchantments" => {
            let Tag::Compound(entries) = t else {
                return Err("enchantments are not a map".into());
            };
            let mut out = vec![];
            for (k, v) in entries {
                let key = Value::string(&identifier(&k.to_string_lossy())?);
                let value = int(v)?;
                let Value::Int(level) = value else {
                    unreachable!()
                };
                if !(1..=255).contains(&level) {
                    return Err("enchantment level outside 1..255".into());
                }
                let value = Value::Int(level);
                if let Some((_, old)) = out.iter_mut().find(|(k, _)| *k == key) {
                    *old = value;
                } else {
                    out.push((key, value));
                }
            }
            Value::Map(out)
        }
        "use_effects" => record(t, |r| {
            r.b("can_sprint", false)?;
            r.b("interact_vibrations", true)?;
            r.f("speed_multiplier", 0.2)
        })?,
        "food" => record(t, |r| {
            r.required("nutrition", int)?;
            r.required("saturation", float)?;
            r.b("can_always_eat", false)
        })?,
        "consumable" => record(t, |r| {
            r.f("consume_seconds", 1.6)?;
            r.id("animation", "eat", string_value)?;
            r.id("sound", "minecraft:entity.generic.eat", sound)?;
            r.b("has_consume_particles", true)?;
            r.list("on_consume_effects", consume_effect)
        })?,
        "use_remainder" | "sulfur_cube_content" => template(t, native_templates)?,
        "use_cooldown" => record(t, |r| {
            r.required("seconds", float)?;
            r.optional("cooldown_group", ident)
        })?,
        "damage_resistant" => record(t, |r| r.required("types", holder_set))?,
        "repairable" => record(t, |r| r.required("items", holder_set))?,
        "enchantable" => record(t, |r| r.required("value", int))?,
        "villager_food" => record(t, |r| r.required("nutrition", int))?,
        "tool" => record(t, |r| {
            r.required("rules", |v| {
                list(v, |v| {
                    record(v, |r| {
                        r.required("blocks", holder_set)?;
                        r.optional("speed", float)?;
                        r.optional("correct_for_drops", boolean)
                    })
                })
            })?;
            r.f("default_mining_speed", 1.0)?;
            r.i("damage_per_block", 1)?;
            r.b("can_destroy_blocks_in_creative", true)
        })?,
        "weapon" => record(t, |r| {
            r.i("item_damage_per_attack", 1)?;
            r.f("disable_blocking_for_seconds", 0.0)
        })?,
        "attack_range" => record(t, |r| {
            for (k, v) in [
                ("min_reach", 0.0),
                ("max_reach", 3.0),
                ("min_creative_reach", 0.0),
                ("max_creative_reach", 5.0),
                ("hitbox_margin", 0.3),
                ("mob_factor", 1.0),
            ] {
                r.f(k, v)?;
            }
            Ok(())
        })?,
        "equippable" => record(t, |r| {
            r.required("slot", string_value)?;
            r.id("equip_sound", "minecraft:item.armor.equip_generic", sound)?;
            r.optional("asset_id", ident)?;
            r.optional("camera_overlay", ident)?;
            r.optional("allowed_entities", holder_set)?;
            for (k, v) in [
                ("dispensable", true),
                ("swappable", true),
                ("damage_on_hurt", true),
                ("equip_on_interact", false),
                ("can_be_sheared", false),
            ] {
                r.b(k, v)?;
            }
            r.id("shearing_sound", "minecraft:item.shears.snip", sound)
        })?,
        "death_protection" => record(t, |r| r.list("death_effects", consume_effect))?,
        "piercing_weapon" => record(t, |r| {
            r.b("deals_knockback", true)?;
            r.b("dismounts", false)?;
            r.optional("sound", sound)?;
            r.optional("hit_sound", sound)
        })?,
        "kinetic_weapon" => record(t, |r| {
            r.i("contact_cooldown_ticks", 10)?;
            r.i("delay_ticks", 0)?;
            for k in [
                "dismount_conditions",
                "knockback_conditions",
                "damage_conditions",
            ] {
                r.optional(k, kinetic)?;
            }
            r.f("forward_movement", 0.0)?;
            r.f("damage_multiplier", 1.0)?;
            r.optional("sound", sound)?;
            r.optional("hit_sound", sound)
        })?,
        "attack_animation" | "interact_animation" => record(t, |r| {
            r.id("type", "whack", string_value)?;
            r.i("duration", 6)
        })?,
        "provides_banner_patterns" => holder_set(t)?,
        "break_sound" => sound(t)?,
        "compostable" => record(t, |r| r.required("layers", |v| resolvable(v, int)))?,
        "cooking_fuel" | "brewing_fuel" => record(t, |r| {
            r.required(
                if name == "cooking_fuel" {
                    "burn_time"
                } else {
                    "uses"
                },
                |v| resolvable(v, int),
            )?;
            r.required("speed_multiplier", |v| resolvable(v, float))
        })?,
        "mob_visibility" => record(t, |r| {
            r.required("targeting_entity_types", holder_set)?;
            r.required("visibility", float)
        })?,
        "can_place_on" | "can_break" => {
            if let Tag::List(v) = t {
                match v.len() {
                    0 => return Err("empty adventure predicate".into()),
                    1 => predicate(&v[0], native_templates)?,
                    _ => list(t, |v| predicate(v, native_templates))?,
                }
            } else {
                predicate(t, native_templates)?
            }
        }
        "attribute_modifiers" => list(t, attribute_entry)?,
        "custom_model_data" => record(t, |r| {
            r.list("floats", float)?;
            r.list("flags", boolean)?;
            r.list("strings", string_value)?;
            r.list("colors", rgb)
        })?,
        "tooltip_display" => record(t, |r| {
            r.b("hide_tooltip", false)?;
            if let Some(v) = t.get("hidden_components") {
                let Value::List(v) = list(v, ident)? else {
                    unreachable!()
                };
                let mut set = vec![];
                for v in v {
                    if !set.contains(&v) {
                        set.push(v);
                    }
                }
                if !set.is_empty() {
                    r.fields
                        .push((Value::string("hidden_components"), Value::Set(set)));
                }
            }
            Ok(())
        })?,
        "charged_projectiles" | "bundle_contents" => list(t, |v| template(v, native_templates))?,
        "potion_contents" => {
            if matches!(t, Tag::String(_)) {
                Value::Map(vec![(Value::string("potion"), ident(t)?)])
            } else {
                record(t, |r| {
                    r.optional("potion", ident)?;
                    r.optional("custom_color", int)?;
                    r.list("custom_effects", |v| effect_details(v, true))?;
                    r.optional("custom_name", string_value)
                })?
            }
        }
        "suspicious_stew_effects" => list(t, |v| {
            record(v, |r| {
                r.required("id", ident)?;
                if let Some(duration) = v.get("duration").and_then(|v| int(v).ok())
                    && duration != Value::Int(160)
                {
                    r.fields.push((Value::string("duration"), duration));
                }
                Ok(())
            })
        })?,
        "writable_book_content" => record(t, |r| r.list("pages", |v| filterable(v, string_value)))?,
        "written_book_content" => record(t, |r| {
            r.required("title", |v| filterable(v, string_value))?;
            r.required("author", string_value)?;
            r.i("generation", 0)?;
            r.list("pages", |v| filterable(v, text))?;
            r.b("resolved", false)
        })?,
        "trim" => record(t, |r| {
            r.required("material", |v| holder(v, trim_material))?;
            r.required("pattern", |v| holder(v, trim_pattern))
        })?,
        "instrument" => holder(t, instrument)?,
        "provides_trim_material" => holder(t, trim_material)?,
        "entity_data" | "block_entity_data" => {
            let mut v = t.clone();
            let id = ident(t.get("id").ok_or("missing typed data id")?)?.to_tag()?;
            let Tag::Compound(entries) = &mut v else {
                return Err("typed data not a compound".into());
            };
            entries.insert("id".into(), id);
            Value::from_nbt(&v, &[])?
        }
        "lodestone_tracker" => record(t, |r| {
            r.optional("target", |v| {
                record(v, |r| {
                    r.required("dimension", ident)?;
                    r.required("pos", |v| {
                        let Value::List(v) = integer_list(v)? else {
                            unreachable!()
                        };
                        if v.len() != 3 {
                            return Err("position must have three coordinates".into());
                        }
                        Ok(Value::IntArray(
                            v.into_iter()
                                .map(|v| {
                                    if let Value::Int(v) = v {
                                        v
                                    } else {
                                        unreachable!()
                                    }
                                })
                                .collect(),
                        ))
                    })
                })
            })?;
            r.b("tracked", true)
        })?,
        "firework_explosion" => explosion(t)?,
        "fireworks" => record(t, |r| {
            r.default("flight_duration", Value::Byte(0), |v| {
                let Value::Int(v) = int(v)? else {
                    unreachable!()
                };
                Ok(Value::Byte(v as i8))
            })?;
            r.list("explosions", explosion)
        })?,
        "banner_patterns" => list(t, |v| {
            record(v, |r| {
                r.required("pattern", |v| holder(v, banner))?;
                r.required("color", string_value)
            })
        })?,
        "pot_decorations" => record(t, |r| {
            for k in ["back", "left", "right", "front"] {
                r.optional(k, |v| template(v, native_templates))?;
            }
            Ok(())
        })?,
        "container" => {
            let Tag::List(entries) = t else {
                return Err("container is not a slot list".into());
            };
            if entries.len() > 256 {
                return Err("container source exceeds 256 entries".into());
            }
            let mut slots = vec![];
            for e in entries {
                let Value::Int(slot) = int(e.get("slot").ok_or("missing container slot")?)? else {
                    unreachable!()
                };
                if !(0..256).contains(&slot) {
                    return Err("container slot outside 0..255".into());
                }
                let item = template_tag(
                    e.get("item").ok_or("missing container item")?,
                    native_templates,
                )?;
                slots.resize_with(slots.len().max(slot as usize + 1), || None);
                slots[slot as usize] = Some(item);
            }
            Value::Container(slots)
        }
        "block_state" => map(t, string_value)?,
        "bees" => list(t, |v| {
            record(v, |r| {
                r.required("entity_data", |v| {
                    component_tag("minecraft:entity_data", v, native_templates)
                })?;
                r.required("ticks_in_hive", int)?;
                r.required("min_ticks_in_hive", int)
            })
        })?,
        "sign_text_front" | "sign_text_back" => sign_text(t)?,
        "profile" => profile(t)?,
        "blocks_attacks" => blocks_attacks(t)?,
        _ if !id.starts_with("minecraft:") => Value::from_nbt(t, &[])?,
        _ => return Err(format!("unknown component codec {id}")),
    })
}
fn profile(t: &Tag) -> Result<Value> {
    if matches!(t, Tag::String(_)) {
        return Ok(Value::Map(vec![(Value::string("name"), string_value(t)?)]));
    }
    record(t, |r| {
        r.optional("name", string_value)?;
        r.optional("id", |v| {
            let value = integer_list(v)?;
            let Value::List(parts) = value else {
                unreachable!()
            };
            if parts.len() != 4 {
                return Err("profile UUID must have four integers".into());
            }
            Ok(Value::IntArray(
                parts
                    .into_iter()
                    .map(|v| {
                        if let Value::Int(v) = v {
                            v
                        } else {
                            unreachable!()
                        }
                    })
                    .collect(),
            ))
        })?;
        if let Some(properties) = t.get("properties") {
            let properties = if let Tag::Compound(entries) = properties {
                let mut out = vec![];
                for (k, values) in entries {
                    let Tag::List(values) = values else {
                        return Err("profile property values are not a list".into());
                    };
                    for v in values {
                        out.push(Value::Map(vec![
                            (Value::string("name"), Value::String(k.0.clone())),
                            (Value::string("value"), string_value(v)?),
                        ]));
                    }
                }
                Value::List(out)
            } else {
                list(properties, |v| {
                    record(v, |r| {
                        r.required("name", string_value)?;
                        r.required("value", string_value)?;
                        r.optional("signature", string_value)
                    })
                })?
            };
            if !matches!(&properties,Value::List(v) if v.is_empty()) {
                r.fields.push((Value::string("properties"), properties));
            }
        }
        for key in ["texture", "cape", "elytra"] {
            r.optional(key, string_value)?;
        }
        r.optional("model", string_value)
    })
}
fn blocks_attacks(t: &Tag) -> Result<Value> {
    record(t, |r| {
        r.f("block_delay_seconds", 0.0)?;
        r.f("disable_cooldown_scale", 1.0)?;
        let default_reduction = Value::List(vec![Value::Map(vec![
            (Value::string("base"), Value::Float(0f32.to_bits())),
            (Value::string("factor"), Value::Float(1f32.to_bits())),
        ])]);
        r.default("damage_reductions", default_reduction, |v| {
            list(v, |v| {
                record(v, |r| {
                    r.f("horizontal_blocking_angle", 90.0)?;
                    r.optional("type", holder_set)?;
                    r.required("base", float)?;
                    r.required("factor", float)
                })
            })
        })?;
        let default_item = Value::Map(vec![
            (Value::string("threshold"), Value::Float(1f32.to_bits())),
            (Value::string("base"), Value::Float(0f32.to_bits())),
            (Value::string("factor"), Value::Float(1f32.to_bits())),
        ]);
        r.default("item_damage", default_item, |v| {
            record(v, |r| {
                r.required("threshold", float)?;
                r.required("base", float)?;
                r.required("factor", float)
            })
        })?;
        r.optional("bypassed_by", holder_set)?;
        r.optional("block_sound", sound)?;
        r.optional("disabled_sound", sound)
    })
}
fn only_fields(t: &Tag, allowed: &[&str]) -> Result<()> {
    let Tag::Compound(entries) = t else {
        return Err("expected predicate map".into());
    };
    for k in entries.keys() {
        if !allowed
            .iter()
            .any(|a| k.0.iter().copied().eq(a.encode_utf16()))
        {
            return Err(format!("unknown predicate field {}", k.to_string_lossy()));
        }
    }
    Ok(())
}
fn sign_messages(t: &Tag) -> Result<Value> {
    let value = list(t, text)?;
    if !matches!(&value,Value::List(v) if v.len()==4) {
        return Err("sign requires four messages".into());
    }
    Ok(value)
}
fn exact_components(t: &Tag, native_templates: bool) -> Result<Value> {
    let Tag::Compound(entries) = t else {
        return Err("expected component map".into());
    };
    let mut out = vec![];
    for (k, v) in entries {
        let name = identifier(&k.to_string_lossy())?;
        if !super::is_persistent(&name) {
            return Err(format!("{name} has no persistent codec"));
        }
        let value = component_tag(&name, v, native_templates)?;
        let key = Value::string(&name);
        if let Some((_, old)) = out.iter_mut().find(|(k, _)| *k == key) {
            *old = value;
        } else {
            out.push((key, value));
        }
    }
    Ok(Value::Map(out))
}
fn partial_predicates(t: &Tag, native_templates: bool) -> Result<Value> {
    let Tag::Compound(entries) = t else {
        return Err("expected partial predicate map".into());
    };
    let mut out = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        let name = identifier(&k.to_string_lossy())?;
        let value = if name == "minecraft:custom_data" {
            let tag = if matches!(v, Tag::String(_)) {
                let s = string(v)?;
                let (tag, n) = sunrise_core::nbt::parse_snbt(&s)?;
                if n != s.len() {
                    return Err("trailing predicate SNBT".into());
                }
                tag
            } else {
                v.clone()
            };
            if !matches!(tag, Tag::Compound(_)) {
                return Err("custom data predicate must be a compound".into());
            }
            Value::string(&crate::snbt_text::to_snbt(&tag))
        } else {
            predicate_value(v, native_templates)?
        };
        out.push((Value::string(&name), value));
    }
    Ok(Value::Map(out))
}
fn predicate_value(t: &Tag, native_templates: bool) -> Result<Value> {
    const BOOLS: &[&str] = &[
        "has_twinkle",
        "has_trail",
        "resolved",
        "ambient",
        "visible",
        "bold",
        "italic",
        "underlined",
        "strikethrough",
        "obfuscated",
        "interpret",
        "plain",
    ];
    match t {
        Tag::Compound(entries) => Ok(Value::Map(
            entries
                .iter()
                .map(|(k, v)| {
                    let name = k.to_string_lossy();
                    let value = if name == "components" {
                        exact_components(v, native_templates)?
                    } else if BOOLS.contains(&name.as_str()) {
                        boolean(v)?
                    } else {
                        predicate_value(v, native_templates)?
                    };
                    Ok((Value::String(k.0.clone()), value))
                })
                .collect::<Result<_>>()?,
        )),
        Tag::List(_) => list(t, |v| predicate_value(v, native_templates)),
        _ => Value::from_nbt(t, &[]),
    }
}
fn item_predicate(t: &Tag, native_templates: bool) -> Result<Value> {
    record(t, |r| {
        r.optional("items", holder_set)?;
        r.default("count", Value::Map(vec![]), |v| {
            if matches!(v, Tag::Compound(_)) {
                record(v, |r| {
                    r.optional("min", int)?;
                    r.optional("max", int)
                })
            } else {
                int(v)
            }
        })?;
        r.default("components", Value::Map(vec![]), |v| {
            exact_components(v, native_templates)
        })?;
        r.default("predicates", Value::Map(vec![]), |v| {
            partial_predicates(v, native_templates)
        })
    })
}
/// The persistent codec's range and size limits on one component's saved form.
pub fn validate_persistent(name: &str, t: &Tag) -> Result<()> {
    validate_source(name.strip_prefix("minecraft:").unwrap_or(name), t)
}
fn validate_source(name: &str, t: &Tag) -> Result<()> {
    fn integer(t: &Tag, min: i32, max: i32) -> Result<()> {
        let Value::Int(n) = int(t)? else {
            unreachable!()
        };
        if !(min..=max).contains(&n) {
            return Err(format!("integer outside {min}..{max}"));
        }
        Ok(())
    }
    fn real(t: &Tag, min: f32, max: f32, exclusive: bool) -> Result<()> {
        let Value::Float(n) = float(t)? else {
            unreachable!()
        };
        let n = f32::from_bits(n);
        // `Float.compareTo` order, as `ExtraCodecs.floatRange` uses: -0.0 is below 0.0 and NaN above every number.
        let below = n.total_cmp(&min);
        if below.is_lt() || exclusive && below.is_eq() || n.total_cmp(&max).is_gt() {
            return Err("float outside component codec range".into());
        }
        Ok(())
    }
    fn field(t: &Tag, k: &str, f: impl FnOnce(&Tag) -> Result<()>) -> Result<()> {
        if let Some(v) = t.get(k) {
            f(v)?;
        }
        Ok(())
    }
    fn enumeration(t: &Tag, values: &[&str]) -> Result<()> {
        let s = string(t)?;
        if !values.contains(&s.as_str()) {
            return Err(format!("unknown enum value {s}"));
        }
        Ok(())
    }
    fn limit(t: &Tag, max: usize) -> Result<()> {
        let n = match t {
            Tag::List(v) => v.len(),
            Tag::ByteArray(v) => v.len(),
            Tag::IntArray(v) => v.len(),
            Tag::LongArray(v) => v.len(),
            _ => return Ok(()),
        };
        if n > max {
            return Err(format!("component list exceeds {max}"));
        }
        Ok(())
    }
    fn string_limit(t: &Tag, max: usize) -> Result<()> {
        if let Tag::String(v) = t {
            if v.0.len() > max {
                return Err(format!("component string exceeds {max}"));
            }
        } else if let Tag::Compound(_) = t {
            for k in ["raw", "filtered"] {
                field(t, k, |v| string_limit(v, max))?;
            }
        }
        Ok(())
    }
    const DYE: &[&str] = &[
        "white",
        "orange",
        "magenta",
        "light_blue",
        "yellow",
        "lime",
        "pink",
        "gray",
        "light_gray",
        "cyan",
        "purple",
        "blue",
        "brown",
        "green",
        "red",
        "black",
    ];
    match name {
        "max_stack_size" => integer(t, 1, 99)?,
        "max_damage" => integer(t, 1, i32::MAX)?,
        "damage" | "repair_cost" => integer(t, 0, i32::MAX)?,
        "ominous_bottle_amplifier" => integer(t, 0, 4)?,
        "minimum_attack_charge" => real(t, 0.0, 1.0, false)?,
        "potion_duration_scale" => real(t, 0.0, f32::MAX, false)?,
        "rarity" => enumeration(t, &["common", "uncommon", "rare", "epic"])?,
        "base_color"
        | "dye"
        | "wolf/collar"
        | "tropical_fish/base_color"
        | "tropical_fish/pattern_color"
        | "cat/collar"
        | "sheep/color"
        | "shulker/color"
        | "cushion/color" => enumeration(t, DYE)?,
        "fox/variant" => enumeration(t, &["red", "snow"])?,
        "salmon/size" => enumeration(t, &["small", "medium", "large"])?,
        "parrot/variant" => enumeration(t, &["red_blue", "blue", "green", "yellow_blue", "gray"])?,
        "tropical_fish/pattern" => enumeration(
            t,
            &[
                "kob",
                "sunstreak",
                "snooper",
                "dasher",
                "brinely",
                "spotty",
                "flopper",
                "stripey",
                "glitter",
                "blockfish",
                "betty",
                "clayfish",
            ],
        )?,
        "mooshroom/variant" => enumeration(t, &["red", "brown"])?,
        "rabbit/variant" => enumeration(
            t,
            &[
                "brown",
                "white",
                "black",
                "white_splotched",
                "gold",
                "salt",
                "evil",
            ],
        )?,
        "horse/variant" => enumeration(
            t,
            &[
                "white",
                "creamy",
                "chestnut",
                "brown",
                "black",
                "gray",
                "dark_brown",
            ],
        )?,
        "llama/variant" => enumeration(t, &["creamy", "white", "brown", "gray"])?,
        "axolotl/variant" => enumeration(t, &["lucy", "wild", "gold", "cyan", "blue"])?,
        "use_effects" => field(t, "speed_multiplier", |v| real(v, 0.0, 1.0, false))?,
        "food" => field(t, "nutrition", |v| integer(v, 0, i32::MAX))?,
        "consumable" => {
            field(t, "consume_seconds", |v| real(v, 0.0, f32::MAX, false))?;
            field(t, "animation", |v| {
                enumeration(
                    v,
                    &[
                        "none",
                        "eat",
                        "drink",
                        "block",
                        "bow",
                        "trident",
                        "crossbow",
                        "spyglass",
                        "toot_horn",
                        "brush",
                        "bundle",
                        "spear",
                    ],
                )
            })?;
        }
        "use_cooldown" => field(t, "seconds", |v| real(v, 0.0, f32::MAX, true))?,
        "enchantable" => field(t, "value", |v| integer(v, 1, i32::MAX))?,
        "villager_food" => field(t, "nutrition", |v| integer(v, 1, i32::MAX))?,
        "tool" => {
            field(t, "damage_per_block", |v| integer(v, 0, i32::MAX))?;
            if let Some(Tag::List(rules)) = t.get("rules") {
                for rule in rules {
                    field(rule, "speed", |v| real(v, 0.0, f32::MAX, true))?;
                }
            }
        }
        "weapon" => {
            field(t, "item_damage_per_attack", |v| integer(v, 0, i32::MAX))?;
            field(t, "disable_blocking_for_seconds", |v| {
                real(v, 0.0, f32::MAX, false)
            })?;
        }
        "attack_range" => {
            for k in [
                "min_reach",
                "max_reach",
                "min_creative_reach",
                "max_creative_reach",
            ] {
                field(t, k, |v| real(v, 0.0, 64.0, false))?;
            }
            field(t, "hitbox_margin", |v| real(v, 0.0, 1.0, false))?;
            field(t, "mob_factor", |v| real(v, 0.0, 2.0, false))?;
        }
        "equippable" => field(t, "slot", |v| {
            enumeration(
                v,
                &[
                    "mainhand", "feet", "legs", "chest", "head", "offhand", "body", "saddle",
                ],
            )
        })?,
        "kinetic_weapon" => {
            for k in ["contact_cooldown_ticks", "delay_ticks"] {
                field(t, k, |v| integer(v, 0, i32::MAX))?;
            }
            for k in [
                "dismount_conditions",
                "knockback_conditions",
                "damage_conditions",
            ] {
                if let Some(v) = t.get(k) {
                    field(v, "max_duration_ticks", |v| integer(v, 0, i32::MAX))?;
                }
            }
        }
        "attack_animation" | "interact_animation" => {
            field(t, "type", |v| enumeration(v, &["none", "whack", "stab"]))?;
            field(t, "duration", |v| integer(v, 0, i32::MAX))?;
        }
        "mob_visibility" => field(t, "visibility", |v| real(v, 0.0, 10.0, false))?,
        "firework_explosion" => field(t, "shape", |v| {
            enumeration(v, &["small_ball", "large_ball", "star", "creeper", "burst"])
        })?,
        "instrument" => {
            field(t, "use_duration", |v| real(v, 0.0, f32::MAX, false))?;
            field(t, "range", |v| real(v, 0.0, f32::MAX, true))?;
            field(t, "durability_damage", |v| integer(v, 0, i32::MAX))?;
        }
        "apply_effects" => field(t, "probability", |v| real(v, 0.0, 1.0, false))?,
        "teleport_randomly" => field(t, "diameter", |v| real(v, 0.0, f32::MAX, true))?,
        "lore" => limit(t, 256)?,
        "fireworks" => field(t, "explosions", |v| limit(v, 256))?,
        "writable_book_content" => {
            field(t, "pages", |v| {
                limit(v, 100)?;
                if let Tag::List(v) = v {
                    for v in v {
                        string_limit(v, 1024)?;
                    }
                }
                Ok(())
            })?;
        }
        "written_book_content" => {
            field(t, "title", |v| string_limit(v, 32))?;
            field(t, "generation", |v| integer(v, 0, 3))?;
        }
        "blocks_attacks" => {
            for k in ["block_delay_seconds", "disable_cooldown_scale"] {
                field(t, k, |v| real(v, 0.0, f32::MAX, false))?;
            }
            if let Some(Tag::List(reductions)) = t.get("damage_reductions") {
                for v in reductions {
                    field(v, "horizontal_blocking_angle", |v| {
                        real(v, 0.0, f32::MAX, true)
                    })?;
                }
            }
        }
        _ => (),
    }
    Ok(())
}
fn attribute_entry(t: &Tag) -> Result<Value> {
    fn enumeration(t: &Tag, values: &[&str]) -> Result<Value> {
        let name = string(t)?;
        if !values.contains(&name.as_str()) {
            return Err(format!("unknown enum value {name}"));
        }
        string_value(t)
    }
    record(t, |r| {
        r.required("type", ident)?;
        r.required("id", ident)?;
        r.required("amount", double)?;
        r.required("operation", |v| {
            enumeration(
                v,
                &["add_value", "add_multiplied_base", "add_multiplied_total"],
            )
        })?;
        r.id("slot", "any", |v| {
            enumeration(
                v,
                &[
                    "any", "mainhand", "offhand", "hand", "feet", "legs", "chest", "head", "armor",
                    "body", "saddle",
                ],
            )
        })?;
        if let Some(display) = t.get("display") {
            let name = string(
                display
                    .get("type")
                    .ok_or("missing attribute display type")?,
            )?;
            if name != "default" {
                let display = record(display, |r| {
                    r.required("type", |v| enumeration(v, &["hidden", "override"]))?;
                    if name == "override" {
                        r.required("value", text)?;
                    }
                    Ok(())
                })?;
                r.fields.push((Value::string("display"), display));
            }
        }
        Ok(())
    })
}
fn sign_text(t: &Tag) -> Result<Value> {
    record(t, |r| {
        r.required("messages", sign_messages)?;
        if let Some(filtered) = t
            .get("filtered_messages")
            .and_then(|v| sign_messages(v).ok())
            && Some(&filtered) != r.fields.first().map(|(_, v)| v)
        {
            r.fields
                .push((Value::string("filtered_messages"), filtered));
        }
        r.fields.push((
            Value::string("color"),
            t.get("color")
                .map(string_value)
                .transpose()?
                .unwrap_or_else(|| Value::string("black")),
        ));
        r.fields.push((
            Value::string("has_glowing_text"),
            t.get("has_glowing_text")
                .map(boolean)
                .transpose()?
                .unwrap_or(Value::Bool(false)),
        ));
        Ok(())
    })
}
fn rgb(t: &Tag) -> Result<Value> {
    if matches!(
        t,
        Tag::List(_) | Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_)
    ) {
        let Value::List(v) = list(t, float)? else {
            unreachable!()
        };
        let [r, g, b] = v.as_slice() else {
            return Err("RGB vector requires three channels".into());
        };
        let channel = |v: &Value| -> i32 {
            let Value::Float(v) = v else { unreachable!() };
            let v = f32::from_bits(*v) * 255.0;
            let n = v as i32;
            n.wrapping_sub(i32::from(v < (n as f32))) & 255
        };
        Ok(Value::Int(
            (255u32 << 24) as i32 | (channel(r) << 16) | (channel(g) << 8) | channel(b),
        ))
    } else {
        int(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(s: &str) -> Tag {
        sunrise_core::nbt::parse_snbt(s).unwrap().0
    }
    #[test]
    fn source_alternatives_and_always_present_defaults_use_codec_types() {
        assert_eq!(
            from_component_tag("minecraft:potion_contents", &parse(r#""healing""#))
                .unwrap()
                .to_tag()
                .unwrap(),
            parse(r#"{potion:"minecraft:healing"}"#)
        );
        assert_eq!(
            from_component_tag("minecraft:profile", &parse(r#""Player""#))
                .unwrap()
                .to_tag()
                .unwrap(),
            parse(r#"{name:"Player"}"#)
        );
        let effects = from_component_tag(
            "minecraft:potion_contents",
            &parse(r#"{custom_effects:[{id:"speed",amplifier:255,show_particles:0b}]}"#),
        )
        .unwrap();
        assert_eq!(
            effects.to_tag().unwrap(),
            parse(
                r#"{custom_effects:[{id:"minecraft:speed",amplifier:-1b,show_particles:0b,show_icon:0b}]}"#
            )
        );
        let sign = from_component_tag(
            "minecraft:sign_text_front",
            &parse(r#"{messages:["a","b","c","d"],filtered_messages:["bad"]}"#),
        )
        .unwrap();
        assert_eq!(
            sign.to_tag().unwrap(),
            parse(r#"{messages:["a","b","c","d"],color:"black",has_glowing_text:0b}"#)
        );
    }
    #[test]
    fn numeric_source_alternatives_follow_java_conversion_not_tag_identity() {
        assert_eq!(
            from_component_tag("minecraft:map_id", &Tag::Long(0x1_0000_0001)).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            from_component_tag("minecraft:dyed_color", &parse("[1.0f,0.0f,0.5f]")).unwrap(),
            Value::Int(0xffff007fu32 as i32)
        );
        let value = from_component_tag(
            "minecraft:custom_model_data",
            &parse("{floats:[I;1,2],flags:[B;0b,1b]}"),
        )
        .unwrap();
        assert_eq!(
            value.to_tag().unwrap(),
            parse("{floats:[1.0f,2.0f],flags:[0b,1b]}")
        );
    }
    #[test]
    fn upgraded_native_templates_do_not_use_persistent_count_bounds() {
        for count in [i32::MIN, -7, 0, 1, 99, 100, 127, i32::MAX] {
            let item = Tag::compound([
                ("id", Tag::String("minecraft:stone".into())),
                ("count", Tag::Int(count)),
            ]);
            let tag = Tag::List(vec![item]);
            let source = from_component_tag("minecraft:bundle_contents", &tag);
            let native = from_upgraded_component_tag("minecraft:bundle_contents", &tag);
            if count == 0 {
                assert!(source.is_err());
                assert!(native.is_err());
                continue;
            }
            let native = native.unwrap();
            if (1..=99).contains(&count) {
                assert_eq!(source.unwrap(), native);
            } else {
                assert!(source.is_err());
                assert!(native.to_tag().is_err());
            }
            assert!(
                matches!(&native,Value::List(values) if matches!(&values[0],Value::Template(item) if item.count==count))
            );
        }
    }
    #[test]
    fn persistent_input_refuses_transient_components_by_name_but_native_stacks_keep_them() {
        for name in ["creative_slot_lock", "map_post_processing", "additional_trade_cost"] {
            let value = if name == "creative_slot_lock" {
                Tag::Compound(Default::default())
            } else {
                Tag::Int(3)
            };
            let error = from_component_tag(&format!("minecraft:{name}"), &value).unwrap_err();
            assert!(error.contains(name), "{error}");
            assert!(from_upgraded_component_tag(&format!("minecraft:{name}"), &value).is_ok());
        }
        let removal = parse(r#"{id:"minecraft:stone",components:{"!minecraft:map_post_processing":{}}}"#);
        assert!(template_from_tag(&removal).unwrap_err().contains("map_post_processing"));
    }
    #[test]
    fn float_ranges_follow_java_ordering_and_refuse_negative_zero_at_a_zero_minimum() {
        let float = |bits: u32| Tag::Float(bits);
        assert!(from_component_tag("minecraft:potion_duration_scale", &float(0)).is_ok());
        assert!(from_component_tag("minecraft:potion_duration_scale", &float(0x8000_0000)).is_err());
        assert!(from_component_tag("minecraft:potion_duration_scale", &float(f32::NAN.to_bits())).is_err());
        assert!(from_component_tag("minecraft:potion_duration_scale", &float(f32::INFINITY.to_bits())).is_err());
        let cooldown = |bits: u32| Tag::compound([("seconds", float(bits))]);
        assert!(from_component_tag("minecraft:use_cooldown", &cooldown(1.0f32.to_bits())).is_ok());
        assert!(from_component_tag("minecraft:use_cooldown", &cooldown(0)).is_err());
        assert!(from_component_tag("minecraft:use_cooldown", &cooldown(0x8000_0000)).is_err());
    }
    #[test]
    fn trees_nested_past_the_nbt_depth_are_refused_before_conversion() {
        let nested = |levels: usize| {
            let mut tag = Tag::Compound(Default::default());
            for _ in 1..levels {
                tag = Tag::compound([("a", tag)]);
            }
            tag
        };
        let convert = |levels: usize| {
            std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || from_component_tag("minecraft:custom_data", &nested(levels)).is_ok())
                .unwrap()
                .join()
                .unwrap()
        };
        assert!(convert(super::super::MAX_TREE_DEPTH));
        assert!(!convert(super::super::MAX_TREE_DEPTH + 1));
        let mut value = Value::Int(0);
        for _ in 0..=super::super::MAX_TREE_DEPTH {
            value = Value::List(vec![value]);
        }
        assert!(value.to_tag().is_err());
        assert!(Value::from_nbt(&nested(super::super::MAX_TREE_DEPTH + 1), &[]).is_err());
    }
}
