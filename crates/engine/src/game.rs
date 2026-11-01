//! Native game-scoped input boundary over the restricted executor.
//!
//! Candidates are owned values, not durable commits. This module supplies no auth,
//! clocks, wire/checkpoint codec, seat projection, replay or general Magic engine.
use crate::{
    card_identity::CardObjectId,
    damage::DamageJournal,
    effects::{EffectError, EffectWindow, ResolutionProgress, SeedCard},
    mana::{FloatingMana, ManaAbilityId, ManaPayment, ManaPaymentRecord, ManaProductionRecord},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
};

/// Host-supplied identity; numeric object/seat identities remain local to this game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameId(pub u128);

/// Owned immutable source identities. Constructors validate shape, not source truth
/// or all-card qualification; the enclosing publisher must establish those claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameContext {
    release: String,
    rules: String,
    catalogue: String,
    programs: String,
    profile: String,
}
impl GameContext {
    pub fn new(
        release: String,
        rules: String,
        catalogue: String,
        programs: String,
        profile: String,
    ) -> Result<Self, GameError> {
        if [&release, &rules, &catalogue, &programs, &profile]
            .iter()
            .any(|value| value.trim().is_empty() || value.trim() != value.as_str())
        {
            return Err(GameError::InvalidContext);
        }
        Ok(Self {
            release,
            rules,
            catalogue,
            programs,
            profile,
        })
    }
}

/// Exact expected input basis. External accepted-input revision is distinct from
/// the private executor's counters, which can advance several times per input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBasis {
    pub game: GameId,
    pub context: GameContext,
    pub epoch: u64,
    pub revision: u64,
}

/// Decision identity local to its complete game/context/epoch/revision basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionId(pub u64);

/// One canonical native frame/reply vocabulary shared with the executor.
pub use crate::effects::{DecisionReply, ResolutionFrame as RuleFrame};

/// Privileged native decision record, not a safe public DecisionView.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionState {
    id: DecisionId,
    basis: GameBasis,
    chooser: SeatId,
    frame: RuleFrame,
}
impl DecisionState {
    pub fn id(&self) -> DecisionId {
        self.id
    }
    pub fn basis(&self) -> &GameBasis {
        &self.basis
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn frame(&self) -> &RuleFrame {
        &self.frame
    }
}

/// Only commands supported by the current closed-world executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameCommand {
    ActivateMana {
        source: CardObjectId,
        ability: ManaAbilityId,
    },
    Cast {
        spell: CardObjectId,
        target: TargetRef,
    },
    CastPaid {
        spell: CardObjectId,
        target: TargetRef,
        payment: ManaPayment,
    },
    Pass,
    Reply {
        decision: DecisionId,
        choice: DecisionReply,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInput {
    pub basis: GameBasis,
    pub actor: SeatId,
    pub command: GameCommand,
}

/// Initialization does not pretend that a previous accepted state existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateBase {
    Initialize { game: GameId, context: GameContext },
    Advance(GameBasis),
}
/// The caller must still serialize and atomically accept this native candidate.
/// It is not a complete event/replay journal or an authorization receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCandidate {
    base: CandidateBase,
    next: GameState,
    damage_journal: DamageJournal,
    mana_payment: Option<ManaPaymentRecord>,
    mana_production: Option<ManaProductionRecord>,
}
impl NativeCandidate {
    pub fn mana_production(&self) -> Option<&ManaProductionRecord> {
        self.mana_production.as_ref()
    }
    pub fn mana_payment(&self) -> Option<&ManaPaymentRecord> {
        self.mana_payment.as_ref()
    }
    pub fn base(&self) -> &CandidateBase {
        &self.base
    }
    pub fn damage_journal(&self) -> &DamageJournal {
        &self.damage_journal
    }
    pub fn state(&self) -> &GameState {
        &self.next
    }
    /// Adopt for native execution only; this method performs no durable acceptance.
    pub fn into_state(self) -> GameState {
        self.next
    }
}

/// One root owns one executor and its accepted boundary/continuation metadata.
/// Objects, stack, payloads and resources are never duplicated in another store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    basis: GameBasis,
    execution: EffectWindow,
    pending: Option<DecisionState>,
    next_decision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    InvalidContext,
    WrongGame,
    WrongContext,
    WrongEpoch,
    StaleRevision,
    UnknownActor,
    DecisionRequired,
    NoDecision,
    WrongDecision,
    WrongChooser,
    RevisionExhausted,
    DecisionIdentityExhausted,
    UnexpectedKernelPhase,
    Effect(EffectError),
}
impl From<EffectError> for GameError {
    fn from(value: EffectError) -> Self {
        Self::Effect(value)
    }
}

