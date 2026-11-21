//! A schema walk over the admitted corpus. Unlike `content_golden`, this test
//! uses the current reader's schema, not the previous release's.

use serde::de::{
    Deserialize, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, VariantAccess,
    Visitor,
    value::{MapDeserializer, SeqDeserializer},
};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeSet, fs};
use sunrise_mod_package::{
    Content, Manifest, Package,
    content::{
        DYE_COLORS, ENTITY_TEMPLATES, ShapeEntityKind, StewEffect, entity_sounds::Mute,
        probe_declaration,
    },
    manifest::{DependencyDetails, JarRef, MANIFEST, RuleDetails},
};

#[test]
fn content_keys() {
    let coverage = RefCell::new(Coverage::default());
    let empty = Value::Object(Default::default());
    let schema = Probe {
        value: &empty,
        coverage: &coverage,
    };
    // The callbacks publish the top-level fields even when the empty object fails
    // to decode. A declaration kind's absence must not hide its entire schema.
    for kind in [
        "block",
        "item",
        "mob_effect",
        "creative_tab",
        "fluid",
        "fluid_interaction",
        "entity_type",
        "menu",
    ] {
        drop(probe_declaration(kind, schema));
    }
    drop(JarRef::deserialize(schema));
    drop(DependencyDetails::deserialize(schema));
    drop(RuleDetails::deserialize(schema));
    drop(StewEffect::deserialize(schema));
    let unknown = Value::String(String::new());
    let variants = Probe {
        value: &unknown,
        ..schema
    };
    let _ = ShapeEntityKind::deserialize(variants);
    let _ = Mute::deserialize(variants);

    for entry in fs::read_dir("tests/content-corpus").unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_str().unwrap().starts_with("refused_") {
            continue;
        }
        let path = entry.path();
        let manifest_path = path.join(MANIFEST);
        let toml: toml::Value =
            toml::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
        let value = serde_json::to_value(toml).unwrap();
        let probe = Probe {
            value: &value,
            coverage: &coverage,
        };
        Manifest::deserialize(probe).unwrap_or_else(|e| panic!("{}: {e}", manifest_path.display()));

        // Serde's untagged wrappers buffer through deserialize_any and replay
        // internally. Probe the public detailed forms separately.
        for table in ["dependencies", "incompatible", "discouraged"] {
            if let Some(rows) = value.get(table).and_then(Value::as_object) {
                for row in rows.values().filter(|row| row.is_object()) {
                    let row = Probe {
                        value: row,
                        ..probe
                    };
                    if table == "dependencies" {
                        DependencyDetails::deserialize(row).unwrap();
                    } else {
                        RuleDetails::deserialize(row).unwrap();
                    }
                }
            }
        }

        let package = Package::read(&path).unwrap();
        let content = Content::read(&path, &package.manifest).unwrap();
        let mut reader = content.reader().unwrap();
        for file in &content.files {
            let mut parts = file.splitn(5, '/');
            if parts.next() != Some("data") {
                continue;
            }
            let ns = parts.next().unwrap();
            if parts.next() != Some("sunrise")
                || !package.manifest.owned_namespaces().any(|owned| owned == ns)
            {
                continue;
            }
            let kind = parts.next().unwrap();
            let bytes = reader.read(file).unwrap();
            let value = serde_json::from_slice(&bytes).unwrap();
            let probe = Probe {
                value: &value,
                coverage: &coverage,
            };
            probe_declaration(kind, probe).unwrap_or_else(|e| panic!("{file}: {e}"));
            if kind == "block" {
                // MapDeserializer hands map keys to Serde as plain strings.
                for field in ["outline_context_shapes", "collision_context_shapes"] {
                    if let Some(shapes) = value.get(field).and_then(Value::as_object) {
                        for key in shapes.keys() {
                            let key = Value::String(key.to_owned());
                            ShapeEntityKind::deserialize(Probe {
                                value: &key,
                                ..probe
                            })
                            .unwrap();
                        }
                    }
                }
            }
            if kind == "entity_type"
                && let Some(sounds) = value.get("sounds").and_then(Value::as_object)
            {
                for value in sounds.values() {
                    // Named sound IDs are the other, non-Mute branch of SoundOverride.
                    let _ = Mute::deserialize(Probe { value, ..probe });
                }
            }
        }

        // These readers validate strings against their own finite vocabularies,
        // not Serde enums. Their actual reader constants are the schema.
        let mut evidence = coverage.borrow_mut();
        let instruments = content.blocks.iter().filter_map(|block| {
            block
                .properties
                .instrument
                .map(|instrument| instrument.name())
        });
        evidence.record(
            "NoteInstrument",
            mod_vocab::NoteInstrument::ALL
                .iter()
                .map(|instrument| instrument.name()),
            instruments,
        );
        let colors = content
            .blocks
            .iter()
            .filter_map(|block| block.sign_text_color.as_deref());
        evidence.record("DyeColor", DYE_COLORS, colors);
        let templates = content
            .entity_types
            .iter()
            .map(|entity| entity.template.as_str());
        evidence.record("EntityType.template", ENTITY_TEMPLATES, templates);
    }

    let coverage = coverage.into_inner();
    let missing: Vec<_> = coverage
        .keys
        .difference(&coverage.seen)
        .filter(|&&(name, key)| {
            // The content-only reader explicitly rejects these tables without [native].
            name != "Manifest" || !matches!(key, "native" | "runtime" | "internals")
        })
        .map(|(name, key)| format!("{name}.{key}"))
        .collect();
    assert!(
        !coverage.keys.is_empty(),
        "no declaration schema was probed"
    );
    assert!(
        missing.is_empty(),
        "no admitted fixture for:\n{}",
        missing.join("\n")
    );
}

