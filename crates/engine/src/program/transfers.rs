//! Printed static and triggered portions of modular, graft, and soulshift.
use super::*;

/// Numeric keyword abilities that transfer counters or a graveyard card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKeyword {
    Modular(u32),
    Graft(u32),
    Soulshift(u32),
}
impl TransferKeyword {
    pub(crate) fn condition(self) -> super::TriggerSpec {
        match self {
            Self::Graft(_) => super::TriggerSpec::new(super::TriggerEvent::Keyword(
                super::TriggerKeywordEvent::Graft,
            )),
            _ => super::TriggerSpec::new(super::TriggerEvent::Dies {
                subject: super::ObjectFilter::self_object(),
                mode: super::DiesTriggerMode::Permanent,
            }),
        }
    }
    pub(crate) fn target(self) -> BodyTarget {
        match self {
            Self::Graft(_) => BodyTarget::None,
            Self::Modular(_) => BodyTarget::SingleCreature,
            Self::Soulshift(_) => BodyTarget::SingleOwnGraveyardCard,
        }
    }
    /// The static as-enters half of a printed graft or modular instance.
    pub fn entry(self) -> Option<EntryCounterAbility> {
        match self {
            Self::Graft(n) | Self::Modular(n) => {
                Some(EntryCounterAbility::FixedPlusOne(i64::from(n).into()))
            }
            Self::Soulshift(_) => None,
        }
    }
    /// One separately stacked optional ability with its target fixed at placement.
    pub fn ability(self, id: TriggeredAbilityId) -> TriggeredAbility {
        TriggeredAbility {
            id,
            condition: self.condition(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                self.target(),
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    Instruction::ResolveTransfer(self),
                ],
            ),
        }
    }
}
