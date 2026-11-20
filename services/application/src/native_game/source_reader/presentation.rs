//! Current appearance observation, separate from immutable native state and late Auth.
use super::*;
use crate::{actor::RequestActor, cosmetics::AppearanceSelection, player_wire::PlayerPresentation};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    access: access::Record,
    #[serde(deserialize_with = "crate::provider_json::object")]
    presentation: PlayerPresentation,
}
/// Exact declared SQL reader outcomes. Presentation unavailability is not a missing player.
pub enum NativePresentationOutcome {
    Observed(NativeHumanPresentation),
    Unauthorized,
    GameUnavailable,
    PresentationUnavailable,
}
/// Sealed current SQL observation, not a permission lease or public source proof.
pub struct NativeHumanPresentation {
    access: NativeHumanAccess,
    presentation: PlayerPresentation,
}
impl NativeHumanPresentation {
    /// Resolved public cosmetic IDs only; the SQL source owns entitlement decisions.
    pub fn presentation(&self) -> &PlayerPresentation {
        &self.presentation
    }
    /// Compare this coherent observation's complete access cut before classifying a stale delivery.
    pub(crate) fn matches_access(&self, access: &NativeHumanAccess) -> bool {
        self.access.same_delivery_source(access)
    }
    /// Verify complete source/disclosure identity and exact independent native roster.
    /// # Errors
    /// Rejects a changed access observation, foreign/reordered/duplicate roster or bad IDs.
    pub fn validate_against(
        &self,
        access: &NativeHumanAccess,
        roster: &[Uuid],
    ) -> Result<(), NativeSourceError> {
        if !self.access.same_delivery_source(access) {
            return Err(invalid());
        }
        self.presentation
            .validate(roster, &self.presentation)
            .map_err(|_| invalid())
    }
}
impl NativeSourceRepository {
    /// Observe the canonical current deck headers and owner's grants in SQL239.
    /// `free` is trusted startup configuration, not a public caller selection.
    /// Separate late original-bearer and source renewal is required after this IO.
    /// # Errors
    /// Rejects nil scope/configuration, malformed/duplicate/foreign source records,
    /// invalid appearance identity/roster bounds and backend failures.
    pub fn human_presentation(
        &self,
        actor: &RequestActor,
        game: Uuid,
        context: Uuid,
        free: &AppearanceSelection,
    ) -> Result<NativePresentationOutcome, NativeSourceError> {
        let Some(account) = actor.account() else {
            return Ok(NativePresentationOutcome::GameUnavailable);
        };
        self.presentation_identity(
            actor.subject(),
            actor.session(),
            account,
            game,
            context,
            free,
        )
    }
    fn presentation_identity(
        &self,
        subject: Uuid,
        session: Uuid,
        account: Uuid,
        game: Uuid,
        context: Uuid,
        free: &AppearanceSelection,
    ) -> Result<NativePresentationOutcome, NativeSourceError> {
        if [subject, session, account, game, context]
            .iter()
            .any(Uuid::is_nil)
            || free.validate().is_err()
            || free.playmat_id.is_some()
        {
            return Err(invalid());
        }
        let response:ObjectResponse<Record>=self.backend.rpc("read_native_human_presentation",
            &json!({"p_subject":subject,"p_session":session,"p_game":game,"p_context":context,"p_free":free}))
            .map_err(|e|NativeSourceError(Failure::Backend(e)))?;
        match response.0 {
            ApiResponse::Success { data } => {
                if data.version != "native-presentation-v1"
                    || !(2..=6).contains(&data.presentation.seats.len())
                {
                    return Err(invalid());
                }
                let roster: Vec<_> = data.presentation.seats.iter().map(|s| s.seat_id).collect();
                data.presentation
                    .validate(&roster, &data.presentation)
                    .map_err(|_| invalid())?;
                let access = data.access.into_access(account, game, context)?;
                if !roster.contains(&access.viewer_seat_id()) {
                    return Err(invalid());
                }
                Ok(NativePresentationOutcome::Observed(
                    NativeHumanPresentation {
                        access,
                        presentation: data.presentation,
                    },
                ))
            }
            ApiResponse::Fail { data } if data.len() == 1 => {
                if data.get("session").is_some_and(|v| v == "unauthorized") {
                    Ok(NativePresentationOutcome::Unauthorized)
                } else if data.get("game").is_some_and(|v| v == "unavailable") {
                    Ok(NativePresentationOutcome::GameUnavailable)
                } else if data.get("presentation").is_some_and(|v| v == "unavailable") {
                    Ok(NativePresentationOutcome::PresentationUnavailable)
                } else {
                    Err(invalid())
                }
            }
            _ => Err(invalid()),
        }
    }
}
#[cfg(test)]
mod tests;