impl GameState {
    pub fn initialize(
        game: GameId,
        context: GameContext,
        seats: Vec<SeatId>,
        active: SeatId,
        cards: Vec<SeedCard>,
        mana: Vec<FloatingMana>,
    ) -> Result<NativeCandidate, GameError> {
        let base = CandidateBase::Initialize {
            game,
            context: context.clone(),
        };
        let mut damage_journal = DamageJournal::default();
        let next = Self {
            basis: GameBasis {
                game,
                context,
                epoch: 0,
                revision: 0,
            },
            execution: EffectWindow::new(seats, active, cards, mana)?,
            pending: None,
            next_decision: 1,
        }
        .drive(&mut damage_journal)?;
        Ok(NativeCandidate {
            base,
            next,
            damage_journal,
            mana_payment: None,
            mana_production: None,
        })
    }
    pub fn basis(&self) -> &GameBasis {
        &self.basis
    }
    pub fn pending_decision(&self) -> Option<&DecisionState> {
        self.pending.as_ref()
    }
    /// Privileged native observation, not a seat-safe projection or mutable handle.
    pub fn execution(&self) -> &EffectWindow {
        &self.execution
    }

    pub fn advance(&self, input: GameInput) -> Result<NativeCandidate, GameError> {
        self.validate_basis(&input.basis)?;
        if !self
            .execution
            .window()
            .participants()
            .contains(&input.actor)
        {
            return Err(GameError::UnknownActor);
        }
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        let mut next = self.clone();
        let mut damage_journal = DamageJournal::default();
        let mut mana_payment = None;
        let mut mana_production = None;
        let progress = match (&self.pending, input.command) {
            (Some(pending), GameCommand::Reply { decision, choice }) => {
                if decision != pending.id {
                    return Err(GameError::WrongDecision);
                }
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                Some(self.execution.resume_resolution(
                    self.execution.revision(),
                    &pending.frame,
                    input.actor,
                    choice,
                )?)
            }
            (Some(_), _) => return Err(GameError::DecisionRequired),
            (None, GameCommand::Reply { .. }) => return Err(GameError::NoDecision),
            (None, GameCommand::Pass) => {
                next.execution = self
                    .execution
                    .pass(self.execution.revision(), input.actor)?;
                None
            }
            (None, GameCommand::ActivateMana { source, ability }) => {
                let activated = self.execution.activate_mana(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                )?;
                mana_production = Some(activated.production().clone());
                next.execution = activated.into_state();
                None
            }
            (None, GameCommand::Cast { spell, target }) => {
                let cast = self.execution.cast_with_payment(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    target,
                    None,
                )?;
                mana_payment = Some(cast.payment().clone());
                next.execution = cast.into_state();
                None
            }
            (
                None,
                GameCommand::CastPaid {
                    spell,
                    target,
                    payment,
                },
            ) => {
                let cast = self.execution.cast_with_payment(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    target,
                    Some(payment),
                )?;
                mana_payment = Some(cast.payment().clone());
                next.execution = cast.into_state();
                None
            }
        };
        next.basis.revision = revision;
        if let Some(progress) = progress {
            next.accept_progress(progress, &mut damage_journal)?;
        }
        let next = next.drive(&mut damage_journal)?;
        Ok(NativeCandidate {
            base: CandidateBase::Advance(self.basis.clone()),
            next,
            damage_journal,
            mana_payment,
            mana_production,
        })
    }
    fn validate_basis(&self, basis: &GameBasis) -> Result<(), GameError> {
        if basis.game != self.basis.game {
            return Err(GameError::WrongGame);
        }
        if basis.context != self.basis.context {
            return Err(GameError::WrongContext);
        }
        if basis.epoch != self.basis.epoch {
            return Err(GameError::WrongEpoch);
        }
        if basis.revision != self.basis.revision {
            return Err(GameError::StaleRevision);
        }
        Ok(())
    }
    fn accept_progress(
        &mut self,
        progress: ResolutionProgress,
        journal: &mut DamageJournal,
    ) -> Result<(), GameError> {
        match progress {
            ResolutionProgress::Complete {
                state,
                journal: append,
            } => {
                self.execution = state;
                self.pending = None;
                journal.append(append);
            }
            ResolutionProgress::Pending {
                state,
                frame,
                journal: append,
            } => {
                let id = DecisionId(self.next_decision);
                self.next_decision = self
                    .next_decision
                    .checked_add(1)
                    .ok_or(GameError::DecisionIdentityExhausted)?;
                self.pending = Some(DecisionState {
                    id,
                    basis: self.basis.clone(),
                    chooser: frame.chooser(),
                    frame,
                });
                self.execution = state;
                journal.append(append);
            }
        }
        Ok(())
    }
    fn drive(mut self, journal: &mut DamageJournal) -> Result<Self, GameError> {
        loop {
            if self.pending.is_some() {
                return Ok(self);
            }
            match self.execution.window().phase() {
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. }) => {
                    self.execution = self.execution.settle_checks(self.execution.revision())?;
                }
                WindowPhase::CheckingTargets(_) => {
                    let progress = self.execution.begin_resolution(self.execution.revision())?;
                    self.accept_progress(progress, journal)?;
                }
                WindowPhase::Priority(PriorityPhase::Ready { .. }) | WindowPhase::StepEnded => {
                    return Ok(self);
                }
                _ => return Err(GameError::UnexpectedKernelPhase),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mana_activation_late_drive_failure_accepts_no_tap_credit_or_record() {
        use crate::mana::{FixedManaAbility, ManaAbilityId, ManaType};
        use std::collections::BTreeMap;
        let a = SeatId(0);
        let b = SeatId(1);
        let initial = GameState::initialize(
            GameId(60),
            GameContext::new(
                "test".into(),
                "rules".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
            )
            .unwrap(),
            vec![a, b],
            a,
            vec![SeedCard {
                card: CardId(1),
                owner: a,
                destination: CardDestination::Battlefield { controller: a },
                program: Program::Land {
                    abilities: vec![FixedManaAbility {
                        id: ManaAbilityId(1),
                        produced: BTreeMap::from([(ManaType::Red, 2.into())]),
                    }],
                },
            }],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 5.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
        )
        .unwrap()
        .into_state();
        let source = initial
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for fault in 0..4 {
            let mut state = initial.clone();
            let expected = if fault == 0 {
                state.basis.revision = u64::MAX;
                GameError::RevisionExhausted
            } else {
                match fault {
                    1 => state.execution.set_test_revision(u64::MAX),
                    2 => state.execution.set_test_revision(u64::MAX - 1),
                    _ => state.execution.set_test_window_revision(u64::MAX - 1),
                }
                GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted,
                ))
            };
            let before = state.clone();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: a,
                    command: GameCommand::ActivateMana {
                        source,
                        ability: ManaAbilityId(1)
                    }
                }),
                Err(expected)
            );
            assert_eq!(state, before);
            assert_eq!(state.execution.mana(a, ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(b, ManaType::Blue), 11.into());
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(false)
            );
            assert!(state.execution.window().entries().is_empty());
        }
    }
    use crate::{
        card_identity::{CardDestination, CardId},
        effects::{Instruction, Program},
        mana::{ManaCost, ManaType},
    };
    #[test]
    fn paid_cast_outer_and_post_cast_drive_overflow_accept_no_receipt_or_debit() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = GameState::initialize(
            GameId(50),
            GameContext::new(
                "test".into(),
                "rules".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
            )
            .unwrap(),
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: CardDestination::Hand,
                    program: Program::Instant {
                        cost: ManaCost::one(ManaType::Red),
                        instructions: vec![Instruction::Damage { amount: 1.into() }],
                    },
                },
                SeedCard {
                    card: CardId(2),
                    owner: b,
                    destination: CardDestination::Battlefield { controller: b },
                    program: Program::VanillaCreature {
                        power: 1.into(),
                        toughness: 10.into(),
                    },
                },
            ],
            vec![FloatingMana {
                player: a,
                mana_type: ManaType::Red,
                amount: 2.into(),
            }],
        )
        .unwrap()
        .into_state();
        let spell = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        let target = TargetRef::Card(
            state
                .execution
                .window()
                .cards()
                .current_identity(CardId(2))
                .unwrap(),
        );
        state.basis.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: a,
                command: GameCommand::Cast { spell, target }
            }),
            Err(GameError::RevisionExhausted)
        );
        assert_eq!(state, saved);
        state.basis.revision = 0;
        for revision in [u64::MAX, u64::MAX - 1] {
            state.execution.set_test_revision(revision);
            let saved = state.clone();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: a,
                    command: GameCommand::Cast { spell, target }
                }),
                Err(GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted
                )))
            );
            assert_eq!(state, saved);
            assert_eq!(state.execution.mana(a, ManaType::Red), 2.into());
            assert!(state.execution.window().entries().is_empty());
        }
    }
    #[test]
    fn initial_program_allocation_failure_discards_all_prior_instructions() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = GameState::initialize(
            GameId(40),
            GameContext::new(
                "test".into(),
                "rules".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
            )
            .unwrap(),
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: CardDestination::Hand,
                    program: Program::Instant {
                        cost: ManaCost::one(ManaType::Red),
                        instructions: vec![
                            Instruction::Damage { amount: 2.into() },
                            Instruction::CreatePreventionShield { amount: 3.into() },
                            Instruction::CreatePreventionShield { amount: 4.into() },
                            Instruction::Damage { amount: 5.into() },
                        ],
                    },
                },
                SeedCard {
                    card: CardId(2),
                    owner: b,
                    destination: CardDestination::Battlefield { controller: b },
                    program: Program::VanillaCreature {
                        power: 1.into(),
                        toughness: 100.into(),
                    },
                },
            ],
            vec![FloatingMana {
                player: a,
                mana_type: ManaType::Red,
                amount: 1.into(),
            }],
        )
        .unwrap()
        .into_state();
        let target = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(2))
            .unwrap();
        let spell = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for (actor, command) in [
            (
                a,
                GameCommand::Cast {
                    spell,
                    target: TargetRef::Card(target),
                },
            ),
            (a, GameCommand::Pass),
        ] {
            state = state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        state.next_decision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: b,
                command: GameCommand::Pass
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, saved);
        assert_eq!(state.execution.creature(target).unwrap().damage, 0.into());
        assert!(state.execution.prevention_shields().next().is_none());
        assert_eq!(state.execution.instruction_index(), None);
        assert!(state.pending.is_none());
    }
    #[test]
    fn allocation_failure_after_a_mixed_reply_preserves_accepted_suspension() {
        let a = SeatId(0);
        let b = SeatId(1);
        let instructions = [
            Instruction::CreatePreventionShield { amount: 1.into() },
            Instruction::CreateDamageMultiplier { factor: 2.into() },
            Instruction::CreateDamageMultiplier { factor: 3.into() },
            Instruction::Damage { amount: 5.into() },
        ];
        let mut cards: Vec<_> = instructions
            .into_iter()
            .enumerate()
            .map(|(index, instruction)| SeedCard {
                card: CardId(index as u64 + 1),
                owner: a,
                destination: CardDestination::Hand,
                program: Program::Instant {
                    cost: ManaCost::one(ManaType::Red),
                    instructions: vec![instruction],
                },
            })
            .collect();
        cards.push(SeedCard {
            card: CardId(10),
            owner: b,
            destination: CardDestination::Battlefield { controller: b },
            program: Program::VanillaCreature {
                power: 1.into(),
                toughness: 100.into(),
            },
        });
        let context = GameContext::new(
            "test".into(),
            "rules".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
        )
        .unwrap();
        let mut state = GameState::initialize(
            GameId(20),
            context,
            vec![a, b],
            a,
            cards,
            vec![FloatingMana {
                player: a,
                mana_type: ManaType::Red,
                amount: 4.into(),
            }],
        )
        .unwrap()
        .into_state();
        for card in 1..=4 {
            let target = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(10))
                .unwrap();
            let spell = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(card))
                .unwrap();
            for (actor, command) in [
                (
                    a,
                    GameCommand::Cast {
                        spell,
                        target: TargetRef::Card(target),
                    },
                ),
                (a, GameCommand::Pass),
                (b, GameCommand::Pass),
            ] {
                state = state
                    .advance(GameInput {
                        basis: state.basis.clone(),
                        actor,
                        command,
                    })
                    .unwrap()
                    .into_state();
            }
        }
        assert!(state.pending.is_some());
        state.next_decision = u64::MAX;
        let saved = state.clone();
        let shield = state.execution.prevention_shields().next().unwrap().id;
        let pending = state.pending.as_ref().unwrap();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: b,
                command: GameCommand::Reply {
                    decision: pending.id,
                    choice: DecisionReply::DamageOrder(shield.into())
                }
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, saved);
        assert_eq!(
            state.execution.pending_damage().unwrap().remaining(),
            &5.into()
        );
        assert_eq!(
            state.execution.prevention_shield(shield).unwrap().remaining,
            1.into()
        );
        assert_eq!(state.execution.pending_damage().unwrap().stage(), 0);
    }
    #[test]
    fn context_rejects_missing_or_noncanonical_identity_components() {
        for index in 0..5 {
            for invalid in ["", "   ", " release", "release "] {
                let mut labels =
                    ["release", "rules", "catalogue", "programs", "profile"].map(String::from);
                labels[index] = invalid.into();
                let [release, rules, catalogue, programs, profile] = labels;
                assert_eq!(
                    GameContext::new(release, rules, catalogue, programs, profile),
                    Err(GameError::InvalidContext)
                );
            }
        }
    }
    #[test]
    fn revision_exhaustion_preserves_private_state_without_forging_a_public_constructor() {
        let context = GameContext::new(
            "r".into(),
            "rules".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
        )
        .unwrap();
        let mut game = GameState::initialize(
            GameId(1),
            context,
            vec![SeatId(0), SeatId(1)],
            SeatId(0),
            vec![],
            vec![],
        )
        .unwrap()
        .into_state();
        game.basis.revision = u64::MAX;
        let saved = game.clone();
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass
            }),
            Err(GameError::RevisionExhausted)
        );
        assert_eq!(game, saved);
    }

    #[test]
    fn decision_identity_exhaustion_rejects_the_last_pass_without_partial_resolution() {
        let a = SeatId(0);
        let b = SeatId(1);
        let context = GameContext::new(
            "native-test".into(),
            "rules".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
        )
        .unwrap();
        let instant = |card, owner, colour, instructions| SeedCard {
            card: CardId(card),
            owner,
            destination: CardDestination::Hand,
            program: Program::Instant {
                cost: ManaCost::one(colour),
                instructions,
            },
        };
        let mut game = GameState::initialize(
            GameId(1),
            context,
            vec![a, b],
            a,
            vec![
                instant(
                    1,
                    a,
                    ManaType::Red,
                    vec![Instruction::Damage { amount: 3.into() }],
                ),
                instant(
                    2,
                    b,
                    ManaType::Blue,
                    vec![Instruction::CopySpell, Instruction::CopySpell],
                ),
                SeedCard {
                    card: CardId(3),
                    owner: b,
                    destination: CardDestination::Battlefield { controller: b },
                    program: Program::VanillaCreature {
                        power: 2.into(),
                        toughness: 2.into(),
                    },
                },
            ],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 1.into(),
                },
            ],
        )
        .unwrap()
        .into_state();
        let object = |game: &GameState, card| {
            game.execution
                .window()
                .cards()
                .current_identity(CardId(card))
                .unwrap()
        };
        let command = GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Card(object(&game, 3)),
        };
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command,
            })
            .unwrap()
            .into_state();
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        let original = game.execution.window().entries()[0].id();
        let command = GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Stack(original),
        };
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: b,
                command,
            })
            .unwrap()
            .into_state();
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: b,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        game.next_decision = u64::MAX;
        let saved = game.clone();
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(game, saved);
        assert!(game.pending_decision().is_none());
        assert!(game.execution.window().spell_copies().is_empty());
        assert!(
            matches!(game.execution.window().phase(), WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == a)
        );
        // Accept the first suspension, then fail only after its valid reply creates
        // a copy and the next instruction needs another decision identity.
        game.next_decision = 1;
        let mut pending = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        pending.next_decision = u64::MAX;
        let saved = pending.clone();
        let decision = pending.pending.as_ref().unwrap();
        assert_eq!(
            pending.advance(GameInput {
                basis: pending.basis.clone(),
                actor: b,
                command: GameCommand::Reply {
                    decision: decision.id,
                    choice: DecisionReply::CopyTargets(crate::effects::CopyTargetChoice::Retain)
                }
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(pending, saved);
        assert!(pending.execution.window().spell_copies().is_empty());
        assert_eq!(pending.execution.instruction_index(), Some(0));
    }
}