#[derive(Default)]
struct Coverage {
    keys: BTreeSet<(&'static str, &'static str)>,
    seen: BTreeSet<(&'static str, &'static str)>,
}

impl Coverage {
    fn record<'a>(
        &mut self,
        name: &'static str,
        keys: impl IntoIterator<Item = &'static str>,
        seen: impl IntoIterator<Item = &'a str>,
    ) {
        let seen: BTreeSet<_> = seen.into_iter().collect();
        for key in keys {
            self.keys.insert((name, key));
            if seen.contains(key) {
                self.seen.insert((name, key));
            }
        }
    }
}

/// Replay the corpus through Serde, recording schema names before visiting
/// their fixture values. Reusing admitted values lets the real validators run
/// and reaches the nested shapes their authors actually use.
#[derive(Clone, Copy)]
struct Probe<'a> {
    value: &'a Value,
    coverage: &'a RefCell<Coverage>,
}

impl<'de> IntoDeserializer<'de, serde_json::Error> for Probe<'de> {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> Deserializer<'de> for Probe<'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value {
            Value::Object(map) => visitor.visit_map(MapDeserializer::new(
                map.iter()
                    .map(|(key, value)| (key.as_str(), Probe { value, ..self })),
            )),
            Value::Array(seq) => visitor.visit_seq(SeqDeserializer::new(
                seq.iter().map(|value| Probe { value, ..self }),
            )),
            value => value.deserialize_any(visitor),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if let Value::Object(map) = self.value {
            self.coverage.borrow_mut().record(
                name,
                fields.iter().copied(),
                map.iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(key, _)| key.as_str()),
            );
        }
        self.deserialize_any(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.value {
            Value::String(variant) => {
                self.coverage.borrow_mut().record(
                    name,
                    variants.iter().copied(),
                    [variant.as_str()],
                );
                visitor.visit_enum(variant.as_str().into_deserializer())
            }
            Value::Object(map) if map.len() == 1 => {
                let (variant, value) = map.iter().next().unwrap();
                let variant = variant.as_str();
                self.coverage
                    .borrow_mut()
                    .record(name, variants.iter().copied(), [variant]);
                visitor.visit_enum(Tagged {
                    name,
                    variant,
                    payload: Probe { value, ..self },
                })
            }
            value => value.deserialize_enum(name, variants, visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.value.is_null() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        unit unit_struct seq tuple tuple_struct map identifier ignored_any
    }
}

struct Tagged<'de> {
    name: &'static str,
    variant: &'de str,
    payload: Probe<'de>,
}

impl<'de> EnumAccess<'de> for Tagged<'de> {
    type Error = serde_json::Error;
    type Variant = Self;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), Self::Error> {
        Ok((seed.deserialize(self.variant.into_deserializer())?, self))
    }
}

impl<'de> VariantAccess<'de> for Tagged<'de> {
    type Error = serde_json::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        <()>::deserialize(self.payload)
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, Self::Error> {
        seed.deserialize(self.payload)
    }

    fn tuple_variant<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.payload.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.payload.deserialize_struct(self.name, fields, visitor)
    }
}
