//! The title screen's Mods button (`sunrise_hooks::title`): a row of its own below Minecraft
//! Realms, as NeoForge's, while the mods folder holds a mod. Without one the title screen is
//! vanilla's.
use sunrise_hooks::title::{Button, Opens, Title};

pub(crate) static MODS: Mods = Mods;

pub(crate) struct Mods;

impl Button for Mods {
    fn label(&self) -> &'static str {
        "sunrise.mods.screen.title"
    }
    fn shown(&self, title: &Title) -> bool {
        title.mods > 0
    }
    fn opens(&self) -> Opens {
        Opens::Mods
    }
}
