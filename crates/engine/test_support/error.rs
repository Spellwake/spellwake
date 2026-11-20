//! Lossless native error observations from exhaustive enum matching.
//! Diagnostic Debug text is retained verbatim and is never parsed for classification.
use serde_json::{Value, json};
use spellwake_engine::{
    actions::TimingError,
    characteristics::{
        CharacteristicDefinitionError, CharacteristicInvariantError, CharacteristicsError,
    },
    checks::CheckError,
    combat::CombatError,
    damage::DamageError,
    effects::EffectError,
    game::GameError,
    mana::ManaPaymentError,
    object_identity::ObjectIdentityError,
    occurrence::TriggerError,
    opening::OpeningError,
    priority::PriorityError,
    program::DefinitionError,
    shuffle::EntropyError,
    stack_window::StackWindowError,
    targets::TargetError,
    turn::{TurnError, TurnWork},
};
struct Node {
    path: Vec<&'static str>,
    domain: &'static str,
    variant: &'static str,
    data: Value,
}
impl Node {
    fn leaf(domain: &'static str, variant: &'static str, data: Value) -> Self {
        Self {
            path: vec![variant],
            domain,
            variant,
            data,
        }
    }
    fn wrapped(mut self, wrapper: &'static str) -> Self {
        self.path.insert(0, wrapper);
        self
    }
}
macro_rules! tree {
    ($name:ident,$kind:ident,[$($leaf:ident),* $(,)?],[$($wrapper:ident=>$nested:ident),* $(,)?])=>{
        fn $name(value:&$kind)->Node{match value{
            $($kind::$leaf=>Node::leaf(stringify!($kind),stringify!($leaf),Value::Null),)*
            $($kind::$wrapper(value)=>$nested(value).wrapped(stringify!($wrapper)),)*
        }}
    };
}
tree!(
    entropy,
    EntropyError,
    [Exhausted, InvalidBound, Capacity, EmptySupply],
    []
);
tree!(objects,ObjectIdentityError,[InconsistentTokenBatch,TokenCapacity,InvalidLibraryOrder,InconsistentLibrary,InconsistentOrderedZone,InconsistentRemoval,NegativeDrawCount,StaleRevision,RevisionExhausted,IdentityExhausted,CardAlreadyPresent,NotCard,UnknownObject,SameZone,NoController,NotPermanent,StatusUnchanged,NotDepartedToken,UnexpectedUnchangedMove],[Entropy=>entropy,InvalidTokenDefinition=>definition]);
tree!(
    priority,
    PriorityError,
    [
        InvalidOrder,
        UnknownFirstPlayer,
        StaleRevision,
        ChecksRequired,
        ChecksNotPending,
        WrongPlayer,
        RoundClosed,
        RevisionExhausted
    ],
    []
);
tree!(
    targets,
    TargetError,
    [
        InvalidClause,
        DuplicateClause,
        UnknownClause,
        TargetCount,
        DuplicateKey,
        RepeatedTarget,
        MissingTarget,
        UseStackReference,
        SelfTarget,
        TargetNotYetOnStack,
        MissingCheck,
        DuplicateCheck,
        UnknownCheck
    ],
    []
);
tree!(window,StackWindowError,[NotOwnHand,NotInTurnWork,StaleRevision,RevisionExhausted,EntryIdentityExhausted,WrongController,SourceTapped,UnknownPlayer,UntrackedInitialStack,NotInPriority,NotResolving,WrongResolution,ChecksNotPending,EmptyTriggerBatch,UnsupportedStackInsertion,MissingSpellExit,UnexpectedSpellExit,InconsistentStack,TargetChecksNotPending,TargetsNotRejected,UnknownTarget,IllegalTarget,UnsupportedTargetOperation,EmptyCheckMoves,InvalidCheckMove,EmptyResolutionMoves,InvalidResolutionMove,InvalidLibraryTop,NotSpell,CopyChecksRequired],[Priority=>priority,Objects=>objects,Targets=>targets]);
tree!(
    trigger,
    TriggerError,
    [
        EventIdentityExhausted,
        OccurrenceIndexExhausted,
        InstanceIdentityExhausted,
        WaveIdentityExhausted,
        EmptyEvent,
        InvalidTriggerCauses,
        TriggerCauseCapacity,
        WrongOrderFrame,
        WrongTargetFrame,
        InvalidTriggerOrder,
        InvalidWorld
    ],
    []
);
tree!(
    check,
    CheckError,
    [
        WrongFrame,
        InvalidDiscard,
        InvalidGraveyardOrder,
        InvalidLegendConflict,
        InvalidLegendKeep,
        ConflictCountExhausted,
        ChoiceRequired,
        IterationExhausted
    ],
    []
);
tree!(
    combat,
    CombatError,
    [
        UnknownPlayer,
        WrongFrame,
        InvalidContext,
        DuplicateAttacker,
        IllegalAttacker,
        IllegalDefender,
        DuplicateBlocker,
        IllegalBlocker,
        IllegalBlockTarget,
        AlreadyDeclared,
        DuplicateAssignment,
        IllegalDamageSource,
        IllegalDamageRecipient,
        InvalidDamageAmount,
        IncorrectDamageTotal
    ],
    []
);
tree!(
    timing,
    TimingError,
    [
        TurnRequired,
        PriorityRequired,
        NotPriorityHolder,
        NotActivePlayer,
        MainPhaseRequired,
        StackNotEmpty
    ],
    []
);
tree!(
    payment,
    ManaPaymentError,
    [
        InvalidCost,
        NonPositiveSpend,
        DuplicateType,
        MissingRequired,
        WrongTotal,
        InsufficientMana,
        PaymentRequired
    ],
    []
);
tree!(
    characteristic_definition,
    CharacteristicDefinitionError,
    [
        UnsupportedTypeOperation,
        MixedCopyParts,
        EmptyParts,
        DuplicateBucket,
        ForeignCreatureType,
        UndefinedCreatureTypeSlot,
        ContradictorySelector,
        EmptyPowerToughnessDefinition
    ],
    []
);
tree!(
    characteristics,
    CharacteristicsError,
    [
        EffectIdentityExhausted,
        TimestampExhausted,
        WorldTenureExhausted,
        InvalidInitialWorldCohorts,
        InvalidCounterPlacement,
        InvalidInitialBattlefieldOrder,
        InitialBattlefieldAlreadyRegistered,
        DuplicateBattlefieldTimestamp,
        UnsupportedTimestampProducer
    ],
    [InvalidParts => characteristic_definition]
);
fn characteristic_invariant(value: &CharacteristicInvariantError) -> Node {
    let (variant, data) = match value {
        CharacteristicInvariantError::MissingDefinition(card) => {
            ("MissingDefinition", json!({"object":format!("{card:?}")}))
        }
        CharacteristicInvariantError::MissingBattlefieldTimestamp(object) => (
            "MissingBattlefieldTimestamp",
            json!({"object":format!("{object:?}")}),
        ),
        CharacteristicInvariantError::MissingBattlefieldController(object) => (
            "MissingBattlefieldController",
            json!({"object":format!("{object:?}")}),
        ),
        CharacteristicInvariantError::DuplicateTimestamp(timestamp) => (
            "DuplicateTimestamp",
            json!({"timestamp":timestamp.sequence().to_string()}),
        ),
        CharacteristicInvariantError::MissingChosenCreatureType(slot) => (
            "MissingChosenCreatureType",
            json!({"slot":slot.ordinal().to_string()}),
        ),
        CharacteristicInvariantError::InvalidResolvedParts(effect) => (
            "InvalidResolvedParts",
            json!({"effect":effect.sequence().to_string()}),
        ),
        CharacteristicInvariantError::InvalidStaticParts(object) => (
            "InvalidStaticParts",
            json!({"object":format!("{object:?}")}),
        ),
        CharacteristicInvariantError::NoEligibleComponent => ("NoEligibleComponent", Value::Null),
        CharacteristicInvariantError::MissingWorldTenure(object) => (
            "MissingWorldTenure",
            json!({"object":format!("{object:?}")}),
        ),
        CharacteristicInvariantError::UnexpectedWorldTenure(object) => (
            "UnexpectedWorldTenure",
            json!({"object":format!("{object:?}")}),
        ),
    };
    Node::leaf("CharacteristicInvariantError", variant, data)
}
tree!(
    damage,
    DamageError,
    [
        InvalidAmount,
        InvalidFactor,
        EmptyBatch,
        DuplicateSourceRecipient,
        UnsupportedSimultaneousScope,
        ShieldIdentityExhausted,
        MultiplierIdentityExhausted,
        BatchIdentityExhausted,
        AttemptIdentityExhausted,
        StageExhausted,
        WrongPhase,
        InapplicableEffect,
        InvalidAllocation,
        EffectsStillApplicable
    ],
    []
);
use spellwake_engine::effects::ZoneReplacementError;
tree!(
    zone_replacement,
    ZoneReplacementError,
    [
        WrongFrame,
        InapplicableApplication,
        InconsistentBatch,
        StageExhausted
    ],
    []
);
tree!(effect,EffectError,[ReplacementChooserRequired,WrongReplacementChooserFrame,UnsupportedDepartureState,LibraryTopChoiceRequired,WrongLibraryTopFrame,InvalidLibraryPartition,InconsistentLibraryTopWork,SelectionRequired,WrongSelectionFrame,InvalidSelectionCount,DuplicateSelection,SelectionOutsideDomain,InvalidGraveyardPermutation,InconsistentSelectionWork,OptionalEffectChoiceRequired,ResolutionPaymentChoiceRequired,WrongOptionalEffectFrame,WrongResolutionPaymentFrame,IllegalOptionalEffect,InvalidResolvingPlayer,TimestampOrderRequired,WrongTimestampOrderFrame,IllegalTimestampOrder,TokenCapacity,PlayerLossPending,InvalidSetup,UnknownObject,NotCastableSpell,LegendarySpellPrerequisite,TargetRequired,UnexpectedTarget,NotLand,NotAbilitySource,CreatureTapRestricted,UnknownActivatedAbility,NotManaSource,UnknownManaAbility,ManaColorChoiceRequired,WrongManaColorFrame,IllegalManaColor,NotOwnHand,IllegalTarget,InsufficientMana,ManaPaymentRequired,WrongPhase,InconsistentProgram,CopyChoiceRequired,UnexpectedCopyChoice,WrongChooser,WrongResolutionFrame,WrongDamageFrame,WrongDecisionKind,DamageChoiceRequired,TriggerTargetChoiceRequired,CreatureTypeChoiceRequired,WrongCreatureTypeFrame,IllegalCreatureType],[ZoneReplacement=>zone_replacement,Trigger=>trigger,Check=>check,Combat=>combat,Window=>window,Timing=>timing,InvalidManaPayment=>payment,Characteristics=>characteristics,CharacteristicInvariant=>characteristic_invariant,Damage=>damage]);
tree!(
    opening,
    OpeningError,
    [
        InvalidSetup,
        NotOpening,
        WrongStage,
        WrongReply,
        IllegalMulligan,
        InvalidBottom
    ],
    []
);
tree!(
    turn,
    TurnError,
    [
        LandLimitReached,
        InvalidStart,
        WorkRequired,
        PriorityRequired,
        WrongWork,
        WrongPriorityOutcome,
        TurnExhausted,
        CleanupExhausted
    ],
    []
);
fn work(value: TurnWork) -> Value {
    match value {
        TurnWork::Untap => json!({"tag":"Untap"}),
        TurnWork::Draw => json!({"tag":"Draw"}),
        TurnWork::DeclareAttackers => json!({"tag":"DeclareAttackers"}),
        TurnWork::DeclareBlockers => json!({"tag":"DeclareBlockers"}),
        TurnWork::CombatDamage { additional } => {
            json!({"tag":"CombatDamage","additional":additional})
        }
        TurnWork::Cleanup => json!({"tag":"Cleanup"}),
    }
}
fn game(value: &GameError) -> Node {
    match value {
        GameError::Opening(value) => opening(value).wrapped("Opening"),
        GameError::Entropy(value) => entropy(value).wrapped("Entropy"),
        GameError::Turn(value) => turn(value).wrapped("Turn"),
        GameError::Effect(value) => effect(value).wrapped("Effect"),
        GameError::TurnWorkRequired(value) => Node::leaf(
            "GameError",
            "TurnWorkRequired",
            json!({"work":work(*value)}),
        ),
        GameError::GameEnded => Node::leaf("GameError", "GameEnded", Value::Null),
        GameError::UnsupportedConcessionScope => {
            Node::leaf("GameError", "UnsupportedConcessionScope", Value::Null)
        }
        GameError::UnsupportedBankExpiryScope => {
            Node::leaf("GameError", "UnsupportedBankExpiryScope", Value::Null)
        }
        GameError::UnsupportedResponseExpiryScope => {
            Node::leaf("GameError", "UnsupportedResponseExpiryScope", Value::Null)
        }
        GameError::ResponsePlayerNotWaiting => {
            Node::leaf("GameError", "ResponsePlayerNotWaiting", Value::Null)
        }
        GameError::BankPlayerNotWaiting => {
            Node::leaf("GameError", "BankPlayerNotWaiting", Value::Null)
        }
        GameError::PlayerEliminationRequired => {
            Node::leaf("GameError", "PlayerEliminationRequired", Value::Null)
        }
        GameError::InvalidContext => Node::leaf("GameError", "InvalidContext", Value::Null),
        GameError::WrongGame => Node::leaf("GameError", "WrongGame", Value::Null),
        GameError::WrongContext => Node::leaf("GameError", "WrongContext", Value::Null),
        GameError::WrongEpoch => Node::leaf("GameError", "WrongEpoch", Value::Null),
        GameError::StaleRevision => Node::leaf("GameError", "StaleRevision", Value::Null),
        GameError::UnknownActor => Node::leaf("GameError", "UnknownActor", Value::Null),
        GameError::DecisionRequired => Node::leaf("GameError", "DecisionRequired", Value::Null),
        GameError::NoDecision => Node::leaf("GameError", "NoDecision", Value::Null),
        GameError::WrongDecision => Node::leaf("GameError", "WrongDecision", Value::Null),
        GameError::WrongChooser => Node::leaf("GameError", "WrongChooser", Value::Null),
        GameError::RevisionExhausted => Node::leaf("GameError", "RevisionExhausted", Value::Null),
        GameError::DecisionIdentityExhausted => {
            Node::leaf("GameError", "DecisionIdentityExhausted", Value::Null)
        }
        GameError::UnexpectedKernelPhase => {
            Node::leaf("GameError", "UnexpectedKernelPhase", Value::Null)
        }
    }
}
fn render(domain: &str, value: impl std::fmt::Debug, node: Node) -> Value {
    json!({"tag":"Err","error":format!("{value:?}"),"error_domain":domain,"error_path":node.path,"error_leaf":{"domain":node.domain,"variant":node.variant,"data":node.data}})
}
/// Observe a real root rejection; driver/codec String failures use a separate envelope.
pub fn game_error(value: &GameError) -> Value {
    render("GameError", value, game(value))
}
/// Observe a real executor rejection, including all actual wrapper layers.
pub fn effect_error(value: &EffectError) -> Value {
    render("EffectError", value, effect(value))
}

