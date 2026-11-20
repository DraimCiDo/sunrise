//! Immutable component codec values shared by item templates and protocol codecs.
use sunrise_core::nbt::{JavaString, Tag};
pub mod source;
pub use source::from_component_tag;

/// The deepest list/compound nesting a tag tree may have in a conversion:
/// `NbtAccounter.MAX_STACK_DEPTH`, the most binary NBT reads.
pub const MAX_TREE_DEPTH: usize = 512;

/// `ComponentSerialization.CODEC` collapses an unstyled literal with no siblings.
/// Borrow the compact form so source and stream traversal share it.
pub fn compact_text_tag(mut value: &Tag) -> &Tag {
    loop {
        match value {
            Tag::List(values) if values.len() == 1 => value = &values[0],
            Tag::Compound(fields) if fields.len() == 1 => match fields.iter().next() {
                Some((key, text @ Tag::String(_))) if key.0 == [116, 101, 120, 116] => {
                    return text;
                }
                _ => return value,
            },
            _ => return value,
        }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Template {
    pub item: String,
    pub count: i32,
    pub patch: Patch,
}
impl Template {
    pub fn new(item: String, count: i32, patch: Patch) -> Self {
        Self { item, count, patch }
    }
}
/// The entries that decide a patch: when a component is named twice the last entry wins,
/// as in the JSON form. Allocation-free; patches hold at most one entry per component type.
pub fn effective_patch(
    patch: &[(String, Option<Value>)],
) -> impl Iterator<Item = &(String, Option<Value>)> {
    patch
        .iter()
        .enumerate()
        .filter(|(index, (name, _))| !patch[index + 1..].iter().any(|(later, _)| later == name))
        .map(|(_, entry)| entry)
}
impl PartialEq for Template {
    fn eq(&self, other: &Self) -> bool {
        self.item == other.item
            && self.count == other.count
            && self.patch.iter().enumerate().all(|(index, (key, value))| {
                self.patch[index + 1..]
                    .iter()
                    .any(|(later, _)| later == key)
                    || other
                        .patch
                        .iter()
                        .rev()
                        .find(|(candidate, _)| candidate == key)
                        .is_some_and(|(_, candidate)| candidate == value)
            })
            && other
                .patch
                .iter()
                .all(|(key, _)| self.patch.iter().any(|(candidate, _)| candidate == key))
    }
}
/// A template's own patch, not its prototype-expanded effective components.
pub type Patch = Vec<(String, Option<Value>)>;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Value {
    Empty,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    Bool(bool),
    String(Vec<u16>),
    ByteArray(Vec<u8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    List(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Set(Vec<Value>),
    Template(Box<Template>),
    Container(Vec<Option<Template>>),
    /// A real stream value for which the persistent codec refuses encoding.
    StreamOnly(Box<Value>),
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Empty, Empty) => true,
            (Byte(a), Byte(b)) => a == b,
            (Short(a), Short(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Long(a), Long(b)) => a == b,
            (Float(a), Float(b)) => {
                a == b || (f32::from_bits(*a).is_nan() && f32::from_bits(*b).is_nan())
            }
            (Double(a), Double(b)) => {
                a == b || (f64::from_bits(*a).is_nan() && f64::from_bits(*b).is_nan())
            }
            (Bool(a), Bool(b)) => a == b,
            (String(a), String(b)) => a == b,
            (ByteArray(a), ByteArray(b)) => a == b,
            (IntArray(a), IntArray(b)) => a == b,
            (LongArray(a), LongArray(b)) => a == b,
            (List(a), List(b)) => a == b,
            (Map(a), Map(b)) => a.len() == b.len() && a.iter().all(|entry| b.contains(entry)),
            (Set(a), Set(b)) => a.iter().all(|v| b.contains(v)) && b.iter().all(|v| a.contains(v)),
            (Template(a), Template(b)) => a == b,
            (Container(a), Container(b)) => a == b,
            (StreamOnly(a), StreamOnly(b)) => a == b,
            _ => false,
        }
    }
}
impl Value {
    pub fn string(value: &str) -> Self {
        Self::String(value.encode_utf16().collect())
    }
    /// Refuses a tree nested deeper than [`MAX_TREE_DEPTH`] before converting any of it.
    pub fn from_nbt(tag: &Tag, bool_keys: &[&str]) -> Result<Self, String> {
        if !sunrise_core::nbt::within_depth(tag, MAX_TREE_DEPTH) {
            return Err(format!("NBT nesting exceeds {MAX_TREE_DEPTH}"));
        }
        Ok(Self::convert_nbt(tag, bool_keys))
    }
    fn convert_nbt(tag: &Tag, bool_keys: &[&str]) -> Self {
        match tag {
            Tag::End => Self::Empty,
            Tag::Byte(v) => Self::Byte(*v),
            Tag::Short(v) => Self::Short(*v),
            Tag::Int(v) => Self::Int(*v),
            Tag::Long(v) => Self::Long(*v),
            Tag::Float(v) => Self::Float(*v),
            Tag::Double(v) => Self::Double(*v),
            Tag::String(v) => Self::String(v.0.clone()),
            Tag::ByteArray(v) => Self::ByteArray(v.clone()),
            Tag::IntArray(v) => Self::IntArray(v.clone()),
            Tag::LongArray(v) => Self::LongArray(v.clone()),
            Tag::List(v) => Self::List(v.iter().map(|v| Self::convert_nbt(v, bool_keys)).collect()),
            Tag::Compound(v) => Self::Map(
                v.iter()
                    .map(|(k, v)| {
                        let value = match v {
                            Tag::Byte(v)
                                if bool_keys
                                    .iter()
                                    .any(|n| k.0.iter().copied().eq(n.encode_utf16())) =>
                            {
                                Self::Bool(*v != 0)
                            }
                            _ => Self::convert_nbt(v, bool_keys),
                        };
                        (Self::String(k.0.clone()), value)
                    })
                    .collect(),
            ),
        }
    }
    /// A read-only encoding error leaves the value available to the caller.
    pub fn to_tag(&self) -> Result<Tag, String> {
        self.tag(false, 0)
    }
    /// Presentation may inspect inline stream holders without claiming they persist.
    pub fn presentation_tag(&self) -> Result<Tag, String> {
        self.tag(true, 0)
    }
    fn tag(&self, presentation: bool, depth: usize) -> Result<Tag, String> {
        if depth > MAX_TREE_DEPTH {
            return Err(format!("value nesting exceeds {MAX_TREE_DEPTH}"));
        }
        Ok(match self {
            Self::Empty => Tag::End,
            Self::Byte(v) => Tag::Byte(*v),
            Self::Short(v) => Tag::Short(*v),
            Self::Int(v) => Tag::Int(*v),
            Self::Long(v) => Tag::Long(*v),
            Self::Float(v) => Tag::Float(*v),
            Self::Double(v) => Tag::Double(*v),
            Self::Bool(v) => Tag::Byte(i8::from(*v)),
            Self::String(v) => Tag::String(JavaString(v.clone())),
            Self::ByteArray(v) => Tag::ByteArray(v.clone()),
            Self::IntArray(v) => Tag::IntArray(v.clone()),
            Self::LongArray(v) => Tag::LongArray(v.clone()),
            Self::List(v) | Self::Set(v) => Tag::List(
                v.iter()
                    .map(|v| v.tag(presentation, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            Self::Map(v) => Tag::Compound(
                v.iter()
                    .map(|(k, v)| {
                        let Self::String(k) = k else {
                            return Err("component map key is not a string".into());
                        };
                        Ok((JavaString(k.clone()), v.tag(presentation, depth + 1)?))
                    })
                    .collect::<Result<_, String>>()?,
            ),
            Self::Template(v) => v.tag(presentation, depth + 1)?,
            Self::Container(slots) => Tag::List(
                slots
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, item)| {
                        item.as_ref().map(|item| {
                            Ok(Tag::compound([
                                ("slot", Tag::Int(slot as i32)),
                                ("item", item.tag(presentation, depth + 1)?),
                            ]))
                        })
                    })
                    .collect::<Result<_, String>>()?,
            ),
            Self::StreamOnly(v) if presentation => v.tag(true, depth + 1)?,
            Self::StreamOnly(_) => return Err("inline holder has no persistent encoding".into()),
        })
    }
}
impl Template {
    pub fn to_tag(&self) -> Result<Tag, String> {
        self.tag(false, 0)
    }
    fn tag(&self, presentation: bool, depth: usize) -> Result<Tag, String> {
        if !presentation && !(1..=99).contains(&self.count) {
            return Err("persistent item template count must be in 1..99".into());
        }
        let mut fields = vec![("id".into(), Tag::String(self.item.as_str().into()))];
        if self.count != 1 {
            fields.push(("count".into(), Tag::Int(self.count)));
        }
        let mut patch = Vec::with_capacity(self.patch.len());
        for (name, value) in effective_patch(&self.patch) {
            if !presentation && !is_persistent(name) {
                continue;
            }
            patch.push(match value {
                Some(value) => {
                    let tag = value.tag(presentation, depth + 1)?;
                    if !presentation {
                        source::validate_persistent(name, &tag)?;
                    }
                    (name.as_str().into(), tag)
                }
                None => (
                    format!("!{name}").as_str().into(),
                    Tag::Compound(Default::default()),
                ),
            });
        }
        if !patch.is_empty() {
            fields.push((
                "components".into(),
                Tag::Compound(patch.into_iter().collect()),
            ));
        }
        Ok(Tag::Compound(fields.into_iter().collect()))
    }
}
/// The pinned component types that intentionally have no persistent codec.
pub fn is_persistent(name: &str) -> bool {
    !matches!(
        name.strip_prefix("minecraft:").unwrap_or(name),
        "map_post_processing" | "creative_slot_lock" | "additional_trade_cost"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_float_and_collection_identity() {
        assert_eq!(Value::Float(0x7fc00001), Value::Float(0x7fc00002));
        assert_ne!(Value::Float(0), Value::Float(0x80000000));
        assert_eq!(
            Value::Set(vec![Value::Int(1), Value::Int(2)]),
            Value::Set(vec![Value::Int(2), Value::Int(1)])
        );
        assert_ne!(
            Value::List(vec![Value::Int(1), Value::Int(2)]),
            Value::List(vec![Value::Int(2), Value::Int(1)])
        );
    }
    #[test]
    fn source_failure_does_not_destroy_stream_value() {
        let value = Value::StreamOnly(Box::new(Value::Int(3)));
        assert!(value.to_tag().is_err());
        assert_eq!(value.presentation_tag().unwrap(), Tag::Int(3));
    }
}
