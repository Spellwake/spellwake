//! Self-contained authored fixture inputs. No expected or reference output is read.
use spellwake_engine::{
    combat::CombatRules,
    effects::{Program, SeedCard},
    game::{GameContext, GameId, GameState, OpeningSetup},
    land::{BasicLandType, LandType},
    mana::{ManaAbilityRef, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectId},
    priority::SeatId,
    setup::GameSeed,
    shuffle::EntropyTape,
    turn::TurnStart,
};
use std::collections::{BTreeMap, BTreeSet};

pub const A: SeatId = SeatId(41);
pub const B: SeatId = SeatId(3);
pub const PLAINS_MANA_ABILITY: ManaAbilityRef = ManaAbilityRef::BasicLand(BasicLandType::Plains);
pub const PLAINS: &str = "plains_8ed_331";
pub const LION: &str = "silvercoat_lion_m13_35";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseId {
    G01,
    G02a,
    G02b,
    G03,
    /// Source-backed mandatory triggers and token lifecycle cases.
    Mandatory(super::mandatory::MandatoryCase),
    /// Source-backed optional effects, resolution payments and spell copies.
    Optional(super::optional::OptionalCase),
    /// Catalogue-backed teaching trajectory with its own explicit profile.
    Teaching(super::teaching::TeachingCase),
    /// Catalogue-backed physical interaction cases.
    Interaction(super::interaction::InteractionCase),
    /// Privileged source9 prepared fixture admission only.
    Selection(super::selection::SelectionCase),
}
impl CaseId {
    pub const ALL: [Self; 4] = [Self::G01, Self::G02a, Self::G02b, Self::G03];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::G01 => "G01",
            Self::G02a => "G02a",
            Self::G02b => "G02b",
            Self::G03 => "G03",
            Self::Teaching(case) => case.as_str(),
            Self::Interaction(case) => case.as_str(),
            Self::Mandatory(case) => case.as_str(),
            Self::Optional(case) => case.as_str(),
            Self::Selection(case) => case.as_str(),
        }
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|case| case.as_str() == value)
            .ok_or_else(|| format!("unknown ordinary-game case {value}"))
    }
    pub fn game_id(self) -> GameId {
        GameId(match self {
            Self::G01 => 5001,
            Self::G02a => 5002,
            Self::G02b => 5003,
            Self::G03 => 5004,
            Self::Teaching(case) => case.game_id(),
            Self::Interaction(case) => case.game_id(),
            Self::Mandatory(case) => case.game_id(),
            Self::Optional(case) => case.game_id(),
            Self::Selection(case) => case.game_id(),
        })
    }
    pub fn is_trade(self) -> bool {
        matches!(self, Self::G02a | Self::G02b)
    }
}

