//! Universal composable rules toolbox.
//!
//! Provides the orthogonal core primitives shared by spells, abilities,
//! triggers, replacements, and custom cards:
//! - [`env`]: typed resolution binding frame (`ExecEnv`) with LKI snapshots.
//! - [`tags`]: generic object metadata tags/partitions, state tracks, and side-decks.
//! - [`query`]: composable player and object selector algebra.
//! - [`expr`]: composable boolean condition algebra integrated with `ValueExpr`.
//! - [`zone_op`]: unified zone-move / reveal / look / destroy / sacrifice / discard / mill pipeline.
//! - [`mutate`]: unified draw and permanent/object state mutation primitives.
//! - [`continuous`]: unified continuous effect, restriction, replacement, and emblem applicator.
//! - [`choice`]: unified interactive choice engine, history, and `ChoiceMade` event metadata.

pub mod choice;
pub mod continuous;
pub mod env;
pub mod expr;
pub mod life_damage;
pub mod mutate;
pub mod query;
pub mod tags;
pub mod zone_op;

pub use choice::{
    ChoiceCategory, ChoiceRecord, PromptChoiceFrame, PromptChoiceReply, PromptChoiceSpec,
    PromptDomain, ResolvedPromptDomain, SubtypeChoiceKind, validate_prompt_reply,
};
pub use continuous::{ApplyContinuousSpec, ContinuousPayload};
pub use env::{BindingId, ExecEnv, ExecValue, ObjectSnapshot};
pub use expr::ConditionExpr;
pub use life_damage::{
    AdjustLifeSpec, DamageSourceQuery, DamageTargetQuery, DealDamageSpec, LifeOp, MutatePlayerSpec,
    PlayerMutation,
};
pub use mutate::{DrawCardsSpec, MutateObjectSpec, ObjectMutation};
pub use query::{ObjectQuery, PlayerSelector, QuerySlice, QueryZone};
pub use tags::{
    ObjectTagStore, SideDeck, SideDeckKind, SideDeckOpKind, SideDeckOpSpec, SideDeckStore,
    StateTrackKey, StateTrackKeySpec, StateTrackOp, StateTrackScope, StateTrackStore,
    StateTrackValue, TagKey, TagValue,
};
pub use zone_op::{
    GraveyardSemantics, LibraryOrderPos, SelectionConstraint, ZoneController, ZoneDest, ZoneOpSpec,
    ZoneSelection,
};
