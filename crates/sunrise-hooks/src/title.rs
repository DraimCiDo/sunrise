//! Title screen buttons (docs/layers.md, "Platform registries"): a crate above the engine adds a
//! full-width button to the title screen without an engine file naming it. Each shown button
//! takes a row of its own below Minecraft Realms, as NeoForge's Mods does; one shows only while
//! it says so, so with none shown the title screen is vanilla's.
use crate::TooLate;
use crate::registry::Registry;

/// What the title screen knows when it asks a button whether it shows.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Title {
    /// The packages in the mods folder, readable or not: the rows of the Mods screen's list.
    pub mods: usize,
}

impl Title {
    /// The title screen's facts: `mods` packages in the mods folder.
    pub fn new(mods: usize) -> Self {
        Self { mods }
    }
}

/// The engine screen a button opens.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opens {
    /// The Mods screen: the installed mods, as NeoForge lists them.
    Mods,
}

/// One button.
pub trait Button: Sync {
    /// Its label: a translation key.
    fn label(&self) -> &'static str;
    /// Whether the title screen shows it now. Asked each time the screen lays its buttons out.
    fn shown(&self, title: &Title) -> bool;
    /// The screen pressing it opens.
    fn opens(&self) -> Opens;
}

static BUTTONS: Registry<dyn Button> = Registry::new();

/// Adds `button` to the title screen. Before the game starts, as [`crate::register`]; the rows
/// keep the order buttons registered in.
pub fn register(button: &'static dyn Button) -> Result<(), TooLate> {
    BUTTONS.register(button)
}

/// The engine's side. Only the engine calls it.
pub mod engine {
    use super::*;

    /// Every registered button, sealing the list on the first call.
    pub fn all() -> &'static [&'static dyn Button] {
        BUTTONS.all()
    }
}
