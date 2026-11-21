use super::*;

impl Game<'_> {
    /// Collect the block-state definition types each mod binds, before client resources load.
    pub(in crate::game) fn native_register_block_state_models(
        context: &mod_api::RegistrationContext,
        definitions: &mut Vec<mod_api::Answered<String>>,
    ) {
        Self::fire_register_block_state_models(context, definitions);
    }
}
