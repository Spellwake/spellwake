//! Targeted and conditional +1/+1 counter triggers.
use super::*;
/// Copiable definitions for the supported mentor, evolve and backup abilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrowthKeyword {
    Mentor,
    Evolve,
    Increment,
    /// Static keyword abilities printed below this backup instance, in Oracle text.
    Backup {
        amount: RulesInteger,
        printed_below: BTreeSet<KeywordAbility>,
    },
}
impl GrowthKeyword {
    pub(crate) fn condition(&self) -> super::TriggerSpec {
        match self {
            Self::Mentor => super::TriggerSpec::self_attacks(),
            Self::Evolve => super::TriggerSpec::new(super::TriggerEvent::Keyword(
                super::TriggerKeywordEvent::Evolve,
            )),
            Self::Increment => super::TriggerSpec::new(super::TriggerEvent::Keyword(
                super::TriggerKeywordEvent::Increment,
            )),
            Self::Backup { .. } => super::TriggerSpec::self_enters(),
        }
    }
    pub(crate) fn target(&self) -> BodyTarget {
        match self {
            Self::Increment | Self::Evolve => BodyTarget::None,
            _ => BodyTarget::SingleCreature,
        }
    }
    /// Construct one independently stacked CR 702.100, 702.134 or 702.165 trigger.
    pub fn ability(self, id: TriggeredAbilityId) -> TriggeredAbility {
        TriggeredAbility {
            id,
            condition: self.condition(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(self.target(), vec![Instruction::ResolveGrowth(self)]),
        }
    }
}