tree!(
    definition,
    DefinitionError,
    [
        UnsupportedZoneReplacementScope,
        InvalidZoneReplacementDefinition,
        TokenDefinition,
        ColorIndicator,
        CardTypes,
        CastingCost,
        ManaSource,
        ActivatedSource,
        TriggeredSource,
        DuplicateAbility,
        EmptyBody,
        InvalidInstruction,
        InvalidControlFlow,
        UnboundTriggerCondition,
        UnboundResolvingPlayer,
        UnguaranteedRelatedPlayer,
        ResolvingPlayerTarget,
        MissingOptionalEffect,
        UnsupportedOptionalEffect,
        InvalidResolutionPaymentCost,
        OperandFamily,
        PhantomTarget,
        TargetlessActivation,
        InvalidCreatureSubtypes,
        InvalidLandSubtypes,
        ForeignCreatureType,
        DuplicateCreatureTypeSlot,
        UndefinedCreatureTypeSlot,
        UnboundTriggerAmount,
        EmptyCreatureTypeDomain,
        ContinuousEffect
    ],
    []
);
/// Standalone primitive/constructor observations retain their actual root domain.
pub fn definition_error(value: &DefinitionError) -> Value {
    render("DefinitionError", value, definition(value))
}
pub fn stack_window_error(value: &StackWindowError) -> Value {
    render("StackWindowError", value, window(value))
}
pub fn object_identity_error(value: &ObjectIdentityError) -> Value {
    render("ObjectIdentityError", value, objects(value))
}
pub fn trigger_error(value: &TriggerError) -> Value {
    render("TriggerError", value, trigger(value))
}
pub fn turn_error(value: &TurnError) -> Value {
    render("TurnError", value, turn(value))
}
