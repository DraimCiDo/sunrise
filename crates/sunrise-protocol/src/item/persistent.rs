//! Current pinned component codecs with explicit registry callbacks.
//! Canonical values retain stream-only state; persistent encoding is a read-only projection.
use super::{Component, Names, Stack};
use crate::{Error, Result, buf::Reader};
use sunrise_core::nbt::Tag;
use sunrise_world::component_value::{Patch, Template as ValueTemplate, Value};

mod source;
pub(crate) use source::component_from_upgraded_tag;
pub use source::{component_from_tag, component_from_value, template_from_parts};

/// The output direction only needs names to ids, not a bidirectional registry.
pub trait Ids {
    fn id(&self, registry: &str, name: &str) -> Option<i32>;
}
impl<F: Fn(&str, &str) -> Option<i32>> Ids for F {
    fn id(&self, registry: &str, name: &str) -> Option<i32> {
        self(registry, name)
    }
}
/// Adapter for a name-only lookup callback.
#[non_exhaustive]
pub struct NameLookup<'a>(pub &'a dyn Fn(&str, i32) -> Option<String>);
impl<'a> NameLookup<'a> {
    pub fn new(lookup: &'a dyn Fn(&str, i32) -> Option<String>) -> Self {
        Self(lookup)
    }
}
impl Names for NameLookup<'_> {
    fn name(&self, registry: &str, id: i32) -> Option<String> {
        (self.0)(registry, id)
    }
}

/// DataComponents' registration order, from the pinned 26.3 registry report.
const COMPONENTS: [&str; 122] = [
    "minecraft:custom_data",
    "minecraft:max_stack_size",
    "minecraft:max_damage",
    "minecraft:damage",
    "minecraft:unbreakable",
    "minecraft:use_effects",
    "minecraft:custom_name",
    "minecraft:minimum_attack_charge",
    "minecraft:damage_type",
    "minecraft:item_name",
    "minecraft:item_model",
    "minecraft:lore",
    "minecraft:rarity",
    "minecraft:enchantments",
    "minecraft:can_place_on",
    "minecraft:can_break",
    "minecraft:attribute_modifiers",
    "minecraft:custom_model_data",
    "minecraft:tooltip_display",
    "minecraft:repair_cost",
    "minecraft:creative_slot_lock",
    "minecraft:enchantment_glint_override",
    "minecraft:intangible_projectile",
    "minecraft:food",
    "minecraft:consumable",
    "minecraft:use_remainder",
    "minecraft:use_cooldown",
    "minecraft:damage_resistant",
    "minecraft:tool",
    "minecraft:weapon",
    "minecraft:attack_range",
    "minecraft:enchantable",
    "minecraft:equippable",
    "minecraft:repairable",
    "minecraft:glider",
    "minecraft:tooltip_style",
    "minecraft:death_protection",
    "minecraft:blocks_attacks",
    "minecraft:piercing_weapon",
    "minecraft:kinetic_weapon",
    "minecraft:attack_animation",
    "minecraft:interact_animation",
    "minecraft:additional_trade_cost",
    "minecraft:block_transformer",
    "minecraft:villager_food",
    "minecraft:stored_enchantments",
    "minecraft:dye",
    "minecraft:dyed_color",
    "minecraft:map_id",
    "minecraft:map_decorations",
    "minecraft:map_post_processing",
    "minecraft:charged_projectiles",
    "minecraft:bundle_contents",
    "minecraft:potion_contents",
    "minecraft:potion_duration_scale",
    "minecraft:suspicious_stew_effects",
    "minecraft:writable_book_content",
    "minecraft:written_book_content",
    "minecraft:trim",
    "minecraft:debug_stick_state",
    "minecraft:entity_data",
    "minecraft:bucket_entity_data",
    "minecraft:block_entity_data",
    "minecraft:instrument",
    "minecraft:provides_trim_material",
    "minecraft:ominous_bottle_amplifier",
    "minecraft:jukebox_playable",
    "minecraft:provides_banner_patterns",
    "minecraft:recipes",
    "minecraft:lodestone_tracker",
    "minecraft:firework_explosion",
    "minecraft:fireworks",
    "minecraft:profile",
    "minecraft:note_block_sound",
    "minecraft:banner_patterns",
    "minecraft:base_color",
    "minecraft:pot_decorations",
    "minecraft:container",
    "minecraft:block_state",
    "minecraft:bees",
    "minecraft:sulfur_cube_content",
    "minecraft:lock",
    "minecraft:container_loot",
    "minecraft:break_sound",
    "minecraft:compostable",
    "minecraft:cooking_fuel",
    "minecraft:brewing_fuel",
    "minecraft:mob_visibility",
    "minecraft:villager/variant",
    "minecraft:wolf/variant",
    "minecraft:wolf/sound_variant",
    "minecraft:wolf/collar",
    "minecraft:fox/variant",
    "minecraft:salmon/size",
    "minecraft:parrot/variant",
    "minecraft:tropical_fish/pattern",
    "minecraft:tropical_fish/base_color",
    "minecraft:tropical_fish/pattern_color",
    "minecraft:mooshroom/variant",
    "minecraft:rabbit/variant",
    "minecraft:pig/variant",
    "minecraft:pig/sound_variant",
    "minecraft:cow/variant",
    "minecraft:cow/sound_variant",
    "minecraft:chicken/variant",
    "minecraft:chicken/sound_variant",
    "minecraft:zombie_nautilus/variant",
    "minecraft:frog/variant",
    "minecraft:horse/variant",
    "minecraft:painting/variant",
    "minecraft:llama/variant",
    "minecraft:axolotl/variant",
    "minecraft:cat/variant",
    "minecraft:cat/sound_variant",
    "minecraft:cat/collar",
    "minecraft:sheep/color",
    "minecraft:shulker/color",
    "minecraft:provides_pottery_pattern",
    "minecraft:sign_text_front",
    "minecraft:sign_text_back",
    "minecraft:waxed",
    "minecraft:cushion/color",
];
/// Built-in registry enumeration excludes process-installed native components.
pub const BUILTIN_COMPONENT_COUNT: usize = COMPONENTS.len();
pub fn builtin_component_name(kind: i32) -> Option<&'static str> {
    usize::try_from(kind)
        .ok()
        .and_then(|kind| COMPONENTS.get(kind))
        .copied()
}
pub fn builtin_component_id(name: &str) -> Option<i32> {
    COMPONENTS
        .iter()
        .position(|candidate| *candidate == name)
        .map(|id| id as i32)
}
/// Trusted canonical values may also refer to an installed native component.
pub fn component_name(kind: i32) -> Option<&'static str> {
    builtin_component_name(kind).or_else(|| super::custom::name(kind))
}
pub fn component_id(name: &str) -> Option<i32> {
    builtin_component_id(name).or_else(|| super::custom::id(name))
}

