//! Triggered keywords whose creation instruction supplies copiable token characteristics.
use super::*;
use crate::token::{
    TokenCreation, TokenDefinition, TokenDefinitionError, TokenName, TokenRecipients, TokenSubtype,
};

/// The 2/2 black Zombie created by cards including Falcon Abomination (CR 702.147).
pub fn decayed_zombie(
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TokenDefinition, TokenDefinitionError> {
    let zombie = vocabulary
        .resolve_label("Zombie")
        .ok_or(TokenDefinitionError::ForeignCreatureType)?;
    let mut program = Program::creature(None, 2.into(), 2.into(), BTreeSet::from([zombie]));
    program.effect_defined_colors = Some(BTreeSet::from([Color::Black]));
    program.printed_keywords.insert(KeywordAbility::Decayed);
    program.triggered.push(TriggeredAbility::combat_keyword(
        TriggeredAbilityId(0),
        CombatTriggerKeyword::Decayed,
    ));
    TokenDefinition::new(
        program,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(zombie)],
        },
        vocabulary,
    )
}
/// The 1/1 red Warrior created by a mobilize trigger (CR 702.181).
pub(crate) fn mobilize_warrior(
    vocabulary: &CreatureTypeVocabulary,
) -> Result<TokenDefinition, TokenDefinitionError> {
    let warrior = vocabulary
        .resolve_label("Warrior")
        .ok_or(TokenDefinitionError::ForeignCreatureType)?;
    let mut program = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([warrior]));
    program.effect_defined_colors = Some(BTreeSet::from([Color::Red]));
    TokenDefinition::new(
        program,
        TokenName::Default {
            ordered_subtypes: vec![TokenSubtype::Creature(warrior)],
        },
        vocabulary,
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKeyword {
    /// CR 702.135.
    Afterlife,
    /// CR 702.123.
    Fabricate,
}

impl TokenKeyword {
    /// Build one independently stacking and source-bound printed keyword definition.
    ///
    /// # Errors
    /// The amount must be nonnegative and the game's creature vocabulary must contain
    /// the token subtype used by this keyword.
    pub fn ability(
        self,
        id: TriggeredAbilityId,
        amount: RulesInteger,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Result<TriggeredAbility, TokenDefinitionError> {
        if amount < 0.into() {
            return Err(TokenDefinitionError::Definition(
                DefinitionError::InvalidInstruction,
            ));
        }
        let (subtype, condition) = match self {
            Self::Afterlife => (
                "Spirit",
                TriggerSpec::new(TriggerEvent::Dies {
                    subject: ObjectFilter::self_object(),
                    mode: DiesTriggerMode::Permanent,
                }),
            ),
            Self::Fabricate => ("Servo", TriggerSpec::self_enters()),
        };
        let subtype = vocabulary
            .resolve_label(subtype)
            .ok_or(TokenDefinitionError::ForeignCreatureType)?;
        let mut program = Program::creature(None, 1.into(), 1.into(), BTreeSet::from([subtype]));
        let mut instructions = Vec::new();
        match self {
            Self::Afterlife => {
                program.effect_defined_colors = Some(BTreeSet::from([Color::White, Color::Black]));
                program.printed_keywords.insert(KeywordAbility::Flying);
            }
            Self::Fabricate => {
                program.effect_defined_colors = Some(BTreeSet::new());
                program.card_types.insert(CardType::Artifact);
                instructions.push(Instruction::MutatePlayer(Box::new(
                    crate::toolbox::MutatePlayerSpec::offer_source_counters(amount.clone(), 2, 1),
                )));
            }
        }
        let definition = TokenDefinition::new(
            program,
            TokenName::Default {
                ordered_subtypes: vec![TokenSubtype::Creature(subtype)],
            },
            vocabulary,
        )?;
        instructions.push(Instruction::CreateTokens {
            creations: vec![TokenCreation {
                definition,
                amount: crate::program::ValueExpr::Literal(amount),
                base_size: None,
                enters_tapped: false,
                recipients: TokenRecipients::Controller,
                attacking_defender: None,
            }],
        });
        Ok(TriggeredAbility {
            id,
            condition,
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, instructions),
        })
    }

    /// Recognize a currently available printed definition by its complete native semantics.
    pub fn from_ability(
        ability: &TriggeredAbility,
        vocabulary: &CreatureTypeVocabulary,
    ) -> Option<Self> {
        let keyword = match ability.condition.event {
            TriggerEvent::Dies {
                subject,
                mode: DiesTriggerMode::Permanent,
            } if subject == ObjectFilter::self_object() => Self::Afterlife,
            TriggerEvent::Enters {
                subject,
                from_zone: TriggerFromZone::Any,
                cast_origin: None,
                attached_to_creature: false,
            } if subject == ObjectFilter::self_object()
                && ability.condition.intervening_if.is_none()
                && matches!(
                    ability.body.instructions.first(),
                    Some(Instruction::MutatePlayer(spec))
                        if matches!(
                            spec.op,
                            crate::toolbox::PlayerMutation::OfferSourceCounters { .. }
                        )
                ) =>
            {
                Self::Fabricate
            }
            _ => return None,
        };
        let Instruction::CreateTokens { creations } = ability.body.instructions.last()? else {
            return None;
        };
        let [
            TokenCreation {
                amount: crate::program::ValueExpr::Literal(amount),
                ..
            },
        ] = creations.as_slice()
        else {
            return None;
        };
        (keyword
            .ability(ability.id, amount.clone(), vocabulary)
            .ok()
            .as_ref()
            == Some(ability))
        .then_some(keyword)
    }
}
