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
/// Color qualification for one permanent selector. Compound predicates and
/// resolution-local color choices have not entered this authored language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorFilter {
    #[default]
    Any,
    Has(Color),
    Colorless,
    Monocolored,
    Multicolored,
}
impl ColorFilter {
    pub(super) fn matches(self, colors: &BTreeSet<Color>) -> bool {
        match self {
            Self::Any => true,
            Self::Has(color) => colors.contains(&color),
            Self::Colorless => colors.is_empty(),
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
