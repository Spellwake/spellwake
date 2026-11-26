#![allow(dead_code, unused_imports)]
use super::*;
use spellwake_engine::{
    characteristics::{
        ControlRecipient, KeywordAbility, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
    },
    life::{LifeAdjustment, LifeChangeKind},
    program::{PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
};