/// Decode every own patch entry without prototype merging or persistence conversion.
pub fn template_value(stack: &Stack, names: &dyn Names) -> Result<ValueTemplate> {
    let _nested = super::Nested::enter()?;
    if stack.count == 0 || stack.item <= 0 {
        return Err(Error::new(
            "item template must have a non-air item and nonzero count",
        ));
    }
    let item = names
        .name("minecraft:item", stack.item)
        .filter(|name| name != "minecraft:air")
        .ok_or_else(|| Error::new("item template registry id has no name"))?;
    let mut patch: Patch = Vec::with_capacity(stack.added.len() + stack.removed.len());
    for (kind, value) in &stack.added {
        let name = component_name(*kind)
            .ok_or_else(|| Error::new(format!("unknown pinned component type {kind}")))?;
        set_patch(
            &mut patch,
            name,
            Some(component_value(*kind, value, names)?),
        );
    }
    for kind in &stack.removed {
        let name = component_name(*kind)
            .ok_or_else(|| Error::new(format!("unknown removed pinned component type {kind}")))?;
        set_patch(&mut patch, name, None);
    }
    patch.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ValueTemplate::new(item, stack.count, patch))
}
fn set_patch(patch: &mut Patch, name: &str, value: Option<Value>) {
    if let Some((_, old)) = patch.iter_mut().find(|(key, _)| key == name) {
        *old = value;
    } else {
        patch.push((name.to_owned(), value));
    }
}

/// The actual canonical value, not an opaque stream cache or a component hash.
pub fn component_value(kind: i32, value: &Component, names: &dyn Names) -> Result<Value> {
    if component_name(kind).is_none() {
        return Err(Error::new(format!("unknown pinned component type {kind}")));
    }
    value.code_with::<super::hash::ops::Values>(kind, names)
        .ok_or_else(|| Error::new(format!("cannot decode complete value of component {kind}: invalid payload or missing registry name")))
}
/// `None` is a deliberately transient component; errors do not modify the value.
pub fn component_to_tag(kind: i32, value: &Component, names: &dyn Names) -> Result<Option<Tag>> {
    if matches!(kind, 20 | 42 | 50) {
        return Ok(None);
    }
    let name = component_name(kind)
        .ok_or_else(|| Error::new(format!("unknown pinned component type {kind}")))?;
    let tag = component_value(kind, value, names)?
        .to_tag()
        .map_err(Error::new)?;
    // The stream carries values the saved codec refuses (negative damage, out-of-range multipliers):
    // refuse them here and leave the retained value as it was.
    sunrise_world::component_value::source::validate_persistent(name, &tag).map_err(Error::new)?;
    Ok(Some(tag))
}

