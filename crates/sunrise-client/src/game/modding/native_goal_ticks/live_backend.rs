//! Determines which entity reads can use the ordinary game backend while a
//! mob tick has its active and other mobs on loan.
use super::{GoalCx, Mob, Resolved};
use mod_api::{EntityRef, EntityTypeKey, EntityView};

// no wire: a read scoped to the mob tick's live and observed entities.
struct GoalEntityView<'a> {
    position: Option<[f64; 3]>,
    mob: Option<&'a Mob>,
}

impl EntityView for GoalEntityView<'_> {
    fn position(&self) -> Option<[f64; 3]> {
        self.position
    }

    fn rotation(&self) -> Option<[f32; 2]> {
        let mob = self.mob?;
        Some([mob.yaw, mob.head_pitch])
    }

    fn kind(&self) -> Option<EntityTypeKey> {
        mod_api::__private::resolved_entity_type_key(&self.mob?.kind.id)
    }
}

impl GoalCx<'_, '_> {
    pub(super) fn with_entity_view(
        &self,
        entity: EntityRef,
        read: &mut dyn FnMut(&dyn EntityView),
    ) {
        let tick_matches = self.w.goal_host.and_then(|host| host.epoch()) == Some(entity.epoch());
        let resolved = tick_matches
            .then(|| self.resolve_tick_row(entity.id()))
            .flatten();
        match resolved {
            Some(Resolved::Unavailable) => return,
            Some(Resolved::Own | Resolved::Live(_) | Resolved::Seen(_)) => {}
            // Player and dragon tick views yield to the Game's data, as do ordinary
            // entities and parked mobs. The snapshot-only pass hides active-level mobs
            // absent from its snapshot, even when Game now has them (a buffered spawn).
            _ => {
                if let Some(view) = self
                    .game
                    .resolve_entity_view(entity)
                    .filter(|view| view.is_current() || (tick_matches && view.is_parked_mob()))
                {
                    if !(tick_matches
                        && self.w.live_mobs.is_none()
                        && resolved.is_none()
                        && view.is_level_mob())
                    {
                        read(&view);
                    }
                    return;
                }
            }
        }
        if let Some(resolved) = resolved {
            let position = self.resolved_position(&resolved).map(|p| [p.x, p.y, p.z]);
            let mob = match resolved {
                Resolved::Own => Some(&*self.m),
                Resolved::Live(mob) => Some(mob),
                // The snapshot carries species, not the real type key or pitch.
                _ => None,
            };
            read(&GoalEntityView { position, mob });
        }
    }

    pub(super) fn is_game_entity(&self, entity: EntityRef) -> bool {
        if matches!(
            self.resolve(entity),
            Some(Resolved::Own | Resolved::Live(_) | Resolved::Seen(_) | Resolved::Unavailable)
        ) {
            return false;
        }
        self.game_host().entity(entity.id()) == Some(entity)
            || self
                .w
                .goal_host
                .is_some_and(|host| host.mob_in_other_level(entity))
    }
}
