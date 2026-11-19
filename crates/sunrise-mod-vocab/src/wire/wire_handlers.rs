//! Explicit resource codecs for the API's erased handler traits.
//!
//! Handlers transfer through the current resource scope, never by copying their
//! concrete Rust state. Traits cannot derive Wire; this is their shared codec.
use crate::wire::{Reader, Wire, WireError, Writer};
use crate::*;
use std::sync::Arc;

/// The handler trait `T`, as the codec below sees it. The generator gives `Marks<dyn Trait>` a
/// `mark` that imports every method of an exported handler trait (the game may call any of them,
/// so a mod that sends one is refused at load when one changed); every other trait falls back to
/// [`NoMarks`]: a Part or `Hooks`, whose attribute marks the methods a mod writes, and a trait
/// the game's table does not export.
pub(crate) struct Marks<T: ?Sized>(std::marker::PhantomData<fn() -> Box<T>>);
impl<T: ?Sized> Marks<T> {
    pub(crate) fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}
/// What a handler trait without generated marks does when sent: nothing.
pub(crate) trait NoMarks {
    fn mark(&self) {}
}
impl<T: ?Sized> NoMarks for Marks<T> {}

/// Marks the handler trait `$handler` where one is sent. Native builds mark nothing.
macro_rules! mark_handler {
    ($handler:ty) => {{
        #[allow(unused_imports)]
        use $crate::wire::wire_handlers::NoMarks as _;
        $crate::wire::wire_handlers::Marks::<$handler>::new().mark();
    }};
}

macro_rules! arc_handler {
    ($handler:ty, $name:literal) => {
        impl Wire for Arc<$handler> {
            const MIN: usize = 8;
            fn holds_resource(_: &mut Vec<&'static str>) -> bool {
                true
            }
            fn put(&self, out: &mut Writer) {
                $crate::wire::wire_handlers::mark_handler!($handler);
                out.put_resource(Box::new(self.clone()));
            }
            fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                input.take_resource()
            }
        }
    };
}
macro_rules! handler {
    ($handler:ty, $name:literal) => {
        $crate::wire::wire_handlers::arc_handler!($handler, $name);
        impl Wire for Box<$handler> {
            const MIN: usize = 8;
            fn holds_resource(_: &mut Vec<&'static str>) -> bool {
                true
            }
            fn put(&self, out: &mut Writer) {
                out.fail(WireError::Invalid("boxed handler needs owned encoding"));
            }
            fn put_owned(self, out: &mut Writer) {
                $crate::wire::wire_handlers::mark_handler!($handler);
                out.put_resource(Box::new(Arc::<$handler>::from(self)));
            }
            fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
                input.take_resource()
            }
        }
    };
}
// New callback traits invoke this same resource codec at their declaration,
// rather than adding another entry to this legacy shared list.
pub(crate) use {arc_handler, handler, mark_handler};

handler!(dyn Hooks, "Hooks");
handler!(dyn ReloadApply, "ReloadApply");
handler!(dyn StructureType, "StructureType");
handler!(dyn BlockEntityTransform, "BlockEntityTransform");
handler!(dyn StructurePiece, "StructurePiece");
handler!(dyn BlockPart, "BlockPart");
handler!(dyn BlockEntityPart, "BlockEntityPart");
handler!(dyn ItemPart, "ItemPart");
handler!(dyn MenuPart, "MenuPart");
handler!(dyn FluidPart, "FluidPart");
handler!(dyn ServerReloadListener, "ServerReloadListener");
handler!(dyn LoadCondition, "LoadCondition");
handler!(dyn GameTestFunction, "GameTestFunction");
handler!(dyn ClientReloadListener, "ClientReloadListener");
handler!(dyn BiomeColorResolver, "BiomeColorResolver");
handler!(dyn BlockTintSource, "BlockTintSource");
handler!(dyn Interaction, "Interaction");
handler!(dyn SectionGeometry, "SectionGeometry");
handler!(dyn ParticleProvider, "ParticleProvider");
handler!(dyn EnchantmentDamageEffect, "EnchantmentDamageEffect");
handler!(dyn CommandArgumentType, "CommandArgumentType");
handler!(dyn GroundStateProvider, "GroundStateProvider");
handler!(dyn PlaceProvider, "PlaceProvider");
handler!(dyn TooltipAppender, "TooltipAppender");
handler!(dyn CriterionTest, "CriterionTest");
handler!(
    dyn ResourceHandler<ItemResource>,
    "ResourceHandler<ItemResource>"
);
handler!(
    dyn ResourceHandler<FluidResource>,
    "ResourceHandler<FluidResource>"
);
handler!(dyn EnergyHandler, "EnergyHandler");
handler!(dyn ItemStorage, "ItemStorage");
handler!(
    dyn BlockCapabilityProvider<dyn ResourceHandler<ItemResource>>,
    "BlockCapabilityProvider<dyn ResourceHandler<ItemResource>>"
);
handler!(
    dyn BlockCapabilityProvider<dyn ResourceHandler<FluidResource>>,
    "BlockCapabilityProvider<dyn ResourceHandler<FluidResource>>"
);
handler!(
    dyn BlockCapabilityProvider<dyn EnergyHandler>,
    "BlockCapabilityProvider<dyn EnergyHandler>"
);
handler!(
    dyn BlockCapabilityProvider<dyn ItemStorage>,
    "BlockCapabilityProvider<dyn ItemStorage>"
);
handler!(
    dyn ItemCapabilityProvider<dyn ResourceHandler<ItemResource>>,
    "ItemCapabilityProvider<dyn ResourceHandler<ItemResource>>"
);
handler!(
    dyn ItemCapabilityProvider<dyn ResourceHandler<FluidResource>>,
    "ItemCapabilityProvider<dyn ResourceHandler<FluidResource>>"
);
handler!(
    dyn ItemCapabilityProvider<dyn EnergyHandler>,
    "ItemCapabilityProvider<dyn EnergyHandler>"
);
handler!(
    dyn EntityCapabilityProvider<dyn ResourceHandler<ItemResource>, ()>,
    "EntityCapabilityProvider<dyn ResourceHandler<ItemResource>, ()>"
);
handler!(
    dyn EntityCapabilityProvider<dyn ResourceHandler<ItemResource>, Option<Direction>>,
    "EntityCapabilityProvider<dyn ResourceHandler<ItemResource>, Option<Direction>>"
);
handler!(
    dyn EntityCapabilityProvider<dyn ResourceHandler<FluidResource>, Option<Direction>>,
    "EntityCapabilityProvider<dyn ResourceHandler<FluidResource>, Option<Direction>>"
);
handler!(
    dyn EntityCapabilityProvider<dyn EnergyHandler, Option<Direction>>,
    "EntityCapabilityProvider<dyn EnergyHandler, Option<Direction>>"
);
arc_handler!(dyn std::any::Any + Send + Sync, "Any");
