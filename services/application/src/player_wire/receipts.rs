//! Original-intent correlation. A receipt is historical, not today's board basis.
use super::*;
use crate::commands::CommandDisposition;

/// Committed successor of exactly one original intent, not a current snapshot.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerCommandAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub actor_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
}
impl PlayerCommandAck {
    /// # Errors
    /// Rejects substituted request/actor/context, overflow and non-successor revision.
    pub fn validate(&self, original: &PlayerCommand, actor: Uuid) -> Result<(), InvalidPlayerWire> {
        original.validate()?;
        if actor.is_nil()
            || self.actor_seat_id != actor
            || self.request_id != original.request_id
            || self.basis.game_id != original.basis.game_id
            || self.basis.context_id != original.basis.context_id
            || self.basis.epoch != original.basis.epoch
            || original.basis.revision.get().checked_add(1) != Some(self.basis.revision.get())
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
}

/// Exact original intent plus the existing durable disposition protocol.
/// The trusted receipt reader supplies actor identity and original intent separately.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerReceipt {
    pub actor_seat_id: Uuid,
    pub original: PlayerCommand,
    pub disposition: CommandDisposition<PlayerCommandAck>,
}
impl<'de> Deserialize<'de> for PlayerReceipt {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            actor_seat_id: Uuid,
            #[serde(deserialize_with = "crate::provider_json::object")]
            original: PlayerCommand,
            #[serde(deserialize_with = "crate::provider_json::object")]
            disposition: CommandDisposition<PlayerCommandAck>,
        }
        let wire: Wire = crate::provider_json::object(input)?;
        Ok(Self {
            actor_seat_id: wire.actor_seat_id,
            original: wire.original,
            disposition: wire.disposition,
        })
    }
}
impl PlayerReceipt {
    /// # Errors
    /// Rejects foreign intents, fabricated successors and unsafe rejection diagnostics.
    /// Does not authenticate the caller or prove persistence of a receipt.
    pub fn validate(&self, original: &PlayerCommand, actor: Uuid) -> Result<(), InvalidPlayerWire> {
        original.validate()?;
        if actor.is_nil() || self.actor_seat_id != actor || &self.original != original {
            return Err(InvalidPlayerWire);
        }
        match &self.disposition {
            CommandDisposition::Accepted { ack } => ack.validate(original, actor),
            CommandDisposition::Rejected { errors } => {
                if errors.is_empty()
                    || !errors.iter().all(|(field, reason)| {
                        matches!(
                            field.as_str(),
                            "request"
                                | "game"
                                | "basis"
                                | "actor"
                                | "source"
                                | "ability"
                                | "target"
                                | "decision"
                                | "response"
                                | "payment"
                                | "action"
                        ) && matches!(
                            reason.as_str(),
                            "invalid" | "stale" | "unavailable" | "not permitted" | "unsupported"
                        )
                    })
                {
                    return Err(InvalidPlayerWire);
                }
                Ok(())
            }
            CommandDisposition::Unknown | CommandDisposition::Pending => Ok(()),
        }
    }
}

/// Bounded original intents for a read-only reconnect receipt lookup.
/// No command is executed or resubmitted by this record.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerReceiptQuery {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub outstanding: Vec<PlayerCommand>,
}
impl PlayerReceiptQuery {
    /// # Errors
    /// Rejects duplicate requests, foreign games/contexts and over100 outstanding intents.
    /// Historical epochs need not equal today's native epoch.
    pub fn validate(&self) -> Result<(), InvalidPlayerWire> {
        if self.game_id.is_nil()
            || self.context_id.is_nil()
            || self.outstanding.len() > 100
            || !super::actions::unique(self.outstanding.iter().map(|x| x.request_id))
            || self.outstanding.iter().any(|x| {
                x.validate().is_err()
                    || x.basis.game_id != self.game_id
                    || x.basis.context_id != self.context_id
            })
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
    /// Exactly one correlated result per submitted intent, in original order.
    /// # Errors
    /// Rejects missing, duplicate, reordered or substituted results.
    pub fn validate_receipts(
        &self,
        receipts: &[PlayerReceipt],
        actor: Uuid,
    ) -> Result<(), InvalidPlayerWire> {
        self.validate()?;
        if actor.is_nil() || receipts.len() != self.outstanding.len() {
            return Err(InvalidPlayerWire);
        }
        for (receipt, original) in receipts.iter().zip(&self.outstanding) {
            receipt.validate(original, actor)?;
        }
        Ok(())
    }
}