/// Finite observation capabilities, independent of source profile names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationScope {
    Historical,
    Teaching,
    Interaction,
    Mandatory,
    OptionalPayment,
    ResolutionSelection,
}
/// Wire report epoch; Registry and capture epochs remain independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportEpoch {
    Report1,
    Report2,
    Report3,
    Report4,
    Report5,
}
/// One exhaustive fixture admission contract for projection and recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservationContract {
    pub observation_scope: ObservationScope,
    pub report_epoch: ReportEpoch,
}
impl CaseId {
    pub const fn observation_contract(self) -> ObservationContract {
        let (observation_scope, report_epoch) = match self {
            Self::G01 | Self::G02a | Self::G02b | Self::G03 => {
                (ObservationScope::Historical, ReportEpoch::Report1)
            }
            Self::Teaching(_) => (ObservationScope::Teaching, ReportEpoch::Report1),
            Self::Interaction(_) => (ObservationScope::Interaction, ReportEpoch::Report2),
            Self::Mandatory(_) => (ObservationScope::Mandatory, ReportEpoch::Report3),
            Self::Optional(_) => (ObservationScope::OptionalPayment, ReportEpoch::Report4),
            Self::Selection(_) => (ObservationScope::ResolutionSelection, ReportEpoch::Report5),
        };
        ObservationContract {
            observation_scope,
            report_epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextInputs {
    pub reference: String,
    pub release: String,
    pub rules: String,
    pub creature_vocabulary: spellwake_contracts::creature_types::CreatureVocabularyId,
    pub name_rules_id: spellwake_contracts::names::NameRulesId,
    pub catalogue: String,
    pub programs: String,
    pub profile: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub alias: String,
    pub card: CardId,
    pub owner: SeatId,
    pub definition_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub path: String,
    pub sha256: String,
    pub uri: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub reference: String,
    pub name: String,
    pub oracle_id: String,
    pub printing_id: String,
    pub set: String,
    pub collector: String,
    pub language: String,
    pub sources: Vec<SourceIdentity>,
    pub program: Program,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShuffleRecipe {
    pub id: String,
    pub owner: SeatId,
    pub before: Vec<CardId>,
    pub target: Vec<CardId>,
    pub words: Vec<u64>,
    pub entropy_start: usize,
    pub entropy_end: usize,
}
/// Trusted native initialization mode, distinct from a replay/recovery checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureInitialization {
    Opening,
    Scheduled(TurnStart),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    pub case: CaseId,
    pub initialization: FixtureInitialization,
    pub game: GameId,
    pub context: GameContext,
    pub context_inputs: ContextInputs,
    pub setup: OpeningSetup,
    pub aliases: BTreeMap<CardId, Alias>,
    pub definitions: Vec<Definition>,
    pub shuffles: Vec<ShuffleRecipe>,
}
impl Fixture {
    pub(super) fn require_ordinary_execution(&self) -> Result<(), String> {
        if matches!(self.case, CaseId::Selection(_)) {
            return Err(
                "selection ordinary execution/projection is unsupported in fixture-only admission"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn new(
        case: CaseId,
        vocabulary: std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>,
    ) -> Result<Self, String> {
        let context_inputs = ContextInputs {
            reference: "ordinary-context-v1".into(),
            release: "ordinary-game-trajectories-v1".into(),
            rules: vocabulary.rules_revision().to_owned(),
            creature_vocabulary: vocabulary.id(),
            name_rules_id: spellwake_contracts::names::NameRules::literal_names().id(),
            catalogue: "restricted-primary-printing-fixtures-v1".into(),
            programs: "ordinary-plains-silvercoat-lion-authored-v1".into(),
            profile: "ordinary-game-trajectories-v1".into(),
        };
        let context = GameContext::new(
            context_inputs.release.clone(),
            vocabulary.rules_revision().to_owned(),
            context_inputs.catalogue.clone(),
            context_inputs.programs.clone(),
            context_inputs.profile.clone(),
            vocabulary.id(),
            context_inputs.name_rules_id,
        )
        .map_err(|error| format!("fixture context: {error:?}"))?;
        if matches!(
            case,
            CaseId::Teaching(_)
                | CaseId::Interaction(_)
                | CaseId::Mandatory(_)
                | CaseId::Optional(_)
                | CaseId::Selection(_)
        ) {
            return Err("teaching fixtures require actual pinned catalogue admission".into());
        }
        let definitions = definitions(&vocabulary)?;
        let mut aliases = BTreeMap::new();
        let mut cards = vec![];
        for (owner, prefix, offset) in [(A, "A", 0), (B, "B", 100)] {
            let lions = case != CaseId::G03 && (owner == A || case.is_trade());
            let plains = if lions { 56 } else { 60 };
            for index in 1..=60u64 {
                let (suffix, definition_ref) = if index <= plains {
                    (format!("P{index}"), PLAINS)
                } else {
                    (format!("L{}", index - plains), LION)
                };
                let card = CardId(offset + index);
                let definition = definitions
                    .iter()
                    .find(|entry| entry.reference == definition_ref)
                    .ok_or("fixture definition missing")?;
                aliases.insert(
                    card,
                    Alias {
                        alias: format!("{prefix}.{suffix}"),
                        card,
                        owner,
                        definition_ref: definition_ref.into(),
                    },
                );
                cards.push(SeedCard {
                    card,
                    owner,
                    destination: ObjectDestination::LibraryBottom,
                    program: definition.program.clone(),
                });
            }
        }
        let mut fixture = Self {
            case,
            initialization: FixtureInitialization::Opening,
            game: case.game_id(),
            context,
            context_inputs,
            setup: OpeningSetup {
                seed: GameSeed::with_uniform_life(
                    vec![A, B],
                    A,
                    cards,
                    vec![],
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    vocabulary.clone(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                ),
                entropy: EntropyTape::new(vec![]),
            },
            aliases,
            definitions,
            shuffles: vec![],
        };
        let initial_a = match case {
            CaseId::G01 => join(&[
                vec!["A.L1".into()],
                range("A", "P", 1, 56),
                range("A", "L", 2, 4),
            ]),
            CaseId::G02a | CaseId::G02b => join(&[
                range("A", "P", 50, 56),
                range("A", "P", 1, 49),
                range("A", "L", 1, 4),
            ]),
            CaseId::G03 => range("A", "P", 1, 60),
            CaseId::Teaching(_)
            | CaseId::Interaction(_)
            | CaseId::Mandatory(_)
            | CaseId::Optional(_)
            | CaseId::Selection(_) => {
                return Err("teaching fixture requires catalogue constructor".into());
            }
        };
        let initial_b = if case.is_trade() {
            join(&[
                range("B", "L", 1, 2),
                range("B", "P", 1, 56),
                range("B", "L", 3, 4),
            ])
        } else {
            range("B", "P", 1, 60)
        };
        for (owner, prefix, target) in [(A, "A", initial_a.clone()), (B, "B", initial_b)] {
            let before = fixture
                .setup
                .seed
                .cards
                .iter()
                .filter(|card| card.owner == owner)
                .map(|card| card.card)
                .collect();
            fixture.add_shuffle(
                format!("{}.{}.initial", case.as_str(), prefix),
                owner,
                before,
                &target,
            )?;
        }
        if case.is_trade() {
            let before_aliases = initial_a[7..]
                .iter()
                .chain(initial_a[..7].iter())
                .cloned()
                .collect::<Vec<_>>();
            let before = before_aliases
                .iter()
                .map(|alias| fixture.card(alias))
                .collect::<Result<Vec<_>, _>>()?;
            let target = join(&[
                range("A", "L", 1, 2),
                range("A", "P", 1, 8),
                vec!["A.L3".into()],
                range("A", "P", 9, 49),
                vec!["A.L4".into()],
                range("A", "P", 50, 56),
            ]);
            fixture.add_shuffle(
                format!("{}.A.mulligan.1", case.as_str()),
                A,
                before,
                &target,
            )?;
        }
        fixture.setup.entropy = EntropyTape::new(
            fixture
                .shuffles
                .iter()
                .flat_map(|entry| entry.words.iter().copied())
                .collect(),
        );
        Ok(fixture)
    }
    pub(super) fn add_shuffle(
        &mut self,
        id: String,
        owner: SeatId,
        before: Vec<CardId>,
        target: &[String],
    ) -> Result<(), String> {
        let target = target
            .iter()
            .map(|alias| self.card(alias))
            .collect::<Result<Vec<_>, _>>()?;
        let words = permutation_words(&before, &target)?;
        let entropy_start = self.shuffles.last().map_or(0, |entry| entry.entropy_end);
        self.shuffles.push(ShuffleRecipe {
            id,
            owner,
            before,
            target,
            entropy_end: entropy_start + words.len(),
            entropy_start,
            words,
        });
        Ok(())
    }
    pub fn card(&self, alias: &str) -> Result<CardId, String> {
        self.aliases
            .values()
            .find(|entry| entry.alias == alias)
            .map(|entry| entry.card)
            .ok_or_else(|| format!("unknown fixture alias {alias}"))
    }
    pub fn alias(&self, card: CardId) -> Result<&Alias, String> {
        self.aliases
            .get(&card)
            .ok_or_else(|| format!("unknown fixture physical card {}", card.0))
    }
    pub fn current_object(&self, state: &GameState, alias: &str) -> Result<ObjectId, String> {
        state
            .execution()
            .window()
            .cards()
            .current_identity(self.card(alias)?)
            .ok_or_else(|| format!("missing current fixture object {alias}"))
    }
    pub fn probe_context(&self) -> Result<(ContextInputs, GameContext), String> {
        let mut inputs = self.context_inputs.clone();
        inputs.reference = "ordinary-probe-context-v1".into();
        inputs.release.push_str("-probe");
        let vocabulary = &self.setup.seed.creature_vocabulary;
        let context = GameContext::new(
            inputs.release.clone(),
            vocabulary.rules_revision().to_owned(),
            inputs.catalogue.clone(),
            inputs.programs.clone(),
            inputs.profile.clone(),
            vocabulary.id(),
            self.setup.seed.name_rules.id(),
        )
        .map_err(|error| format!("probe context: {error:?}"))?;
        Ok((inputs, context))
    }
}

pub(crate) fn seat_name(seat: SeatId) -> Result<&'static str, String> {
    match seat {
        A => Ok("A"),
        B => Ok("B"),
        _ => Err("unknown fixture seat".into()),
    }
}
pub(crate) fn other(seat: SeatId) -> Result<SeatId, String> {
    match seat {
        A => Ok(B),
        B => Ok(A),
        _ => Err("unknown fixture seat".into()),
    }
}
pub(crate) fn range(seat: &str, kind: &str, start: u64, end: u64) -> Vec<String> {
    (start..=end)
        .map(|index| format!("{seat}.{kind}{index}"))
        .collect()
}
fn join(parts: &[Vec<String>]) -> Vec<String> {
    parts.iter().flatten().cloned().collect()
}
fn permutation_words(before: &[CardId], target: &[CardId]) -> Result<Vec<u64>, String> {
    let inventory = before.iter().copied().collect::<BTreeSet<_>>();
    if before.len() != 60
        || target.len() != 60
        || inventory.len() != 60
        || target.iter().copied().collect::<BTreeSet<_>>() != inventory
    {
        return Err("shuffle target is not the complete distinct 60-card inventory".into());
    }
    let mut shadow = before.to_vec();
    let mut words = Vec::with_capacity(59);
    for index in (1..shadow.len()).rev() {
        let selected = shadow[..=index]
            .iter()
            .position(|card| *card == target[index])
            .ok_or("target alias missing from unfixed shuffle prefix")?;
        words.push(u64::try_from(selected).map_err(|_| "shuffle index does not fit u64")?);
        shadow.swap(index, selected);
    }
    if shadow != target {
        return Err("shuffle shadow calculation did not reach target".into());
    }
    Ok(words)
}
fn definitions(
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<Vec<Definition>, String> {
    let mut plains = Program::land(BTreeSet::from([LandType::Plains]), vec![]);
    // The retained 8ED definition supplies Basic, independently of its name.
    plains.supertypes = BTreeSet::from([spellwake_engine::program::Supertype::Basic]);
    let mut definitions = vec![Definition {
        reference: PLAINS.into(), name: "Plains".into(), oracle_id: "bc71ebf6-2056-41f7-be35-b2e5c34afa99".into(),
        printing_id: "a37c667a-f035-49b2-9f34-9831e186ae82".into(), set: "8ed".into(), collector: "331".into(), language: "en".into(),
        sources: vec![SourceIdentity { path: ".quzzar-system-design/architecture-proposed/ordinary-game-trajectories/reference-research/sources/retained-wizards-plains-8ed-331.webp".into(),
            sha256: "3b8a3794b53bc0d86597917efa1e5ccc6caf3df07db5715ec33bdc5eab4376d9".into(),
            uri: "https://gatherer.wizards.com/Handlers/Image.ashx?multiverseid=46437&type=card".into() }],
        program: plains,
    }, Definition {
        reference: LION.into(), name: "Silvercoat Lion".into(), oracle_id: "e3c0b11e-7340-4e0a-98b7-91b38439d4f9".into(),
        printing_id: "9d33e866-cfd8-44e6-8070-df8df1ce965d".into(), set: "m13".into(), collector: "35".into(), language: "en".into(),
        sources: vec![SourceIdentity { path: ".quzzar-system-design/architecture-proposed/ordinary-game-trajectories/reference-research/sources/wizards-silvercoat-lion-m13-266132.webp".into(),
            sha256: "3ccce45e492f2f61ade791ea6e9ebb6c2b9501c8875e3e3a1c01aac32a65e8ed".into(),
            uri: "https://gatherer.wizards.com/Handlers/Image.ashx?multiverseid=266132&type=card".into() }],
        program: Program::creature(Some(ManaCost { generic:1.into(),required:BTreeMap::from([(ManaType::White,1.into())]) }), 2.into(), 2.into(), std::collections::BTreeSet::from([vocabulary.resolve_label("Cat").ok_or("source-guarded Lion requires Cat in explicitly selected vocabulary")?])),
    }];
    for definition in &mut definitions {
        definition.program.names = spellwake_contracts::names::ObjectNames::single(
            spellwake_contracts::names::EnglishCardName::new(definition.name.clone())
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(definitions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_recipe_has_complete_inventory_and_a_reconstructible_finite_tape() {
        for case in CaseId::ALL {
            let fixture = Fixture::new(
                case,
                crate::creature_types::synthetic_vocabulary_for_rules("synthetic-ordinary-unit"),
            )
            .unwrap();
            assert_eq!(fixture.aliases.len(), 120);
            assert_eq!(fixture.setup.seed.cards.len(), 120);
            assert_eq!(fixture.setup.seed.seats, vec![A, B]);
            assert!(fixture.setup.seed.mana.is_empty());
            for card in &fixture.setup.seed.cards {
                let expected = if card
                    .program
                    .has_type(spellwake_engine::program::CardType::Land)
                {
                    BTreeSet::from([spellwake_engine::program::Supertype::Basic])
                } else {
                    BTreeSet::new()
                };
                assert_eq!(card.program.supertypes, expected);
            }
            assert_eq!(
                fixture.setup.entropy.remaining(),
                if case.is_trade() { 177 } else { 118 }
            );
            for recipe in &fixture.shuffles {
                let mut shadow = recipe.before.clone();
                for (index, word) in (1..60).rev().zip(&recipe.words) {
                    shadow.swap(index, usize::try_from(*word).unwrap());
                }
                assert_eq!(shadow, recipe.target);
                assert_eq!(recipe.entropy_end - recipe.entropy_start, 59);
            }
        }
    }
    #[test]
    fn malformed_permutations_do_not_produce_entropy() {
        let before = (1..=60).map(CardId).collect::<Vec<_>>();
        let mut duplicate = before.clone();
        duplicate[59] = CardId(1);
        assert!(permutation_words(&before, &duplicate).is_err());
        assert!(permutation_words(&before, &before[..59]).is_err());
    }
}
