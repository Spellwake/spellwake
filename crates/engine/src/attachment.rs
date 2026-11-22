//! Copiable Aura and Equipment definitions and their attachment restrictions.
use crate::{
    characteristics::ObjectCharacteristics,
    object_identity::ObjectZone,
    priority::SeatId,
    program::{BodyTarget, CardType},
};

/// The object or player described by a permanent's enchant ability (CR 702.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnchantRestriction {
    Creature,
    NonblackCreature,
    CreatureYouControl,
    CreatureAnOpponentControls,
    Artifact,
    Enchantment,
    Land,
    Permanent,
    Player,
}
impl EnchantRestriction {
    pub(crate) fn target(self) -> BodyTarget {
        if self == Self::Player {
            BodyTarget::SinglePlayer
        } else {
            BodyTarget::SinglePermanent
        }
    }
    pub(crate) fn permits(self, object: &ObjectCharacteristics, controller: SeatId) -> bool {
        object.zone == ObjectZone::Battlefield
            && match self {
                Self::Creature => object.is_creature(),
                Self::NonblackCreature => {
                    object.is_creature()
                        && !object
                            .current_colors
                            .contains(&crate::characteristics::Color::Black)
                }
                Self::CreatureYouControl => {
                    object.is_creature() && object.controller == Some(controller)
                }
                Self::CreatureAnOpponentControls => {
                    object.is_creature() && object.controller.is_some_and(|seat| seat != controller)
                }
                Self::Artifact => object.has_type(CardType::Artifact),
                Self::Enchantment => object.has_type(CardType::Enchantment),
                Self::Land => object.has_type(CardType::Land),
                Self::Permanent => true,
                Self::Player => false,
            }
    }
}

/// Printed Aura or Equipment subtype and, for Auras, the enchant ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Aura(EnchantRestriction),
    Equipment,
    Fortification,
}

/// Sorcery-speed attach ability and its target and source subtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachAbility {
    Reconfigure,
    Equip,
    Fortify,
}
impl AttachAbility {
    pub(crate) fn kind(self) -> AttachmentKind {
        match self {
            Self::Equip | Self::Reconfigure => AttachmentKind::Equipment,
            Self::Fortify => AttachmentKind::Fortification,
        }
    }
    pub(crate) fn target(self) -> BodyTarget {
        match self {
            Self::Equip | Self::Reconfigure => BodyTarget::SingleCreature,
            Self::Fortify => BodyTarget::SinglePermanent,
        }
    }
    pub(crate) fn host_type(self) -> CardType {
        match self {
            Self::Equip | Self::Reconfigure => CardType::Creature,
            Self::Fortify => CardType::Land,
        }
    }
}
