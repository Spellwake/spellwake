//! Face-down characteristics applied after copy effects (CR 613.2b, 708.2).
use crate::{
    mana::ManaCost,
    program::{Program, TriggeredAbility, TriggeredAbilityId},
};
use std::{collections::BTreeSet, sync::LazyLock};
/// Public reason and characteristics for a face-down spell or permanent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceDownKind {
    Morph,
    Disguise,
    Manifest,
    Forest,
}
/// A printed special-action permission, evaluated in the prospective face-up state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaceUpAbility {
    Morph(ManaCost),
    Megamorph(ManaCost),
    Disguise(ManaCost),
}
impl FaceUpAbility {
    /// Public face-down characteristics granted by this ability.
    pub fn kind(&self) -> FaceDownKind {
        match self {
            Self::Morph(_) | Self::Megamorph(_) => FaceDownKind::Morph,
            Self::Disguise(_) => FaceDownKind::Disguise,
        }
    }
    /// Printed mana payment for the turn-face-up special action.
    pub fn cost(&self) -> &ManaCost {
        match self {
            Self::Morph(cost) | Self::Megamorph(cost) | Self::Disguise(cost) => cost,
        }
    }
}
impl FaceDownKind {
    /// The copiable public characteristics independent of the card beneath them.
    pub fn definition(self) -> &'static Program {
        static MORPH: LazyLock<Program> =
            LazyLock::new(|| Program::creature(None, 2.into(), 2.into(), BTreeSet::new()));
        static DISGUISE: LazyLock<Program> = LazyLock::new(|| {
            let mut p = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
            p.triggered.push(TriggeredAbility::ward(
                TriggeredAbilityId(1),
                ManaCost::generic(2.into()),
            ));
            p
        });
        static FOREST: LazyLock<Program> =
            LazyLock::new(|| Program::land([crate::land::LandType::Forest].into(), vec![]));
        match self {
            Self::Forest => &FOREST,
            Self::Morph | Self::Manifest => &MORPH,
            Self::Disguise => &DISGUISE,
        }
    }
}
