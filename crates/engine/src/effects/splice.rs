//! Copy splice text only onto the spell on the stack (CR 702.47).
use super::*;
use crate::mana::SpliceChoice;
use crate::program::SpliceQuality;

pub(super) struct SplicedText {
    pub card: GameObject,
    pub body: ExecutableBody,
    pub target: Option<TargetRef>,
    pub cost: ManaCost,
}

impl EffectWindow {
    pub(super) fn prepare_splice(
        &self,
        choices: &[SpliceChoice],
        actor: SeatId,
        host: &ObjectCharacteristics,
        current: &EvaluatedCharacteristics,
    ) -> Result<Vec<SplicedText>, EffectError> {
        let mut revealed = BTreeSet::new();
        choices
            .iter()
            .map(|choice| {
                let card = self
                    .window
                    .cards()
                    .get(choice.card)
                    .filter(|card| {
                        card.zone() == ObjectZone::Hand(actor)
                            && card.id() != host.object
                            && revealed.insert(card.id())
                    })
                    .ok_or(EffectError::InvalidCastResource)?;
                let program = self
                    .copiable_program(card.id())
                    .ok_or(EffectError::InvalidCastResource)?;
                let ability = program
                    .splice
                    .as_ref()
                    .ok_or(EffectError::InvalidCastResource)?;
                let eligible = match ability.quality {
                    SpliceQuality::Arcane => host
                        .current_spell_subtypes
                        .contains(&crate::program::SpellSubtype::Arcane),
                    SpliceQuality::InstantOrSorcery => {
                        host.has_type(CardType::Instant) || host.has_type(CardType::Sorcery)
                    }
                };
                if !eligible {
                    return Err(EffectError::InvalidCastResource);
                }
                let ProgramKind::Spell(spell) = &program.kind else {
                    return Err(EffectError::InconsistentProgram);
                };
                match (spell.body.target, choice.target) {
                    (BodyTarget::None, None) => {}
                    (BodyTarget::None, Some(_)) => return Err(EffectError::UnexpectedTarget),
                    (_, None) => return Err(EffectError::TargetRequired),
                    (_, Some(target))
                        if Self::protected_target(
                            target,
                            &super::protection::ProtectionSource::from(host),
                            current,
                        ) || !self.legal_body_target_in(&spell.body, target, actor, current) =>
                    {
                        return Err(EffectError::IllegalTarget);
                    }
                    _ => {}
                }
                Ok(SplicedText {
                    card: card.clone(),
                    body: spell.body.clone(),
                    target: choice.target,
                    cost: ability.cost.clone(),
                })
            })
            .collect()
    }
}

/// A single independent target clause may name the same object as another clause.
pub(super) fn append_splice_text(
    payload: &mut StackPayload,
    text: &[SplicedText],
    clauses: &mut Vec<TargetClause>,
    targets: &mut Vec<TargetChoice>,
) -> Result<(), EffectError> {
    if text.is_empty() {
        return Ok(());
    }
    let StackPayload::Instructions(payload) = payload else {
        return Err(EffectError::InvalidCastResource);
    };
    if payload.modes.is_empty() {
        for target in targets.iter() {
            payload
                .splice_target_bodies
                .insert(target.id, payload.body.clone());
        }
        payload.modes.push(modes::SelectedMode::splice(
            0,
            payload.body.instructions.len(),
            targets
                .iter()
                .map(|choice| (choice.id, payload.body.target)),
        ));
    }
    for addition in text {
        let start = payload.body.instructions.len();
        payload
            .body
            .instructions
            .extend(addition.body.instructions.iter().cloned());
        let mut clause_target = None;
        if let Some(target) = addition.target {
            let ordinal = clauses
                .iter()
                .map(|clause| clause.id.0)
                .max()
                .map_or(Some(0), |n| n.checked_add(1))
                .ok_or(EffectError::InvalidCastResource)?;
            let clause = TargetClauseId(ordinal);
            let id = TargetId(
                u32::try_from(targets.len()).map_err(|_| EffectError::InvalidCastResource)?,
            );
            clauses.push(TargetClause {
                id: clause,
                minimum: 1,
                maximum: 1,
            });
            targets.push(TargetChoice { id, clause, target });
            payload
                .splice_target_bodies
                .insert(id, addition.body.clone());
            clause_target = Some((id, addition.body.target));
        }
        payload.modes.push(modes::SelectedMode::splice(
            start,
            payload.body.instructions.len(),
            clause_target.into_iter(),
        ));
    }
    Ok(())
}
