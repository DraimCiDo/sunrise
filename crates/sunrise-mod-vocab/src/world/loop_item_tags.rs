//! Bound, immutable item members for one published resource generation.
use crate::{ItemKey, TagKey};
use std::collections::{HashMap, HashSet};

// no wire: engine-owned lookup borrowed by an in-process loop context.
/// Item tags resolved against the owning generation and the completed API item table.
#[doc(hidden)]
// Keep tag names generation-owned: arbitrary server tags are not process-lifetime TagKeys.
pub struct BoundItemTags(HashMap<String, Members>);

struct Members {
    ordered: Box<[ItemKey]>,
    membership: HashSet<ItemKey>,
}

impl BoundItemTags {
    /// Bind an authoritative tag table at publication. `None` means the item keys
    /// have not been published; an empty input still yields an available table.
    pub fn bind(tags: &HashMap<String, Vec<String>>, registry: &HashSet<&str>) -> Option<Self> {
        let items = crate::content::registry_keys::resolved_items().get()?;
        let tags = tags
            .iter()
            .map(|(name, members)| {
                let ordered: Box<[_]> = members
                    .iter()
                    .filter(|name| registry.contains(name.as_str()))
                    .filter_map(|name| items.slots.get(name.as_str()).copied().map(ItemKey))
                    .collect();
                let membership = ordered.iter().copied().collect();
                (
                    name.clone(),
                    Members {
                        ordered,
                        membership,
                    },
                )
            })
            .collect();
        Some(Self(tags))
    }

    pub(super) fn contains(&self, tag: TagKey, item: ItemKey) -> Option<bool> {
        Some(
            self.members(tag)?
                .is_some_and(|members| members.membership.contains(&item)),
        )
    }

    pub(super) fn items(&self, tag: TagKey) -> Option<&[ItemKey]> {
        Some(
            self.members(tag)?
                .map_or(&[], |members| members.ordered.as_ref()),
        )
    }

    // The outer option refuses an invalid key; a valid key need not name an existing tag.
    fn members(&self, tag: TagKey) -> Option<Option<&Members>> {
        crate::world::tags::with_tag_name(tag, |name| Some(self.0.get(name?)))
    }
}
