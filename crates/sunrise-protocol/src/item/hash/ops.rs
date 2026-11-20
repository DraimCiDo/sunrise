//! Operations for the single component-schema traversal: hashes or actual values.
use super::{Names, Stack};
use crate::hash_ops::{HashCode, HashOps};
use sunrise_core::nbt::Tag;
use sunrise_world::component_value::Value;

pub(crate) trait Ops: Sized {
    type Value: Clone;
    type Container;
    const STREAM_VALUES: bool = false;
    fn empty_map() -> Self::Value;
    fn create_byte(v: i8) -> Self::Value;
    fn create_int(v: i32) -> Self::Value;
    fn create_f32(v: f32) -> Self::Value;
    fn create_double(v: u64) -> Self::Value;
    fn create_boolean(v: bool) -> Self::Value;
    fn create_string(v: &str) -> Self::Value;
    fn create_string_units(v: &[u16]) -> Self::Value;
    fn create_int_list(v: &[i32]) -> Self::Value;
    fn create_map(v: Vec<(Self::Value, Self::Value)>) -> Self::Value;
    fn create_list(v: impl IntoIterator<Item = Self::Value>) -> Self::Value;
    /// `None` when the tree is nested deeper than a conversion may recurse.
    fn from_nbt(v: &Tag, bools: &[&str]) -> Option<Self::Value>;
    fn template(stack: &Stack, names: &dyn Names) -> Option<Self::Value>;
    fn special_component(
        _: i32,
        _: &super::Component,
        _: &dyn Names,
    ) -> Option<Option<Self::Value>> {
        None
    }
    fn stream_only(_: Self::Value) -> Option<Self::Value> {
        None
    }
    fn set(v: Vec<Self::Value>) -> Self::Value {
        Self::create_list(v)
    }
    fn container_new(size: usize) -> Self::Container;
    fn container_slot(
        slots: &mut Self::Container,
        slot: usize,
        item: Option<Self::Value>,
    ) -> Option<()>;
    fn container_build(slots: Self::Container) -> Self::Value;
}
impl Ops for HashOps {
    type Value = HashCode;
    type Container = Vec<HashCode>;
    fn empty_map() -> HashCode {
        HashOps::empty_map()
    }
    fn create_byte(v: i8) -> HashCode {
        HashOps::create_byte(v)
    }
    fn create_int(v: i32) -> HashCode {
        HashOps::create_int(v)
    }
    fn create_f32(v: f32) -> HashCode {
        HashOps::create_f32(v)
    }
    fn create_double(v: u64) -> HashCode {
        HashOps::create_double(v)
    }
    fn create_boolean(v: bool) -> HashCode {
        HashOps::create_boolean(v)
    }
    fn create_string(v: &str) -> HashCode {
        HashOps::create_string(v)
    }
    fn create_string_units(v: &[u16]) -> HashCode {
        HashOps::create_string_units(v)
    }
    fn create_int_list(v: &[i32]) -> HashCode {
        HashOps::create_int_list(v)
    }
    fn create_map(v: Vec<(HashCode, HashCode)>) -> HashCode {
        HashOps::create_map(v)
    }
    fn create_list(v: impl IntoIterator<Item = HashCode>) -> HashCode {
        HashOps::create_list(v)
    }
    fn from_nbt(v: &Tag, bools: &[&str]) -> Option<HashCode> {
        Some(HashOps::from_nbt(v, bools))
    }
    fn template(stack: &Stack, names: &dyn Names) -> Option<HashCode> {
        super::template_record_with::<Self>(stack, names)
    }
    fn container_new(_: usize) -> Self::Container {
        Vec::new()
    }
    fn container_slot(
        slots: &mut Self::Container,
        slot: usize,
        item: Option<HashCode>,
    ) -> Option<()> {
        if let Some(item) = item {
            slots.push(
                Record::<Self>::new()
                    .field("slot", Self::create_int(slot as i32))
                    .field("item", item)
                    .build(),
            );
        }
        Some(())
    }
    fn container_build(slots: Self::Container) -> HashCode {
        Self::create_list(slots)
    }
}
pub(crate) struct Values;
impl Ops for Values {
    const STREAM_VALUES: bool = true;
    type Value = Value;
    type Container = Vec<Option<sunrise_world::component_value::Template>>;
    fn empty_map() -> Value {
        Value::Map(Vec::new())
    }
    fn create_byte(v: i8) -> Value {
        Value::Byte(v)
    }
    fn create_int(v: i32) -> Value {
        Value::Int(v)
    }
    fn create_f32(v: f32) -> Value {
        Value::Float(v.to_bits())
    }
    fn create_double(v: u64) -> Value {
        Value::Double(v)
    }
    fn create_boolean(v: bool) -> Value {
        Value::Bool(v)
    }
    fn create_string(v: &str) -> Value {
        Value::string(v)
    }
    fn create_string_units(v: &[u16]) -> Value {
        Value::String(v.to_vec())
    }
    fn create_int_list(v: &[i32]) -> Value {
        Value::IntArray(v.to_vec())
    }
    fn create_map(v: Vec<(Value, Value)>) -> Value {
        let mut out: Vec<(Value, Value)> = Vec::with_capacity(v.len());
        for (key, value) in v {
            if let Some((_, old)) = out.iter_mut().find(|(k, _)| *k == key) {
                *old = value;
            } else {
                out.push((key, value));
            }
        }
        Value::Map(out)
    }
    fn create_list(v: impl IntoIterator<Item = Value>) -> Value {
        Value::List(v.into_iter().collect())
    }
    fn from_nbt(v: &Tag, bools: &[&str]) -> Option<Value> {
        Value::from_nbt(v, bools).ok()
    }
    fn template(stack: &Stack, names: &dyn Names) -> Option<Value> {
        crate::item::persistent::template_value(stack, names)
            .ok()
            .map(|v| Value::Template(Box::new(v)))
    }
    fn special_component(
        kind: i32,
        value: &super::Component,
        names: &dyn Names,
    ) -> Option<Option<Value>> {
        crate::item::persistent::special_value(kind, value, names)
    }
    fn set(v: Vec<Value>) -> Value {
        Value::Set(v)
    }
    fn stream_only(v: Value) -> Option<Value> {
        Some(Value::StreamOnly(Box::new(v)))
    }
    fn container_new(size: usize) -> Self::Container {
        Vec::with_capacity(size)
    }
    fn container_slot(slots: &mut Self::Container, _: usize, item: Option<Value>) -> Option<()> {
        slots.push(match item {
            Some(Value::Template(item)) => Some(*item),
            None => None,
            Some(_) => return None,
        });
        Some(())
    }
    fn container_build(slots: Self::Container) -> Value {
        Value::Container(slots)
    }
}
pub(crate) struct Record<O: Ops>(Vec<(O::Value, O::Value)>);
impl<O: Ops> Record<O> {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }
    pub(crate) fn field(mut self, name: &str, value: O::Value) -> Self {
        self.0.push((O::create_string(name), value));
        self
    }
    pub(crate) fn optional(self, name: &str, value: Option<O::Value>) -> Self {
        match value {
            Some(v) => self.field(name, v),
            None => self,
        }
    }
    pub(crate) fn build(self) -> O::Value {
        O::create_map(self.0)
    }
}
