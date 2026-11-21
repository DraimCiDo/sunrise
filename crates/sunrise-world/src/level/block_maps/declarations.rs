impl BlockMaps {
    /// The maps from their merged values (block id to the value a file gave
    /// it). A value of another shape is skipped with a warning, as a codec
    /// error leaves the entry out. The inverse maps are derived here, never
    /// declared: where several blocks share a next stage or a waxed twin, the
    /// one with the lowest `rank` (its block registry id) is the one scraping
    /// or unwaxing gives back, a Sunrise choice where NeoForge keeps whichever
    /// its map iteration met last. An oxidation chain that loops (a block its
    /// own next stage included) is refused with an error naming the map and the
    /// blocks: its scraping steps are left out, so no walk back can hang.
    pub fn from_values<'a>(
        flammable: impl IntoIterator<Item = (&'a str, &'a Value)>,
        oxidizables: impl IntoIterator<Item = (&'a str, &'a Value)>,
        waxables: impl IntoIterator<Item = (&'a str, &'a Value)>,
        rank: impl Fn(&str) -> u32,
    ) -> Self {
        let mut maps = Self::default();
        let int = |v: &Value| v.as_i64().and_then(|n| i32::try_from(n).ok());
        for (block, value) in flammable {
            match (int(&value["ignite_odds"]), int(&value["burn_odds"])) {
                (Some(ignite), Some(burn)) => {
                    maps.flammable.insert(block.to_owned(), (ignite, burn));
                }
                _ => log::warn!("Sunrise: neoforge:flammable {block}: not a Flammable, skipped"),
            }
        }
        for (block, value) in oxidizables {
            match value["next_oxidation_stage"].as_str() {
                Some(next) => {
                    inverse(&mut maps.scrapes_to, full(next), block, &rank);
                    maps.oxidizes_to.insert(block.to_owned(), full(next));
                }
                None => {
                    log::warn!("Sunrise: neoforge:oxidizables {block}: not an Oxidizable, skipped")
                }
            }
        }
        for (block, value) in waxables {
            match value["waxed"].as_str() {
                Some(waxed) => {
                    inverse(&mut maps.unwaxes_to, full(waxed), block, &rank);
                    maps.waxes_to.insert(block.to_owned(), full(waxed));
                }
                None => log::warn!("Sunrise: neoforge:waxables {block}: not a Waxable, skipped"),
            }
        }
        for cycle in cycles(&maps.scrapes_to) {
            log::error!(
                "Sunrise: neoforge:oxidizables: the oxidation chain loops through {}; scraping is left out for those blocks",
                cycle.join(" -> ")
            );
            for block in &cycle {
                maps.scrapes_to.remove(block);
            }
        }
        maps
    }
}
