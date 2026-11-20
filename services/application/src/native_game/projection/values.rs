//! Exhaustive native/public value mappings, without Debug or numeric truncation.
use super::*;
pub(super) fn quantity(value: &RulesInteger) -> Result<RulesCount, NativeProjectionError> {
    RulesCount::try_from(String::from(value.clone()))
        .map_err(|_| NativeProjectionError::InvalidSnapshot)
}
pub(super) fn count(value: usize) -> Result<RulesCount, NativeProjectionError> {
    quantity(&RulesInteger::from_usize(value))
}
pub(super) fn mana(value: spellwake_engine::mana::ManaType) -> ManaColor {
    match value {
        spellwake_engine::mana::ManaType::White => ManaColor::White,
        spellwake_engine::mana::ManaType::Blue => ManaColor::Blue,
        spellwake_engine::mana::ManaType::Black => ManaColor::Black,
        spellwake_engine::mana::ManaType::Red => ManaColor::Red,
        spellwake_engine::mana::ManaType::Green => ManaColor::Green,
        spellwake_engine::mana::ManaType::Colourless => ManaColor::Colourless,
    }
}
pub(super) fn color(value: spellwake_engine::characteristics::Color) -> ManaColor {
    match value {
        spellwake_engine::characteristics::Color::White => ManaColor::White,
        spellwake_engine::characteristics::Color::Blue => ManaColor::Blue,
        spellwake_engine::characteristics::Color::Black => ManaColor::Black,
        spellwake_engine::characteristics::Color::Red => ManaColor::Red,
        spellwake_engine::characteristics::Color::Green => ManaColor::Green,
    }
}
pub(super) fn card_type(value: spellwake_engine::program::CardType) -> PlayerCardType {
    match value {
        spellwake_engine::program::CardType::Land => PlayerCardType::Land,
        spellwake_engine::program::CardType::Artifact => PlayerCardType::Artifact,
        spellwake_engine::program::CardType::Creature => PlayerCardType::Creature,
        spellwake_engine::program::CardType::Enchantment => PlayerCardType::Enchantment,
        spellwake_engine::program::CardType::Kindred => PlayerCardType::Kindred,
        spellwake_engine::program::CardType::Instant => PlayerCardType::Instant,
        spellwake_engine::program::CardType::Sorcery => PlayerCardType::Sorcery,
    }
}
pub(super) fn supertype(value: spellwake_engine::program::Supertype) -> PlayerSupertype {
    match value {
        spellwake_engine::program::Supertype::Basic => PlayerSupertype::Basic,
        spellwake_engine::program::Supertype::Legendary => PlayerSupertype::Legendary,
        spellwake_engine::program::Supertype::Ongoing => PlayerSupertype::Ongoing,
        spellwake_engine::program::Supertype::Snow => PlayerSupertype::Snow,
        spellwake_engine::program::Supertype::World => PlayerSupertype::World,
    }
}
pub(super) fn land_type(value: spellwake_engine::land::LandType) -> PlayerLandType {
    match value {
        spellwake_engine::land::LandType::Cave => PlayerLandType::Cave,
        spellwake_engine::land::LandType::Desert => PlayerLandType::Desert,
        spellwake_engine::land::LandType::Forest => PlayerLandType::Forest,
        spellwake_engine::land::LandType::Gate => PlayerLandType::Gate,
        spellwake_engine::land::LandType::Island => PlayerLandType::Island,
        spellwake_engine::land::LandType::Lair => PlayerLandType::Lair,
        spellwake_engine::land::LandType::Locus => PlayerLandType::Locus,
        spellwake_engine::land::LandType::Mine => PlayerLandType::Mine,
        spellwake_engine::land::LandType::Mountain => PlayerLandType::Mountain,
        spellwake_engine::land::LandType::Plains => PlayerLandType::Plains,
        spellwake_engine::land::LandType::Planet => PlayerLandType::Planet,
        spellwake_engine::land::LandType::PowerPlant => PlayerLandType::PowerPlant,
        spellwake_engine::land::LandType::Sphere => PlayerLandType::Sphere,
        spellwake_engine::land::LandType::Swamp => PlayerLandType::Swamp,
        spellwake_engine::land::LandType::Tower => PlayerLandType::Tower,
        spellwake_engine::land::LandType::Town => PlayerLandType::Town,
        spellwake_engine::land::LandType::Urzas => PlayerLandType::Urzas,
    }
}
pub(super) fn keyword(value: spellwake_engine::characteristics::KeywordAbility) -> PlayerKeyword {
    match value {
        spellwake_engine::characteristics::KeywordAbility::Haste => PlayerKeyword::Haste,
        spellwake_engine::characteristics::KeywordAbility::Vigilance => PlayerKeyword::Vigilance,
    }
}
pub(super) fn phase(value: spellwake_engine::turn::Phase) -> PlayerPhase {
    match value {
        spellwake_engine::turn::Phase::Beginning => PlayerPhase::Beginning,
        spellwake_engine::turn::Phase::PrecombatMain => PlayerPhase::PrecombatMain,
        spellwake_engine::turn::Phase::Combat => PlayerPhase::Combat,
        spellwake_engine::turn::Phase::PostcombatMain => PlayerPhase::PostcombatMain,
        spellwake_engine::turn::Phase::Ending => PlayerPhase::Ending,
    }
}
pub(super) fn step(value: spellwake_engine::turn::Step) -> PlayerStep {
    match value {
        spellwake_engine::turn::Step::Untap => PlayerStep::Untap,
        spellwake_engine::turn::Step::Upkeep => PlayerStep::Upkeep,
        spellwake_engine::turn::Step::Draw => PlayerStep::Draw,
        spellwake_engine::turn::Step::PrecombatMain => PlayerStep::PrecombatMain,
        spellwake_engine::turn::Step::BeginningCombat => PlayerStep::BeginningCombat,
        spellwake_engine::turn::Step::DeclareAttackers => PlayerStep::DeclareAttackers,
        spellwake_engine::turn::Step::DeclareBlockers => PlayerStep::DeclareBlockers,
        spellwake_engine::turn::Step::CombatDamage => PlayerStep::CombatDamage,
        spellwake_engine::turn::Step::EndCombat => PlayerStep::EndCombat,
        spellwake_engine::turn::Step::PostcombatMain => PlayerStep::PostcombatMain,
        spellwake_engine::turn::Step::End => PlayerStep::End,
        spellwake_engine::turn::Step::Cleanup => PlayerStep::Cleanup,
    }
}

pub(super) fn mana_label(value: ManaType) -> &'static str {
    match value {
        ManaType::White => "White",
        ManaType::Blue => "Blue",
        ManaType::Black => "Black",
        ManaType::Red => "Red",
        ManaType::Green => "Green",
        ManaType::Colourless => "Colorless",
    }
}