pub(crate) fn special_value(
    kind: i32,
    value: &Component,
    names: &dyn Names,
) -> Option<Option<Value>> {
    let result = if matches!(kind, 20 | 42 | 50) {
        Some(transient_value(kind, value))
    } else if let Component::Raw(bytes) = value {
        // Typed components may also arrive from a caller as pinned raw values.
        // Interpret them through the existing typed stream codec, not another parser.
        if matches!(kind, 0..=4 | 6 | 9..=13 | 17..=19 | 21 | 30 | 45 | 47 | 48 | 51..=53 | 55..=57 | 65 | 69..=72 | 75)
            || super::custom::contains(kind)
        {
            Some((|| {
                let mut reader = Reader::new(bytes);
                let decoded = Component::read(kind, &mut reader)?;
                if reader.remaining() != 0 {
                    return Err(Error::new("trailing component bytes"));
                }
                component_value(kind, &decoded, names)
            })())
        } else {
            None
        }
    } else {
        None
    };
    result.map(Result::ok)
}
fn transient_value(kind: i32, value: &Component) -> Result<Value> {
    match (kind, value) {
        (20, Component::Unit) => Ok(Value::Map(Vec::new())),
        (42 | 50, Component::Int(value)) => Ok(Value::Int(*value)),
        (20, Component::Raw(bytes)) if bytes.is_empty() => Ok(Value::Map(Vec::new())),
        (42 | 50, Component::Raw(bytes)) => {
            let mut reader = Reader::new(bytes);
            let value = reader.varint()?;
            if reader.remaining() != 0 {
                return Err(Error::new("trailing transient component bytes"));
            }
            Ok(Value::Int(value))
        }
        _ => Err(Error::new("wrong transient component value")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        buf::{Codec, Writer},
        item::Template,
    };

    struct Registry;
    impl Names for Registry {
        fn name(&self, registry: &str, id: i32) -> Option<String> {
            match (registry, id) {
                ("minecraft:item", 1) => Some("minecraft:stone".into()),
                ("minecraft:entity_type", 0) => Some("minecraft:cow".into()),
                ("minecraft:sound_event", 0) => Some("minecraft:item.armor.equip_generic".into()),
                ("minecraft:sound_event", 1) => Some("minecraft:item.shears.snip".into()),
                _ => None,
            }
        }
    }
    fn registry_id(registry: &str, name: &str) -> Option<i32> {
        match (registry, name) {
            ("minecraft:item", "minecraft:stone") => Some(1),
            ("minecraft:entity_type", "minecraft:cow") => Some(0),
            ("minecraft:sound_event", "minecraft:item.armor.equip_generic") => Some(0),
            ("minecraft:sound_event", "minecraft:item.shears.snip") => Some(1),
            _ => None,
        }
    }
    #[test]
    fn full_equippable_and_unit_survive_parent_patch_change() {
        let mut wire = Writer::new();
        wire.varint(3); // chest
        wire.varint(1); // reference equip sound 0
        wire.bool(true);
        wire.string("minecraft:equipment/test");
        wire.bool(true);
        wire.string("minecraft:overlay/test");
        wire.bool(true);
        wire.varint(2);
        wire.varint(0); // one allowed entity
        for flag in [false, false, false, true, true] {
            wire.bool(flag);
        }
        wire.varint(2); // reference shearing sound 1
        let mut stack = Stack::new(1, 1);
        stack.added.push((32, Component::Raw(wire.data)));
        stack.added.push((34, Component::Raw(Vec::new())));
        let original = template_value(&stack, &Registry).unwrap();
        let mut mutated = original.clone();
        mutated
            .patch
            .push(("minecraft:damage".into(), Some(Value::Int(9))));
        let rebuilt =
            template_from_parts(&mutated.item, mutated.count, &mutated.patch, &registry_id)
                .unwrap();
        let rebuilt = template_value(&rebuilt, &Registry).unwrap();
        for id in ["minecraft:equippable", "minecraft:glider"] {
            assert_eq!(
                original.patch.iter().find(|(key, _)| key == id),
                rebuilt.patch.iter().find(|(key, _)| key == id)
            );
        }
    }
    #[test]
    fn inline_jukebox_song_survives_wire_but_refuses_source() {
        let mut wire = Writer::new();
        wire.varint(0); // direct song
        wire.varint(0);
        wire.string("minecraft:test.song");
        wire.bool(false); // direct sound
        wire.nbt(&Tag::String("song".into()));
        wire.f32(1.5);
        wire.varint(7);
        let value = component_value(66, &Component::Raw(wire.data), &Registry).unwrap();
        assert!(matches!(&value, Value::StreamOnly(_)));
        assert!(value.to_tag().is_err());
        let rebuilt = component_from_value(66, &value, &registry_id).unwrap();
        assert_eq!(component_value(66, &rebuilt, &Registry).unwrap(), value);
        let reference = component_value(
            66,
            &Component::Raw(vec![1]),
            &NameLookup(&|registry, id| {
                (registry == "minecraft:jukebox_song" && id == 0)
                    .then(|| "minecraft:test_song".into())
            }),
        )
        .unwrap();
        assert_ne!(reference, value);
    }
    #[test]
    fn transient_components_are_wire_values_not_persistent_entries() {
        let template = ValueTemplate::new(
            "minecraft:stone".into(),
            1,
            vec![
                (
                    "minecraft:creative_slot_lock".into(),
                    Some(Value::Map(Vec::new())),
                ),
                (
                    "minecraft:additional_trade_cost".into(),
                    Some(Value::Int(4)),
                ),
                ("minecraft:map_post_processing".into(), Some(Value::Int(1))),
            ],
        );
        assert!(template.to_tag().unwrap().get("components").is_none());
        let wire = template_from_parts(
            &template.item,
            template.count,
            &template.patch,
            &registry_id,
        )
        .unwrap();
        assert_eq!(template_value(&wire, &Registry).unwrap(), template);
    }
    #[test]
    fn java_float_identity_keeps_negative_zero_in_default_records() {
        let reduction = |base: f32| {
            let mut wire = Writer::new();
            wire.f32(0.0);
            wire.f32(1.0);
            wire.varint(1);
            wire.f32(90.0);
            wire.bool(false);
            wire.f32(base);
            wire.f32(1.0);
            wire.f32(1.0);
            wire.f32(0.0);
            wire.f32(1.0);
            wire.bool(false);
            wire.bool(false);
            wire.bool(false);
            component_value(37, &Component::Raw(wire.data), &Registry).unwrap()
        };
        assert_ne!(reduction(0.0), reduction(-0.0));
        let nan = |bits: u32| {
            let mut wire = Writer::new();
            wire.f32(f32::from_bits(bits));
            component_value(7, &Component::Raw(wire.data), &Registry).unwrap()
        };
        assert_eq!(nan(0x7fc00001), nan(0x7fc00002));
    }
    #[test]
    fn template_stream_refuses_air_and_zero_but_keeps_negative_count() {
        for (item, count) in [
            (0, 1),
            (1, 0),
            (1, -7),
            (1, 127),
            (1, i32::MIN),
            (1, i32::MAX),
        ] {
            let mut bytes = Writer::new();
            bytes.varint(item);
            bytes.varint(count);
            bytes.varint(0);
            bytes.varint(0);
            let value = Template::read(&mut Reader::new(&bytes.data));
            if item > 0 && count != 0 {
                let stack = value.unwrap();
                assert_eq!(stack.count, count);
                let canonical = template_value(&stack, &Registry).unwrap();
                assert!(canonical.to_tag().is_err());
                assert_eq!(
                    template_from_parts(
                        &canonical.item,
                        canonical.count,
                        &canonical.patch,
                        &registry_id
                    )
                    .unwrap(),
                    stack
                );
            } else {
                assert!(value.is_err());
            }
        }
    }
    #[test]
    fn literal_text_wrappers_share_source_and_stream_identity() {
        let literal = Tag::String("literal".into());
        let compound = Tag::compound([("text", literal.clone())]);
        let singleton = Tag::List(vec![compound.clone()]);
        let expected = component_value(6, &Component::Text(literal), &Registry).unwrap();
        for tag in [compound, singleton] {
            assert_eq!(
                component_value(6, &Component::Text(tag.clone()), &Registry).unwrap(),
                expected
            );
            assert_eq!(
                sunrise_world::component_value::from_component_tag("minecraft:custom_name", &tag)
                    .unwrap(),
                expected
            );
        }
    }
    #[test]
    fn negative_holder_ids_refuse_without_arithmetic_overflow() {
        for id in [-1, i32::MIN] {
            let mut wire = Writer::new();
            wire.varint(10);
            wire.varint(0);
            wire.bool(false);
            wire.bool(false);
            wire.bool(false);
            wire.f32(0.0);
            wire.f32(1.0);
            wire.bool(true);
            wire.varint(id);
            wire.bool(false);
            assert!(component_value(39, &Component::Raw(wire.data), &Registry).is_err());
            for kind in [27, 58, 66, 109] {
                let mut wire = Writer::new();
                wire.varint(id);
                assert!(component_value(kind, &Component::Raw(wire.data), &Registry).is_err());
            }
        }
    }
    fn bundle_of(template: ValueTemplate) -> Value {
        Value::List(vec![Value::Template(Box::new(template))])
    }
    fn stone(patch: Patch) -> ValueTemplate {
        ValueTemplate::new("minecraft:stone".into(), 1, patch)
    }
    #[test]
    fn a_component_named_twice_keeps_its_last_entry_in_every_projection() {
        let damage = || "minecraft:damage".to_owned();
        let set_last = vec![(damage(), None), (damage(), Some(Value::Int(9)))];
        let removed_last = vec![(damage(), Some(Value::Int(9))), (damage(), None)];
        for (patch, kept, key, absent) in [
            (set_last, Some(Value::Int(9)), "minecraft:damage", "!minecraft:damage"),
            (removed_last, None, "!minecraft:damage", "minecraft:damage"),
        ] {
            let stack = template_from_parts("minecraft:stone", 1, &patch, &registry_id).unwrap();
            assert_eq!(
                template_value(&stack, &Registry).unwrap().patch,
                vec![(damage(), kept)]
            );
            // The same patch nested in a bundle takes the nested-writer path.
            let nested = component_from_value(51, &bundle_of(stone(patch.clone())), &registry_id);
            let Component::Stacks(inner) = nested.unwrap() else {
                panic!("bundle contents are stacks");
            };
            assert_eq!(inner[0].added.len() + inner[0].removed.len(), 1);
            let tag = stone(patch).to_tag().unwrap();
            let components = tag.get("components").unwrap();
            assert!(components.get(key).is_some() && components.get(absent).is_none());
        }
    }
    #[test]
    fn component_chains_up_to_the_readers_depth_convert_on_a_network_stack() {
        let chain = |bundles: usize| {
            let mut template = stone(vec![]);
            for _ in 0..bundles {
                template = stone(vec![(
                    "minecraft:bundle_contents".into(),
                    Some(bundle_of(template)),
                )]);
            }
            template
        };
        let convert = |bundles: usize| {
            std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    let top = chain(bundles);
                    template_from_parts(&top.item, top.count, &top.patch, &registry_id).is_ok()
                })
                .unwrap()
                .join()
                .unwrap()
        };
        assert!(convert(64));
        assert!(!convert(65));
    }
    #[test]
    fn persistent_projection_refuses_what_the_saved_codec_refuses() {
        assert!(component_to_tag(3, &Component::Int(-1), &Registry).is_err());
        assert!(component_to_tag(3, &Component::Int(4), &Registry).unwrap().is_some());
        let use_effects = |speed: f32| {
            let mut wire = Writer::new();
            wire.bool(true);
            wire.bool(true);
            wire.f32(speed);
            Component::Raw(wire.data)
        };
        for speed in [2.0, -0.5, -0.0] {
            assert!(component_to_tag(5, &use_effects(speed), &Registry).is_err());
        }
        assert!(component_to_tag(5, &use_effects(0.5), &Registry).unwrap().is_some());
        // The retained value is untouched by the refusal.
        let value = Component::Int(-1);
        let _ = component_to_tag(3, &value, &Registry);
        assert_eq!(value, Component::Int(-1));
        let template = stone(vec![("minecraft:damage".into(), Some(Value::Int(-1)))]);
        assert!(template.to_tag().is_err());
    }
    #[test]
    fn persistent_input_names_the_transient_component_it_refuses() {
        let error = component_from_tag(20, &Tag::Compound(Default::default()), &registry_id)
            .unwrap_err()
            .to_string();
        assert!(error.contains("creative_slot_lock"), "{error}");
    }
    #[test]
    fn a_registry_id_at_the_top_of_the_range_is_refused_not_wrapped() {
        let top = |_: &str, _: &str| Some(i32::MAX);
        let error = component_from_value(64, &Value::string("minecraft:quartz"), &top);
        assert!(error.is_err());
    }
}
