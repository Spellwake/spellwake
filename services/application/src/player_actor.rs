//! Shared strict canonical player identity; metadata is never possession proof.
use crate::{
    amount::Amount, controller_possession::PossessedController, match_history::ControllerKind,
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlayerActor {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub(crate) seat_id: Uuid,
    pub(crate) kind: ControllerKind,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub(crate) account_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub(crate) grant_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub(crate) binding_id: Option<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub(crate) binding_epoch: Option<Amount>,
}
impl PlayerActor {
    pub(crate) fn matches_controller(&self, proof: &PossessedController) -> bool {
        self.kind == ControllerKind::Ai
            && self.account_id.is_none()
            && self.grant_id == Some(proof.grant())
            && self.binding_id == Some(proof.binding())
            && self.binding_epoch.as_ref() == Some(proof.epoch())
    }
}
