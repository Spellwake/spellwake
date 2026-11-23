//! Typed color characteristics. Colorless is an empty set, not a sixth color.
use crate::mana::ManaType;
use std::collections::BTreeSet;

/// The five Magic colors (CR 105.1), independently of mana type or color identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}
impl Color {
    pub const ALL: [Self; 5] = [Self::White, Self::Blue, Self::Black, Self::Red, Self::Green];

    /// Single-bit mask position for this color.
    #[must_use]
    pub const fn bit(self) -> u8 {
        match self {
            Self::White => 1 << 0,
            Self::Blue => 1 << 1,
            Self::Black => 1 << 2,
            Self::Red => 1 << 3,
            Self::Green => 1 << 4,
        }
    }

    /// Color contributed by one represented mana symbol. Colorless mana
    /// contributes no color; generic cost quantities are not mana symbols.
    pub(crate) fn from_mana(value: ManaType) -> Option<Self> {
        match value {
            ManaType::White => Some(Self::White),
            ManaType::Blue => Some(Self::Blue),
            ManaType::Black => Some(Self::Black),
            ManaType::Red => Some(Self::Red),
            ManaType::Green => Some(Self::Green),
            ManaType::Colourless => None,
        }
    }
}
/// Color qualification for one permanent selector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorFilter {
    #[default]
    Any,
    Has(Color),
    /// Match objects that have at least one color in this 5-bit mask (`Color::bit`).
    AnyOf(u8),
    /// Match objects that do not have this color.
    Lacks(Color),
    /// Match objects that have none of the colors in this 5-bit mask (`Color::bit`).
    LacksAll(u8),
    Colorless,
    /// Match objects with at least one color.
    Colored,
    Monocolored,
    Multicolored,
}
impl ColorFilter {
    pub(super) fn matches(self, colors: &BTreeSet<Color>) -> bool {
        match self {
            Self::Any => true,
            Self::Has(color) => colors.contains(&color),
            Self::AnyOf(mask) => colors.iter().any(|c| (c.bit() & mask) != 0),
            Self::Lacks(color) => !colors.contains(&color),
            Self::LacksAll(mask) => colors.iter().all(|c| (c.bit() & mask) == 0),
            Self::Colorless => colors.is_empty(),
            Self::Colored => !colors.is_empty(),
            Self::Monocolored => colors.len() == 1,
            Self::Multicolored => colors.len() >= 2,
        }
    }
}
/// One layer-5 component. Set replaces all prior colors; Add retains them;
/// Remove removes only the specified colors. An empty Set makes an object colorless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorOperation<T> {
    Set(T),
    Add(T),
    Remove(T),
}
impl<T> ColorOperation<T> {
    pub(super) fn map<U>(&self, resolve: impl FnOnce(&T) -> U) -> ColorOperation<U> {
        match self {
            Self::Set(value) => ColorOperation::Set(resolve(value)),
            Self::Add(value) => ColorOperation::Add(resolve(value)),
            Self::Remove(value) => ColorOperation::Remove(resolve(value)),
        }
    }
}
/// Live static color operands are evaluated in the exact current layer pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticColorOperand {
    Literal(BTreeSet<Color>),
    SourceCurrentColors,
}
