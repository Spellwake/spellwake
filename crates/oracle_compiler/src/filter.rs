//! Strict zero-regex parsers for `ObjectFilterAst`, `SubjectRefAst`, `ValueExprAst`,
//! `ConditionAst`, `ManaSpecAst`, and `TokenSpecAst`.

use crate::ast::{
    AggregateOp, CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ConditionAst,
    ControllerRelation, GameMarker, ManaSpecAst, ObjectFilterAst, ObjectFilterFlags, PlayerRefAst,
    PlayerStatKind, Rounding, StatBoundAst, StatKind, SubjectRefAst, SupertypeMask, TargetCountAst,
    TokenSpecAst, ValueExprAst, ZoneFilter,
};
use crate::preprocess::{
    is_mana_sequence, is_physical_tabletop_clause, parse_base_pt, split_top_level,
};
use crate::vocab::{
    Vocabulary, lookup_card_type, lookup_color, lookup_supertype, parse_number_word,
};

fn trim_punct(s: &str) -> &str {
    s.trim().trim_end_matches(['.', ',', ';'])
}

/// Strictly parse a noun phrase into a 13-dimension `ObjectFilterAst`.
pub fn parse_object_filter(phrase: &str, vocab: &Vocabulary) -> Result<ObjectFilterAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_filter".to_string());
    }

    let mut flt = ObjectFilterAst::default();
    let mut s = raw
        .to_ascii_lowercase()
        .replace("time lords", "timelords")
        .replace("time lord", "timelord");

    // Repeatedly peel trailing zone, controller/owner, relational, stat-bound, counter, and keyword suffixes.
    // Longest match MUST precede shorter suffix (e.g. "your opponents control" before "opponents control").
    loop {
        let mut changed = false;

        const ZONE_SUFFIXES: &[(&str, ZoneFilter)] = &[
            (
                " from your opponents' graveyards",
                ZoneFilter::OpponentsGraveyards,
            ),
            (
                " in your opponents' graveyards",
                ZoneFilter::OpponentsGraveyards,
            ),
            (
                " from an opponent's graveyard",
                ZoneFilter::OpponentGraveyard,
            ),
            (" in an opponent's graveyard", ZoneFilter::OpponentGraveyard),
            (" in opponents' graveyards", ZoneFilter::OpponentsGraveyards),
            (
                " from that player's graveyard",
                ZoneFilter::ThatPlayerGraveyard,
            ),
            (
                " in that player's graveyard",
                ZoneFilter::ThatPlayerGraveyard,
            ),
            (
                " in target player's graveyard",
                ZoneFilter::TargetPlayerGraveyard,
            ),
            (
                " from defending player's graveyard",
                ZoneFilter::OpponentGraveyard,
            ),
            (
                " in defending player's graveyard",
                ZoneFilter::OpponentGraveyard,
            ),
            (
                " in the chosen player's graveyard",
                ZoneFilter::ThatPlayerGraveyard,
            ),
            (
                " from the chosen player's graveyard",
                ZoneFilter::ThatPlayerGraveyard,
            ),
            (" from your graveyard or from exile", ZoneFilter::Exile),
            (" from a graveyard or from exile", ZoneFilter::Exile),
            (" from your graveyard or exile", ZoneFilter::Exile),
            (" from a graveyard or exile", ZoneFilter::Exile),
            (" you cast from your graveyard", ZoneFilter::YourGraveyard),
            (" you cast from exile", ZoneFilter::Exile),
            (" cast from your graveyard", ZoneFilter::YourGraveyard),
            (" cast from a graveyard", ZoneFilter::AnyGraveyard),
            (" cast from exile", ZoneFilter::Exile),
            (
                " that were put into your graveyard from your library this turn",
                ZoneFilter::YourGraveyard,
            ),
            (" you own in exile and in your graveyard", ZoneFilter::Exile),
            (" in exile and in your graveyard", ZoneFilter::Exile),
            (" from your graveyard", ZoneFilter::YourGraveyard),
            (" in your graveyard", ZoneFilter::YourGraveyard),
            (" from their graveyard", ZoneFilter::ThatPlayerGraveyard),
            (" in their graveyard", ZoneFilter::ThatPlayerGraveyard),
            (" from a player's graveyard", ZoneFilter::AnyGraveyard),
            (" in a player's graveyard", ZoneFilter::AnyGraveyard),
            (" in its owner's graveyard", ZoneFilter::AnyGraveyard),
            (" from its owner's graveyard", ZoneFilter::AnyGraveyard),
            (" from a graveyard", ZoneFilter::AnyGraveyard),
            (" in a graveyard", ZoneFilter::AnyGraveyard),
            (" in any graveyard", ZoneFilter::AnyGraveyard),
            (" from your hand or graveyard", ZoneFilter::YourGraveyard),
            (" from your graveyard or hand", ZoneFilter::YourGraveyard),
            (" from all graveyards", ZoneFilter::AllGraveyards),
            (" in all graveyards", ZoneFilter::AllGraveyards),
            (" in graveyards", ZoneFilter::AllGraveyards),
            (" from graveyards", ZoneFilter::AllGraveyards),
            (" from among the milled cards", ZoneFilter::YourGraveyard),
            (
                " from among the cards milled this way",
                ZoneFilter::YourGraveyard,
            ),
            (" from among cards exiled this way", ZoneFilter::Exile),
            (" from among cards exiled with ~", ZoneFilter::Exile),
            (" from among the exiled cards", ZoneFilter::Exile),
            (
                " from among the nonland cards exiled this way",
                ZoneFilter::Exile,
            ),
            (" from among those nonland cards", ZoneFilter::Exile),
            (
                " from among cards revealed this way",
                ZoneFilter::YourLibrary,
            ),
            (" from among those cards", ZoneFilter::YourLibrary),
            (" from among them", ZoneFilter::YourLibrary),
            (" from it", ZoneFilter::YourHand),
            (" from outside the game", ZoneFilter::Exile),
            (" your opponents own in exile", ZoneFilter::Exile),
            (" an opponent owns in exile", ZoneFilter::Exile),
            (" you own in exile", ZoneFilter::Exile),
            (" from your hand", ZoneFilter::YourHand),
            (" from their hand", ZoneFilter::YourHand),
            (" in your hand", ZoneFilter::YourHand),
            (" from your library", ZoneFilter::YourLibrary),
            (" in your library", ZoneFilter::YourLibrary),
            (" from exile", ZoneFilter::Exile),
            (" in exile", ZoneFilter::Exile),
            (" from the command zone", ZoneFilter::Exile),
            (
                " with the greatest power among creatures on the battlefield",
                ZoneFilter::Battlefield,
            ),
            (" on the battlefield", ZoneFilter::Battlefield),
            (" on the stack", ZoneFilter::Stack),
        ];
        for (suf, z) in ZONE_SUFFIXES {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.zone = Some(*z);
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        // Try trailing stat bound BEFORE CTRL_SUFFIXES so "creature you control with toughness less than
        // the number of basic land types among lands you control" doesn't strip "you control" off the RHS!
        if s.contains(" with ")
            && let Some((prefix, bound)) = peel_stat_bound(&s, vocab)
        {
            flt.stat_bounds.push(bound);
            s = prefix.trim().to_string();
            continue;
        }

        // Peel multi-clause superlative suffixes BEFORE CTRL_SUFFIXES so
        // "... with the greatest power among creatures that player controls" doesn't lose "that player controls" first!
        for superlative in [
            " with the greatest power among creatures they control",
            " with the greatest power among creatures that player controls",
            " with the greatest power among creatures target opponent controls",
            " with the greatest power among creatures on the battlefield",
            " with the greatest mana value among creatures and planeswalkers they control",
            " with the greatest mana value among permanents they control",
            " with the greatest power among permanents they control",
            " with the greatest toughness among creatures they control",
            " with the least power among creatures they control",
            " that are enchanted by auras you control",
            " that are enchanted by an aura you control",
            " enchanted by auras you control",
            " with a legendary equipment attached to it",
        ] {
            if let Some(prefix) = s.strip_suffix(superlative) {
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        const CTRL_SUFFIXES: &[(&str, ControllerRelation)] = &[
            (" your opponents control", ControllerRelation::Opponents),
            (" each opponent controls", ControllerRelation::Opponents),
            (" your team controls", ControllerRelation::YourTeam),
            (" your opponents don't control", ControllerRelation::You),
            (" you cast but don't own", ControllerRelation::You),
            (
                " target opponent controls",
                ControllerRelation::TargetOpponent,
            ),
            (" target player controls", ControllerRelation::TargetPlayer),
            (" that player controls", ControllerRelation::ThatPlayer),
            (" that opponent controls", ControllerRelation::Opponent),
            (" you both own and control", ControllerRelation::You),
            (" target player owns", ControllerRelation::OwnerThatPlayer),
            (
                " that opponent or that planeswalker's controller controls",
                ControllerRelation::ThatPlayer,
            ),
            (
                " defending player controls",
                ControllerRelation::DefendingPlayer,
            ),
            (
                " its controller controls",
                ControllerRelation::ItsController,
            ),
            (" under your control", ControllerRelation::You),
            (" under an opponent's control", ControllerRelation::Opponent),
            (
                " controlled by defending player",
                ControllerRelation::DefendingPlayer,
            ),
            (" controlled by that player", ControllerRelation::ThatPlayer),
            (" an opponent controls", ControllerRelation::Opponent),
            (" you don't control", ControllerRelation::NotYou),
            (" opponents control", ControllerRelation::Opponents),
            (" they control", ControllerRelation::ThatPlayer),
            (" they protect", ControllerRelation::ThatPlayer),
            (" you own or control", ControllerRelation::You),
            (" you control", ControllerRelation::You),
            (" enchanted player controls", ControllerRelation::ThatPlayer),
            (" enchanted player casts", ControllerRelation::ThatPlayer),
            (" that player casts", ControllerRelation::ThatPlayer),
            (" you control but don't own", ControllerRelation::You),
            (" an opponent owns", ControllerRelation::OwnerOpponent),
            (" your opponents own", ControllerRelation::OwnerOpponent),
            (" that player owns", ControllerRelation::OwnerThatPlayer),
            (" they own", ControllerRelation::OwnerThatPlayer),
            (" you don't own", ControllerRelation::OwnerOpponent),
            (" you own", ControllerRelation::OwnerYou),
            (" you cast from exile", ControllerRelation::You),
            (" you cast from your graveyard", ControllerRelation::You),
            (
                " you cast from anywhere other than your hand",
                ControllerRelation::You,
            ),
            (" your opponents cast", ControllerRelation::Opponents),
            (" an opponent casts", ControllerRelation::Opponent),
            (" opponents cast", ControllerRelation::Opponents),
            (" you cast", ControllerRelation::You),
            (" players cast", ControllerRelation::ThatPlayer),
        ];
        for (suf, ctrl) in CTRL_SUFFIXES {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.controller = Some(*ctrl);
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        const REL_SUFFIXES: &[(&str, ObjectFilterFlags, Option<CombatState>)] = &[
            (
                " blocking or blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingOrBlockedBySelf),
            ),
            (
                " that blocked or were blocked by it this turn",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingOrBlockedBySelf),
            ),
            (
                " blocking ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that isn't attacking",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::NotAttacking),
            ),
            (
                " that was dealt damage by ~ this turn",
                ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                None,
            ),
            (
                " dealt damage by ~ this turn",
                ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                None,
            ),
            (
                " that was dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " that entered the battlefield this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " that entered this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " that attacked this turn",
                ObjectFilterFlags::ATTACKED_THIS_TURN,
                None,
            ),
            (
                " that died under your control this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that died this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (" exiled with ~", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (" exiled with it", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (" exiled by ~", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (" exiled by it", ObjectFilterFlags::EXILED_WITH_SELF, None),
            (
                " with another aura attached to it",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (" attached to ~", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (" attached to it", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (
                " attached to them",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (" attached to creatures", ObjectFilterFlags::EMPTY, None),
            (
                " that are enchanted by auras you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (
                " enchanted by auras you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (
                " other than that creature or ~",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " controlled by different players",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " the chosen player controls",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" chosen at random", ObjectFilterFlags::EMPTY, None),
            (
                " of defending player's choice",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " in the pile of their choice",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of a type chosen this way",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" with a name sticker on it", ObjectFilterFlags::EMPTY, None),
            (" other than ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " with the same name as that land",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with the same name as that creature",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with the same name as another card in their hand",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" with the same controller", ObjectFilterFlags::EMPTY, None),
            (" with the same mana value", ObjectFilterFlags::EMPTY, None),
            (
                " that shares a creature type with the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a creature type with that creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color with that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that wasn't put there this combat",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that weren't put onto the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that you've put one or more +1/+1 counters on this turn",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with flying, deathtouch, and/or lifelink",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with total power and toughness 5 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" not cast this way", ObjectFilterFlags::EMPTY, None),
            (
                " not put onto the battlefield this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " not put into your hand this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put there from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (" sacrificed as it entered", ObjectFilterFlags::EMPTY, None),
            (" with an art sticker", ObjectFilterFlags::EMPTY, None),
            (
                " with power or toughness 2 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with power or toughness 3 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with power or toughness 4 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value 1 you discarded this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value less than or equal to that damage",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value less than or equal to that aura",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with a different name", ObjectFilterFlags::EMPTY, None),
            (
                " from an artifact or enchantment source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" from an artifact source", ObjectFilterFlags::EMPTY, None),
            (
                " attacking one of your opponents",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " with the same name as a nontoken permanent",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with two or more card types",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that saddled it this turn", ObjectFilterFlags::EMPTY, None),
            (" that crewed it this turn", ObjectFilterFlags::EMPTY, None),
            (
                " that targets only a single permanent or player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " dealt damage by ~ this turn",
                ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                None,
            ),
            (
                " attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that's attacking that player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " attacking that player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that's attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" dealt damage this way", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " put onto the battlefield this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" not named ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " with the same name as that permanent",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with lesser mana value than the creature that died",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with equal or lesser mana value",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with lesser mana value", ObjectFilterFlags::EMPTY, None),
            (" with power less than ~'s", ObjectFilterFlags::EMPTY, None),
            (" in your party", ObjectFilterFlags::EMPTY, None),
            (
                " exiled from their hand this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " exiled from your hand this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" discarded this way", ObjectFilterFlags::EMPTY, None),
            (" destroyed this way", ObjectFilterFlags::EMPTY, None),
            (" revealed this way", ObjectFilterFlags::EMPTY, None),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (" returned this way", ObjectFilterFlags::EMPTY, None),
            (" tapped this way", ObjectFilterFlags::EMPTY, None),
            (" created this way", ObjectFilterFlags::EMPTY, None),
            (
                " put into a graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that aren't of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that isn't of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the creature type of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the color of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of that color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " with a name chosen for ~",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" other than that creature", ObjectFilterFlags::EMPTY, None),
            (" other than that permanent", ObjectFilterFlags::EMPTY, None),
            (" other than that card", ObjectFilterFlags::EMPTY, None),
            (
                " that didn't attack this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that entered the battlefield under your control this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (" not chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " that wasn't chosen this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" you own in exile", ObjectFilterFlags::EMPTY, None),
            (" that were attached to it", ObjectFilterFlags::EMPTY, None),
            (" that was attached to it", ObjectFilterFlags::EMPTY, None),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that's attached to a creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" attached to a creature", ObjectFilterFlags::EMPTY, None),
            (
                " that are enchanted by an aura you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
            (
                " with an activated ability that isn't a mana ability",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with lesser power", ObjectFilterFlags::EMPTY, None),
            (" with lesser toughness", ObjectFilterFlags::EMPTY, None),
            (
                " with toughness less than ~'s",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't exactly two colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with a legendary equipment attached to it",
                ObjectFilterFlags::EQUIPPED,
                None,
            ),
            (
                " dealt damage this turn by a spider you controlled",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " that share a color with the spell most recently cast this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that has the same name as a card in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as a card in their graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among creatures they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in their hand", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that were put into your graveyard from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that were put there from anywhere this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that was milled this turn", ObjectFilterFlags::EMPTY, None),
            (
                " other than basic land cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that blocked or was blocked by a zombie this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with a sticker on it", ObjectFilterFlags::EMPTY, None),
            (" from a single graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that convoked ~", ObjectFilterFlags::EMPTY, None),
            (" goaded this way", ObjectFilterFlags::EMPTY, None),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into target player's graveyard from their library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same mana value as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that attacked during their controller's last turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with toughness greater than their power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " in graveyards and libraries",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there from your library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into graveyards from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that share a color with that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 1/1",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " dealt damage by enchanted creature this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " with the same name as a card exiled with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that targets only a single creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color or mana value with the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to a player this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value, power, or toughness equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value 4, 5, or 6",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 2/2",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " previously dealt damage by it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " it has dealt damage to this game",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of their choice", ObjectFilterFlags::EMPTY, None),
            (" of your choice", ObjectFilterFlags::EMPTY, None),
            (" of that player's choice", ObjectFilterFlags::EMPTY, None),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" that aren't legendary", ObjectFilterFlags::EMPTY, None),
            (" that isn't legendary", ObjectFilterFlags::EMPTY, None),
            (" that aren't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that isn't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that aren't all colors", ObjectFilterFlags::EMPTY, None),
            (" that isn't all colors", ObjectFilterFlags::EMPTY, None),
            (
                " attached to you",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to target land",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to that creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " with power or toughness 1 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with that spell's mana value",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that targets you", ObjectFilterFlags::EMPTY, None),
            (" that have different names", ObjectFilterFlags::EMPTY, None),
            (" with mana cost {0} or {1}", ObjectFilterFlags::EMPTY, None),
            (" of each basic land type", ObjectFilterFlags::EMPTY, None),
            (" except for ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " that is snow or could produce {c}",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with another aura attached to it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " attacking the last chosen player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " of each of the sacrificed creature's colors",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the chosen kind",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " blocking or being blocked by ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " put onto the battlefield with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attacking your opponents",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that's not historic", ObjectFilterFlags::EMPTY, None),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (" they discarded this way", ObjectFilterFlags::EMPTY, None),
            (" they sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (" they revealed this way", ObjectFilterFlags::EMPTY, None),
            (" they milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " with the chosen name",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" named ~", ObjectFilterFlags::MATCHES_NAMED_AGENDA, None),
            (" with different names", ObjectFilterFlags::EMPTY, None),
            (" with different powers", ObjectFilterFlags::EMPTY, None),
            (
                " with different mana values",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with different colors", ObjectFilterFlags::EMPTY, None),
            (
                " blocking it",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " blocked by it",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (" sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " with no counters on it",
                ObjectFilterFlags::WITHOUT_COUNTERS,
                None,
            ),
            (
                " with no counters on them",
                ObjectFilterFlags::WITHOUT_COUNTERS,
                None,
            ),
            (
                " that shares a creature type with ~",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that shares a color with ~",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" that has an adventure", ObjectFilterFlags::EMPTY, None),
            (" that have an adventure", ObjectFilterFlags::EMPTY, None),
            (
                " with counters on it",
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (
                " with counters on them",
                ObjectFilterFlags::WITH_ANY_COUNTER,
                None,
            ),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (" with {x} in its mana cost", ObjectFilterFlags::EMPTY, None),
            (" with a basic land type", ObjectFilterFlags::EMPTY, None),
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
            (" with greater power", ObjectFilterFlags::EMPTY, None),
            (" that don't have a name", ObjectFilterFlags::EMPTY, None),
            (
                " it blocked this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " blocking it this combat",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " that crewed it this turn",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " that's attached to a permanent",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to a permanent",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " that share a creature type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that share a color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" of their choice", ObjectFilterFlags::EMPTY, None),
            (" of your choice", ObjectFilterFlags::EMPTY, None),
            (
                " that don't have the same name as ~",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " that share a card type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that shares one of those types with it",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" you own", ObjectFilterFlags::EMPTY, None),
            (
                " that are one or more colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value of the chosen quality",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that player or that planeswalker's controller controls",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of that type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " with that name",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " attached to that creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " ~ is blocking",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that dealt damage to you this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" or suspended card", ObjectFilterFlags::EMPTY, None),
            (
                " controlled by the same player",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" other than ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (" other than it", ObjectFilterFlags::EXCLUDE_SELF, None),
            (" not named ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " other than the chosen creature",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (" with different names", ObjectFilterFlags::EMPTY, None),
            (
                " with the same name as that spell",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with the same name as that creature",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with the same name as that permanent",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with the same name as that card",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (
                " with the same name as the exiled card",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" with no abilities", ObjectFilterFlags::EMPTY, None),
            (
                " each with power greater than its base power",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with power greater than its base power",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with power greater than that creature's base power",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " exiled this way",
                ObjectFilterFlags::EXILED_WITH_SELF,
                None,
            ),
            (" revealed this way", ObjectFilterFlags::EMPTY, None),
            (" milled this way", ObjectFilterFlags::EMPTY, None),
            (" discarded this way", ObjectFilterFlags::EMPTY, None),
            (" sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (
                " put into a graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that weren't cast", ObjectFilterFlags::EMPTY, None),
            (" that convoked it", ObjectFilterFlags::MODIFIED, None),
            (
                " you've cast this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " they've cast this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " cast this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that share a land type", ObjectFilterFlags::EMPTY, None),
            (" with enchant creature", ObjectFilterFlags::EMPTY, None),
            (" with enchant permanent", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking alone",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that fought this turn", ObjectFilterFlags::MODIFIED, None),
            (
                " on the battlefield or in the command zone",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" from a colorless source", ObjectFilterFlags::EMPTY, None),
            (" from a creature source", ObjectFilterFlags::EMPTY, None),
            (
                " from an enchantment source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" from an artifact source", ObjectFilterFlags::EMPTY, None),
            (" from a land source", ObjectFilterFlags::EMPTY, None),
            (
                " from a planeswalker source",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" with different powers", ObjectFilterFlags::EMPTY, None),
            (" that wasn't cast", ObjectFilterFlags::EMPTY, None),
            (
                " that weren't cast this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that wasn't cast this way", ObjectFilterFlags::EMPTY, None),
            (" with an even mana value", ObjectFilterFlags::EMPTY, None),
            (" with an odd mana value", ObjectFilterFlags::EMPTY, None),
            (
                " from among the milled cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " of the creature type of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that aren't of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that isn't of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that aren't of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that isn't of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the chosen type",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the creature type of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the color of your choice",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of that color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " with a name chosen for ~",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" other than that creature", ObjectFilterFlags::EMPTY, None),
            (" other than that permanent", ObjectFilterFlags::EMPTY, None),
            (" other than that card", ObjectFilterFlags::EMPTY, None),
            (
                " that didn't attack this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that entered the battlefield under your control this turn",
                ObjectFilterFlags::ENTERED_THIS_TURN,
                None,
            ),
            (" not chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " that wasn't chosen this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " you own in exile and in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" you own in exile", ObjectFilterFlags::EMPTY, None),
            (" that were attached to it", ObjectFilterFlags::EMPTY, None),
            (" that was attached to it", ObjectFilterFlags::EMPTY, None),
            (
                " attached to target permanent you own",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attached to target creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that's attached to a creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" attached to a creature", ObjectFilterFlags::EMPTY, None),
            (
                " that are enchanted by an aura you control",
                ObjectFilterFlags::ENCHANTED,
                None,
            ),
            (" with a mana ability", ObjectFilterFlags::EMPTY, None),
            (
                " with an activated ability that isn't a mana ability",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with lesser power", ObjectFilterFlags::EMPTY, None),
            (" with lesser toughness", ObjectFilterFlags::EMPTY, None),
            (
                " with toughness less than ~'s",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " that isn't exactly two colors",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with a legendary equipment attached to it",
                ObjectFilterFlags::EQUIPPED,
                None,
            ),
            (
                " dealt damage this turn by a spider you controlled",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " that share a color with the spell most recently cast this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that has the same name as a card in your graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same name as a card in their graveyard",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among creatures they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" in their hand", ObjectFilterFlags::EMPTY, None),
            (
                " that's attacking you or a planeswalker you control",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " that were put into your graveyard from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that were put there from anywhere this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that was milled this turn", ObjectFilterFlags::EMPTY, None),
            (
                " other than basic land cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that blocked or was blocked by a zombie this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" with a sticker on it", ObjectFilterFlags::EMPTY, None),
            (" from a single graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " you own that your opponents control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that convoked ~", ObjectFilterFlags::EMPTY, None),
            (" goaded this way", ObjectFilterFlags::EMPTY, None),
            (
                " you controlled as you cast ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into target player's graveyard from their library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the same mana value as that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that attacked during their controller's last turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with toughness greater than their power",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " in graveyards and libraries",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there from your library this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that were put into graveyards from the battlefield this turn",
                ObjectFilterFlags::DIED_THIS_TURN,
                None,
            ),
            (
                " that share a color with that card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that isn't on the battlefield",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 1/1",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " dealt damage by enchanted creature this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (
                " with the same name as a card exiled with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that targets only a single creature",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a color or mana value with the exiled card",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that dealt combat damage to a player this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value, power, or toughness equal to the chosen number",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with mana value 4, 5, or 6",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with base power and toughness 2/2",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " previously dealt damage by it",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " it has dealt damage to this game",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of their choice", ObjectFilterFlags::EMPTY, None),
            (" of your choice", ObjectFilterFlags::EMPTY, None),
            (" of that player's choice", ObjectFilterFlags::EMPTY, None),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" that aren't legendary", ObjectFilterFlags::EMPTY, None),
            (" that isn't legendary", ObjectFilterFlags::EMPTY, None),
            (" that aren't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that isn't enchanted", ObjectFilterFlags::EMPTY, None),
            (" that aren't all colors", ObjectFilterFlags::EMPTY, None),
            (" that isn't all colors", ObjectFilterFlags::EMPTY, None),
            (
                " attached to you",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to target land",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to that creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " with power or toughness 1 or less",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with that spell's mana value",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that targets you", ObjectFilterFlags::EMPTY, None),
            (" that have different names", ObjectFilterFlags::EMPTY, None),
            (" with mana cost {0} or {1}", ObjectFilterFlags::EMPTY, None),
            (" of each basic land type", ObjectFilterFlags::EMPTY, None),
            (" except for ~", ObjectFilterFlags::EXCLUDE_SELF, None),
            (
                " that is snow or could produce {c}",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with another aura attached to it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " attacking the last chosen player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " of each of the sacrificed creature's colors",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " of the chosen kind",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " blocking or being blocked by ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" of an opponent's choice", ObjectFilterFlags::EMPTY, None),
            (" exiled this way", ObjectFilterFlags::EMPTY, None),
            (
                " put onto the battlefield with ~",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " attacking your opponents",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (" that's not historic", ObjectFilterFlags::EMPTY, None),
            (" they exiled this way", ObjectFilterFlags::EMPTY, None),
            (" you exiled this way", ObjectFilterFlags::EMPTY, None),
            (" they discarded this way", ObjectFilterFlags::EMPTY, None),
            (" they sacrificed this way", ObjectFilterFlags::EMPTY, None),
            (" they revealed this way", ObjectFilterFlags::EMPTY, None),
            (" they milled this way", ObjectFilterFlags::EMPTY, None),
            (
                " of the chosen color",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " with the chosen name",
                ObjectFilterFlags::MATCHES_NAMED_AGENDA,
                None,
            ),
            (" chosen this way", ObjectFilterFlags::MODIFIED, None),
            (
                " other than enchanted creature",
                ObjectFilterFlags::EXCLUDE_SELF,
                None,
            ),
            (
                " attached to a creature",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attached to a land",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (" attached to it", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (" attached to ~", ObjectFilterFlags::ATTACHED_TO_SELF, None),
            (
                " attached to an opponent",
                ObjectFilterFlags::ATTACHED_TO_SELF,
                None,
            ),
            (
                " attacking enchanted player",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " attacking you",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::Attacking),
            ),
            (
                " blocking ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockingSelf),
            ),
            (
                " blocked by ~",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (
                " that were put there from the battlefield this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that was put there from the battlefield this turn",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (" that was exiled with ~", ObjectFilterFlags::EMPTY, None),
            (" exiled with ~", ObjectFilterFlags::EMPTY, None),
            (" you discarded this way", ObjectFilterFlags::EMPTY, None),
            (
                " you put into your graveyard this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest power among creatures they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest power among creatures that player controls",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among creatures and planeswalkers they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest mana value among permanents they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with the greatest power among permanents they control",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " with activated abilities that aren't mana abilities",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " that shares a card type with it",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (" with mana value 1 or 2", ObjectFilterFlags::EMPTY, None),
            (" that's one or more colors", ObjectFilterFlags::EMPTY, None),
            (" created with ~", ObjectFilterFlags::EMPTY, None),
            (
                " it's blocking",
                ObjectFilterFlags::EMPTY,
                Some(CombatState::BlockedBySelf),
            ),
            (" not chosen this way", ObjectFilterFlags::EMPTY, None),
            (
                " dealt damage this turn",
                ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                None,
            ),
            (" that blocked this turn", ObjectFilterFlags::EMPTY, None),
            (" cast from a graveyard", ObjectFilterFlags::EMPTY, None),
            (
                " that had counters put on it this way",
                ObjectFilterFlags::EMPTY,
                None,
            ),
            (
                " without mana value of the chosen quality",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that has a -1/-1 counter on it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " that has a +1/+1 counter on it",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (
                " with ice counters on them",
                ObjectFilterFlags::MODIFIED,
                None,
            ),
            (" it can enchant", ObjectFilterFlags::EMPTY, None),
            (
                " that shares a color with it",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that share a color with it",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that shares a creature type with it",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " that share a creature type with it",
                ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                None,
            ),
            (
                " from among the exiled cards",
                ObjectFilterFlags::EMPTY,
                None,
            ),
        ];
        for (suf, flag, combat) in REL_SUFFIXES {
            if let Some(prefix) = s.strip_suffix(suf) {
                flt.flags = flt.flags.union(*flag);
                if let Some(c) = combat {
                    flt.combat = Some(*c);
                }
                s = prefix.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }

        // Flatten relative subtype/type clause INSIDE the loop so trailing controller/zone suffixes
        // like "creature you control that's an Ape or a Monkey" still get peeled cleanly!
        if let Some((left, right)) = s
            .split_once(" that's ")
            .or_else(|| s.split_once(" that is "))
            .or_else(|| s.split_once(" that are "))
        {
            s = format!("{left} {right}");
            continue;
        }
        if let Some((left, right)) = s.split_once(" you control ") {
            flt.controller = Some(ControllerRelation::You);
            s = format!("{left} {right}");
            continue;
        }
        if let Some((left, right)) = s.split_once(" your opponents control ") {
            flt.controller = Some(ControllerRelation::Opponents);
            s = format!("{left} {right}");
            continue;
        }
        if let Some((left, right)) = s.split_once(" an opponent controls ") {
            flt.controller = Some(ControllerRelation::Opponent);
            s = format!("{left} {right}");
            continue;
        }

        // Trailing "whose controller controls <filter>" (Seasinger, Islandhome-style)
        if let Some((left, right)) = s.split_once(" whose controller controls ")
            && !left.trim().is_empty()
            && parse_object_filter(right.trim(), vocab).is_ok()
        {
            s = left.trim().to_string();
            continue;
        }

        // Trailing "except [for] <filter>" exclusion (Flame Sweep, etc.)
        if let Some((left, right)) = s
            .split_once(" except for ")
            .or_else(|| s.split_once(" except "))
            && !left.trim().is_empty()
            && parse_object_filter(right.trim(), vocab).is_ok()
        {
            flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            s = left.trim().to_string();
            continue;
        }

        // Trailing stat bounds: "with power/toughness/mana value N/X or less/greater / less than ..."
        if let Some((prefix, bound)) = peel_stat_bound(&s, vocab) {
            flt.stat_bounds.push(bound);
            s = prefix.trim().to_string();
            continue;
        }

        // Trailing counter predicate: "with/without a <kind> counter on it"
        if let Some((prefix, positive, ckind)) = peel_counter_pred(&s) {
            if positive {
                flt.flags = flt.flags.union(ObjectFilterFlags::WITH_ANY_COUNTER);
                flt.counter_kind = Some(ckind);
            } else {
                flt.flags = flt.flags.union(ObjectFilterFlags::WITHOUT_COUNTERS);
                flt.counter_kind = Some(ckind);
            }
            s = prefix.trim().to_string();
            continue;
        }

        // Trailing literal name predicate: "<filter> named <CardName>"
        if let Some((prefix, name_part)) = s.rsplit_once(" named ")
            && !prefix.trim().is_empty()
            && !name_part.trim().is_empty()
            && name_part.split_whitespace().count() <= 4
        {
            flt.flags = flt.flags.union(ObjectFilterFlags::MATCHES_NAMED_AGENDA);
            s = prefix.trim().to_string();
            continue;
        }

        // Trailing keyword predicate: "with flying" / "without flying" / "with flying or reach"
        if let Some((prefix, positive, kws)) = peel_keyword_pred(&s, vocab) {
            if positive {
                flt.required_keywords.extend(kws);
            } else {
                flt.forbidden_keywords.extend(kws);
            }
            s = prefix.trim().to_string();
            continue;
        }

        break;
    }

    // Flatten relative subtype/type clause: "creature you control that's an Ape or a Monkey"
    if let Some((left, right)) = s
        .split_once(" that's ")
        .or_else(|| s.split_once(" that is "))
        .or_else(|| s.split_once(" that are "))
    {
        s = format!("{left} {right}");
    }

    // Strip leading article
    let core = s
        .strip_prefix("a ")
        .or_else(|| s.strip_prefix("an "))
        .or_else(|| s.strip_prefix("the "))
        .unwrap_or(&s)
        .trim();

    let tokens: Vec<&str> = core
        .split_whitespace()
        .map(|t| t.trim_matches(','))
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return if !flt.is_empty() {
            Ok(flt)
        } else {
            Err(format!("empty_core:{raw}"))
        };
    }

    let mut unknown = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        match tok {
            "or" | "and" | "and/or" | "of" | "the" | "a" | "an" => {}
            "other" | "another" => flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF),
            "permanent" | "permanents" | "source" | "sources" => flt.any_permanent = true,
            "card" | "cards" => flt.any_card = true,
            "spell" | "spells" => flt.any_spell = true,
            "ability" | "abilities" => flt.is_ability = true,
            "activated" | "triggered" => flt.is_ability = true,
            "kicked" => flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED),
            "token" | "tokens" => flt.token = Some(true),
            "nontoken" => flt.token = Some(false),
            "colorless" => flt.color_arity = Some(ColorArity::Colorless),
            "colored" => flt.color_arity = Some(ColorArity::Colored),
            "multicolored" => flt.color_arity = Some(ColorArity::Multicolored),
            "monocolored" => flt.color_arity = Some(ColorArity::Monocolored),
            "tapped" => flt.tapped = Some(true),
            "untapped" => flt.tapped = Some(false),
            "attacking" => {
                flt.combat = Some(match flt.combat {
                    Some(CombatState::Blocking) => CombatState::AttackingOrBlocking,
                    _ => CombatState::Attacking,
                });
            }
            "blocking" => {
                flt.combat = Some(match flt.combat {
                    Some(CombatState::Attacking) => CombatState::AttackingOrBlocking,
                    _ => CombatState::Blocking,
                });
            }
            "blocked" => flt.combat = Some(CombatState::Blocked),
            "unblocked" => flt.combat = Some(CombatState::Unblocked),
            "modified" | "saddled" | "suspected" | "goaded" | "monstrous" | "renowned"
            | "exerted" | "prepared" | "adventure" | "modal" | "silver-bordered" | "acorn"
            | "black-bordered" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED);
            }
            "equipped" => flt.flags = flt.flags.union(ObjectFilterFlags::EQUIPPED),
            "enchanted" => flt.flags = flt.flags.union(ObjectFilterFlags::ENCHANTED),
            "face-down" => flt.flags = flt.flags.union(ObjectFilterFlags::FACE_DOWN),
            "commander" | "commanders" => {
                flt.required_supertypes = flt.required_supertypes.union(SupertypeMask::LEGENDARY);
            }
            "nonland" => flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND),
            "noncreature" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::CREATURE);
            }
            "nonartifact" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::ARTIFACT);
            }
            "nonenchantment" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::ENCHANTMENT);
            }
            "nonplaneswalker" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::PLANESWALKER);
            }
            "nonbattle" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::BATTLE);
            }
            "noninstant" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::INSTANT);
            }
            "nonsorcery" => {
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::SORCERY);
            }
            "nonbasic" => {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::BASIC);
            }
            "nonlegendary" => {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
            }
            "nonsnow" => {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::SNOW);
            }
            "nonattacking" | "nonblocking" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::EMPTY);
            }
            "foretold" | "face-up" | "double-faced" => {
                flt.flags = flt.flags.union(ObjectFilterFlags::EMPTY);
            }
            "nonwhite" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::WHITE),
            "nonblue" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLUE),
            "nonblack" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLACK),
            "nonred" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::RED),
            "nongreen" => flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::GREEN),
            "nonaura" => flt.forbidden_subtypes.push("Aura".to_string()),
            "nonequipment" => flt.forbidden_subtypes.push("Equipment".to_string()),
            "transformed" | "sacrificed" | "exiled" | "discarded" | "revealed" | "milled"
            | "chosen" | "returned" => flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED),
            _ => {
                if let Some(c) = lookup_color(tok) {
                    flt.any_colors = flt.any_colors.union(c);
                } else if let Some(st) = lookup_supertype(tok) {
                    flt.required_supertypes = flt.required_supertypes.union(st);
                } else if let Some(ct) = lookup_card_type(tok) {
                    flt.any_types = flt.any_types.union(ct);
                } else if let Some(rest) = tok.strip_prefix("non-") {
                    if let Some(sub) = vocab.lookup_subtype(rest) {
                        flt.forbidden_subtypes.push(sub.to_string());
                    } else if let Some(ct) = lookup_card_type(rest) {
                        flt.forbidden_types = flt.forbidden_types.union(ct);
                    } else {
                        unknown.push(tok);
                    }
                } else if let Some(sub) = vocab.lookup_subtype(tok) {
                    flt.subtypes.push(sub.to_string());
                } else if parse_base_pt(tok).is_some() {
                    flt.flags = flt.flags.union(ObjectFilterFlags::MODIFIED);
                } else {
                    unknown.push(tok);
                }
            }
        }
        i += 1;
    }

    if !unknown.is_empty() {
        return Err(format!("unknown_filter_tokens:{}", unknown.join(" ")));
    }
    if flt.is_empty() {
        return Err(format!("empty_filter_result:{raw}"));
    }
    Ok(flt)
}

fn peel_stat_bound<'a>(s: &'a str, vocab: &Vocabulary) -> Option<(&'a str, StatBoundAst)> {
    let (idx, tail) = if let Some(i) = s.rfind(" each with ") {
        (i, &s[i + 11..])
    } else if let Some(i) = s.rfind(" that each have ") {
        (i, &s[i + 16..])
    } else {
        let i = s.rfind(" with ")?;
        (i, &s[i + 6..])
    };
    let (stat, rest) = if let Some(r) = tail
        .strip_prefix("base power ")
        .or_else(|| tail.strip_prefix("total power "))
        .or_else(|| tail.strip_prefix("power "))
    {
        (StatKind::Power, r)
    } else if let Some(r) = tail
        .strip_prefix("base toughness ")
        .or_else(|| tail.strip_prefix("total toughness "))
        .or_else(|| tail.strip_prefix("toughness "))
    {
        (StatKind::Toughness, r)
    } else if let Some(r) = tail
        .strip_prefix("total mana value ")
        .or_else(|| tail.strip_prefix("mana value "))
        .or_else(|| tail.strip_prefix("converted mana cost "))
    {
        (StatKind::ManaValue, r)
    } else {
        let r = tail.strip_prefix("loyalty ")?;
        (StatKind::Loyalty, r)
    };

    let (val_tok, op) = if let Some(v) = rest.strip_suffix(" or less") {
        (v.trim(), CompareOp::Le)
    } else if let Some(v) = rest.strip_suffix(" or greater") {
        (v.trim(), CompareOp::Ge)
    } else if let Some(v) = rest.strip_prefix("less than or equal to ") {
        (v.trim(), CompareOp::Le)
    } else if let Some(v) = rest.strip_prefix("greater than or equal to ") {
        (v.trim(), CompareOp::Ge)
    } else if let Some(v) = rest.strip_prefix("less than ") {
        (v.trim(), CompareOp::Lt)
    } else if let Some(v) = rest.strip_prefix("greater than ") {
        (v.trim(), CompareOp::Gt)
    } else if let Some(v) = rest.strip_prefix("equal to ") {
        (v.trim(), CompareOp::Eq)
    } else {
        (rest.trim(), CompareOp::Eq)
    };

    let value = if val_tok == "x" {
        ValueExprAst::ChosenX
    } else if let Ok(n) = val_tok.parse::<i32>() {
        ValueExprAst::Const(n)
    } else if let Ok(expr) = parse_value_expr(val_tok, vocab) {
        expr
    } else {
        return None;
    };

    Some((
        &s[..idx],
        StatBoundAst {
            stat,
            op,
            value: Box::new(value),
        },
    ))
}

fn peel_counter_pred(s: &str) -> Option<(&str, bool, String)> {
    let body = s
        .strip_suffix(" on it")
        .or_else(|| s.strip_suffix(" on them"))?;
    let body = body
        .strip_suffix(" counters")
        .or_else(|| body.strip_suffix(" counter"))?;
    let (idx, positive, after) = if let Some(i) = body.rfind(" without ") {
        (i, false, body[i + 9..].trim())
    } else {
        let i = body.rfind(" with ")?;
        (i, true, body[i + 6..].trim())
    };
    let kind = after
        .strip_prefix("one or more ")
        .or_else(|| after.strip_prefix("a "))
        .or_else(|| after.strip_prefix("an "))
        .unwrap_or(after)
        .trim();
    let ckind = if kind.is_empty() {
        "any".to_string()
    } else {
        kind.to_string()
    };
    Some((&s[..idx], positive, ckind))
}

fn peel_keyword_pred<'a>(s: &'a str, vocab: &Vocabulary) -> Option<(&'a str, bool, Vec<String>)> {
    let (idx, positive, tail) = if let Some(i) = s.rfind(" without ") {
        (i, false, &s[i + 9..])
    } else {
        let i = s.rfind(" with ")?;
        (i, true, &s[i + 6..])
    };
    let kws = parse_keyword_list(tail, vocab)?;
    Some((&s[..idx], positive, kws))
}

/// Parse a comma/`and`/`or`-separated list of known standalone CR 702 keywords.
#[must_use]
pub fn parse_keyword_list(text: &str, vocab: &Vocabulary) -> Option<Vec<String>> {
    let clean = trim_punct(text).to_ascii_lowercase();
    if clean.is_empty() || clean.contains('"') {
        return None;
    }
    // Handle "protection from ..." and "hexproof from ..." inside keyword lists
    if clean.starts_with("protection from ") || clean.starts_with("hexproof from ") {
        return Some(vec![clean]);
    }
    let normalized = clean
        .replace(" and from ", " /from ")
        .replace(", and ", ",")
        .replace(", or ", ",")
        .replace(" and ", ",")
        .replace(" or ", ",");
    let mut out = Vec::new();
    for part in normalized.split(',') {
        let kw = part.trim();
        if kw.is_empty() {
            continue;
        }
        if kw.starts_with("protection from ") || kw.starts_with("hexproof from ") {
            out.push(kw.to_string());
            continue;
        }
        if let Some(arg) = kw
            .strip_prefix("ward ")
            .or_else(|| kw.strip_prefix("toxic "))
            .or_else(|| kw.strip_prefix("afflict "))
            .or_else(|| kw.strip_prefix("afterlife "))
            .or_else(|| kw.strip_prefix("annihilator "))
            .or_else(|| kw.strip_prefix("bushido "))
            .or_else(|| kw.strip_prefix("firebending "))
            .or_else(|| kw.strip_prefix("frenzy "))
            .or_else(|| kw.strip_prefix("poisonous "))
            .or_else(|| kw.strip_prefix("rampage "))
            .or_else(|| kw.strip_prefix("renown "))
            .or_else(|| kw.strip_prefix("absorb "))
            .or_else(|| kw.strip_prefix("modular "))
            .or_else(|| kw.strip_prefix("crew "))
            .or_else(|| kw.strip_prefix("saddle "))
            .or_else(|| kw.strip_prefix("ripple "))
            .or_else(|| kw.strip_prefix("equip "))
            .or_else(|| kw.strip_prefix("dredge "))
            .or_else(|| kw.strip_prefix("prowl "))
            .or_else(|| kw.strip_prefix("miracle "))
            .or_else(|| kw.strip_prefix("affinity for "))
            .or_else(|| kw.strip_prefix("freerunning "))
            .or_else(|| kw.strip_prefix("sticker kicker "))
            .or_else(|| kw.strip_prefix("bloodthirst "))
            .or_else(|| kw.strip_prefix("encore "))
            .or_else(|| kw.strip_prefix("flashback "))
            .or_else(|| kw.strip_prefix("cycling "))
            .or_else(|| kw.strip_prefix("madness "))
            .or_else(|| kw.strip_prefix("kicker "))
            .or_else(|| kw.strip_prefix("buyback "))
            .or_else(|| kw.strip_prefix("overload "))
            .or_else(|| kw.strip_prefix("evoke "))
            .or_else(|| kw.strip_prefix("dash "))
            .or_else(|| kw.strip_prefix("unearth "))
            .or_else(|| kw.strip_prefix("escape "))
            .or_else(|| kw.strip_prefix("embalm "))
            .or_else(|| kw.strip_prefix("eternalize "))
            .or_else(|| kw.strip_prefix("plot "))
            .or_else(|| kw.strip_prefix("warp "))
            .or_else(|| kw.strip_prefix("foretell "))
            .or_else(|| kw.strip_prefix("suspend "))
            .or_else(|| kw.strip_prefix("ninjutsu "))
            .or_else(|| kw.strip_prefix("spectacle "))
            .or_else(|| kw.strip_prefix("blitz "))
            .or_else(|| kw.strip_prefix("disturb "))
            .or_else(|| kw.strip_prefix("scavenge "))
            .or_else(|| kw.strip_prefix("morph "))
            .or_else(|| kw.strip_prefix("megamorph "))
            .or_else(|| kw.strip_prefix("disguise "))
            .or_else(|| kw.strip_prefix("squad "))
            .or_else(|| kw.strip_prefix("offspring "))
            .or_else(|| kw.strip_prefix("web-slinging "))
            .or_else(|| kw.strip_prefix("outlast "))
            .or_else(|| kw.strip_prefix("sneak "))
            .or_else(|| kw.strip_prefix("fabricate "))
            .or_else(|| kw.strip_prefix("casualty "))
            .or_else(|| kw.strip_prefix("backup "))
            .or_else(|| kw.strip_prefix("awaken "))
            .or_else(|| kw.strip_prefix("devour "))
            && !arg.is_empty()
        {
            out.push(kw.to_string());
            continue;
        }
        if !vocab.is_evergreen_keyword(kw)
            && !matches!(
                kw,
                "foretell"
                    | "awaken"
                    | "flashback"
                    | "escape"
                    | "scavenge"
                    | "unearth"
                    | "madness"
                    | "cycling"
                    | "kicker"
                    | "evoke"
                    | "blitz"
                    | "dash"
                    | "disturb"
                    | "embalm"
                    | "eternalize"
                    | "encore"
                    | "plot"
                    | "warp"
                    | "suspend"
                    | "buyback"
                    | "overload"
                    | "spectacle"
                    | "surge"
                    | "prowl"
                    | "miracle"
                    | "cascade"
                    | "convoke"
                    | "delve"
                    | "improvise"
                    | "assist"
                    | "demonstrate"
                    | "storm"
                    | "retrace"
                    | "jump-start"
                    | "dredge"
                    | "transmute"
                    | "ninjutsu"
                    | "mutate"
                    | "morph"
                    | "disguise"
                    | "emerge"
                    | "toxic"
                    | "infect"
                    | "wither"
                    | "enlist"
                    | "backup"
                    | "saddle"
                    | "crew"
                    | "partner"
                    | "companion"
                    | "prototype"
                    | "freerunning"
                    | "offspring"
                    | "squad"
                    | "impending"
                    | "exhaust"
                    | "bargain"
                    | "cleave"
                    | "casualty"
                    | "ravenous"
                    | "craft"
                    | "spree"
                    | "gift"
                    | "harmonize"
                    | "mobilize"
                    | "mayhem"
                    | "vanishing"
                    | "fading"
                    | "echo"
                    | "phasing"
                    | "flanking"
                    | "bushido"
                    | "rampage"
                    | "banding"
                    | "horsemanship"
                    | "shadow"
                    | "fear"
                    | "intimidate"
                    | "skulk"
                    | "shroud"
                    | "split second"
                    | "riot"
                    | "unleash"
                    | "undying"
                    | "persist"
                    | "dethrone"
                    | "melee"
                    | "myriad"
                    | "exploit"
                    | "exalted"
                    | "extort"
                    | "evolve"
                    | "graft"
                    | "modular"
                    | "sunburst"
                    | "reconfigure"
                    | "bestow"
                    | "annihilator"
                    | "devoid"
                    | "ingest"
                    | "prowess"
                    | "training"
                    | "mentor"
                    | "renown"
                    | "fabricate"
                    | "afterlife"
                    | "ascend"
                    | "entwine"
                    | "replicate"
                    | "rebound"
                    | "cipher"
                    | "splice"
                    | "forecast"
                    | "haunt"
                    | "soulbond"
                    | "soulshift"
                    | "changeling"
                    | "champion"
                    | "hideaway"
                    | "affinity"
                    | "undaunted"
            )
        {
            return None;
        }
        out.push(kw.to_string());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Parse a player reference phrase.
#[must_use]
pub fn parse_player_ref(phrase: &str) -> Option<PlayerRefAst> {
    match trim_punct(phrase).to_ascii_lowercase().as_str() {
        "you" => Some(PlayerRefAst::You),
        "target player" => Some(PlayerRefAst::TargetPlayer),
        "target opponent" => Some(PlayerRefAst::TargetOpponent),
        "each player" => Some(PlayerRefAst::EachPlayer),
        "each opponent" => Some(PlayerRefAst::EachOpponent),
        "each other opponent" => Some(PlayerRefAst::EachOtherOpponent),
        "each other player" => Some(PlayerRefAst::EachOtherPlayer),
        "an opponent" | "an opponent chosen at random" => Some(PlayerRefAst::AnyOpponent),
        "a player" | "any player" | "a player of your choice" | "each friend" | "each foe" => {
            Some(PlayerRefAst::AnyPlayer)
        }
        "your team" | "each of your teammates" => Some(PlayerRefAst::You),
        "that player"
        | "they"
        | "that player or that permanent's controller"
        | "that player or that planeswalker's controller" => Some(PlayerRefAst::ThatPlayer),
        "defending player" => Some(PlayerRefAst::DefendingPlayer),
        "active player" | "the active player" => Some(PlayerRefAst::ActivePlayer),
        "enchanted player" => Some(PlayerRefAst::EnchantedPlayer),
        "its controller" => Some(PlayerRefAst::ItsController),
        "its owner" => Some(PlayerRefAst::ItsOwner),
        "that creature's controller"
        | "the creature's controller"
        | "that creature's or spell's controller"
        | "that permanent's controller"
        | "that land's controller"
        | "the land's controller"
        | "that artifact's controller"
        | "that enchantment's controller"
        | "that spell's controller"
        | "the controller of that spell" => Some(PlayerRefAst::ThatObjectController),
        "each opponent with no cards in hand" => Some(PlayerRefAst::EachOpponent),
        "each player ~ attacked this turn" => Some(PlayerRefAst::EachPlayer),
        "the player to your right" | "the player to your left" => Some(PlayerRefAst::AnyOpponent),
        "the chosen player"
        | "the player"
        | "those players"
        | "that opponent"
        | "that attacking player" => Some(PlayerRefAst::ThatPlayer),
        "their controller"
        | "that player's controller"
        | "~'s controller"
        | "enchanted permanent's controller"
        | "enchanted creature's controller"
        | "enchanted artifact's controller"
        | "equipped creature's controller"
        | "the source's controller"
        | "that source's controller"
        | "that spell or ability's controller" => Some(PlayerRefAst::ThatObjectController),
        "the exiled card's owner"
        | "that creature's owner"
        | "that permanent's owner"
        | "that card's owner" => Some(PlayerRefAst::ItsOwner),
        "players" => Some(PlayerRefAst::EachPlayer),
        "your opponents"
        | "each of your opponents"
        | "each of those opponents"
        | "any number of target opponents"
        | "each opponent who didn't"
        | "two target opponents" => Some(PlayerRefAst::EachOpponent),
        "each player who doesn't have max speed"
        | "each player who searched a library this way"
        | "each player who received no votes"
        | "each player who chose that number"
        | "each player who discarded a card with the greatest mana value among cards discarded this way"
        | "each player other than its controller"
        | "each of those players"
        | "each of them" => Some(PlayerRefAst::EachPlayer),
        "each opponent who didn't sacrifice a permanent or discard a card this way" => {
            Some(PlayerRefAst::EachOpponent)
        }
        "the player whose turn it is" => Some(PlayerRefAst::ActivePlayer),
        "the player with the most life" => Some(PlayerRefAst::AnyPlayer),
        "another one of your opponents chosen at random" => Some(PlayerRefAst::AnyOpponent),
        "the owner of target permanent" => Some(PlayerRefAst::ItsOwner),
        "enchanted opponent" => Some(PlayerRefAst::EnchantedPlayer),
        "any opponent" | "target opponent chosen at random" => Some(PlayerRefAst::TargetOpponent),
        "target player other than its controller"
        | "target player other than ~'s owner"
        | "another target player"
        | "target player who was dealt combat damage by ~ this turn"
        | "target player dealt damage by ~ this turn"
        | "two target players"
        | "up to two target players" => Some(PlayerRefAst::TargetPlayer),
        "another target opponent" => Some(PlayerRefAst::TargetOpponent),
        "the controller of target instant or sorcery spell" => {
            Some(PlayerRefAst::ThatObjectController)
        }
        _ => None,
    }
}

/// Parse a subject, target, or group phrase into a typed `SubjectRefAst`.
pub fn parse_subject_or_target(phrase: &str, vocab: &Vocabulary) -> Result<SubjectRefAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_subject".to_string());
    }
    let low = raw.to_ascii_lowercase();

    match low.as_str() {
        "~" => return Ok(SubjectRefAst::SelfObject),
        "it"
        | "itself"
        | "he"
        | "she"
        | "him"
        | "her"
        | "himself"
        | "herself"
        | "them"
        | "themselves"
        | "that creature"
        | "that permanent"
        | "that card"
        | "that spell"
        | "that spell or ability"
        | "that ability"
        | "that artifact"
        | "that enchantment"
        | "that land"
        | "that planeswalker"
        | "that token"
        | "that aura"
        | "that equipment"
        | "those creatures"
        | "those permanents"
        | "those cards"
        | "those tokens"
        | "both creatures"
        | "the exiled card"
        | "the exiled cards"
        | "the other exiled cards"
        | "the sacrificed creature"
        | "the sacrificed permanent"
        | "the discarded card"
        | "the revealed card"
        | "the revealed cards"
        | "the chosen creature"
        | "the chosen card"
        | "the chosen cards"
        | "the chosen player"
        | "the token"
        | "the rest"
        | "the milled card"
        | "the copied spell"
        | "the copies"
        | "that permanent or player"
        | "that creature or planeswalker"
        | "the player or planeswalker that creature is attacking"
        | "the player or planeswalker it's attacking"
        | "the amassed army"
        | "the bottom card of your library"
        | "target face-up exiled card"
        | "each of the chosen creatures"
        | "each of those creatures"
        | "each of them"
        | "your commander"
        | "a commander you control"
        | "the player you chose"
        | "one of them"
        | "one of those creatures"
        | "one of those cards"
        | "two of those cards"
        | "the other"
        | "those auras"
        | "those lands"
        | "those artifacts"
        | "the chosen permanents"
        | "that wall"
        | "the top card of your graveyard"
        | "the top creature card of your graveyard"
        | "the top creature card of defending player's graveyard"
        | "the top card of their library"
        | "the top card of your library"
        | "all permanents in the pile of their choice"
        | "one of them chosen at random"
        | "any of them that are walls"
        | "a random creature card from among them"
        | "each of those permanents and/or players"
        | "the first creature"
        | "the creature you chose"
        | "the creature your opponent chose"
        | "all cards they exiled this way"
        | "all cards you exiled this way"
        | "the cards exiled this way"
        | "one of the exiled cards"
        | "the rest of the exiled cards"
        | "the rest of the revealed cards"
        | "all other cards revealed this way"
        | "a card from the chosen pile"
        | "the nonland cards revealed this way"
        | "the top two cards of that player's library"
        | "either of them"
        | "target permanent or suspended card" => {
            return Ok(SubjectRefAst::Anaphora(low));
        }
        "enchanted creature"
        | "enchanted permanent"
        | "enchanted land"
        | "enchanted artifact"
        | "enchanted enchantment"
        | "fortified land"
        | "equipped creature" => return Ok(SubjectRefAst::AttachedHost(low)),
        "any target"
        | "any other target"
        | "another target"
        | "any target chosen at random"
        | "any target of an opponent's choice"
        | "target creature or player"
        | "target opponent or creature"
        | "target player or battle"
        | "one or two targets"
        | "two targets"
        | "three targets"
        | "x targets"
        | "up to x targets"
        | "up to two targets"
        | "up to three targets"
        | "up to four targets"
        | "up to five targets"
        | "up to six targets"
        | "up to two other targets"
        | "up to three other targets"
        | "target opponent, creature an opponent controls, or planeswalker an opponent controls"
        | "target creature token, player, or planeswalker"
        | "target planeswalker that player controls or battle that player protects"
        | "up to one target creature, up to one target player, and/or up to one target planeswalker" =>
        {
            return Ok(SubjectRefAst::AnyTarget);
        }
        "target player or planeswalker" | "up to one target player or planeswalker" => {
            return Ok(SubjectRefAst::TargetPlayerOrPlaneswalker {
                opponent_only: false,
            });
        }
        "target opponent or planeswalker" => {
            return Ok(SubjectRefAst::TargetPlayerOrPlaneswalker {
                opponent_only: true,
            });
        }
        "the first spell you cast each turn" => {
            return Ok(SubjectRefAst::Anaphora(low));
        }
        _ => {}
    }

    // Universal "any target that isn't a <flt>" (Wrathful Red Dragon, Wrathful Raptors)
    if let Some(excl) = low.strip_prefix("any target that isn't ")
        && parse_object_filter(excl, vocab).is_ok()
    {
        return Ok(SubjectRefAst::AnyTarget);
    }

    // Universal "that <subtype/type>" and "one of those <subtype/type>" anaphora
    if let Some(rest) = low
        .strip_prefix("that ")
        .or_else(|| low.strip_prefix("one of those "))
        && !rest.contains(" controls")
        && !rest.contains(" owns")
        && parse_object_filter(rest, vocab).is_ok()
    {
        return Ok(SubjectRefAst::Anaphora(low));
    }

    if let Some(p) = parse_player_ref(raw) {
        return Ok(SubjectRefAst::Player(p));
    }

    // Composed multi-subject references with explicit second-branch quantifier/target/player
    // (must run BEFORE `strip_prefix("each ")` / `strip_prefix("target ")` / `strip_prefix("up to ")`):
    // e.g. "each creature and each player", "each opponent and each creature and planeswalker they control",
    // "up to one target land and up to one target creature", "target artifact and target creature".
    for (needle, keep_prefix) in [
        (" and/or ", ""),
        (" and each ", "each "),
        (", and each ", "each "),
        (" and up to ", "up to "),
        (", and up to ", "up to "),
        (" and another target ", "another target "),
        (" and target ", "target "),
        (", and target ", "target "),
        (" or target ", "target "),
        (" and all ", "all "),
    ] {
        if let Some((left, right)) = low.split_once(needle) {
            let l = left.trim();
            let r_full = format!("{keep_prefix}{}", right.trim());
            if !l.is_empty()
                && let (Ok(ls), Ok(rs)) = (
                    parse_subject_or_target(l, vocab),
                    parse_subject_or_target(&r_full, vocab),
                )
            {
                let mut parts = match ls {
                    SubjectRefAst::Compound(v) => v,
                    other => vec![other],
                };
                match rs {
                    SubjectRefAst::Compound(v) => parts.extend(v),
                    other => parts.push(other),
                }
                return Ok(SubjectRefAst::Compound(parts));
            }
        }
    }

    // "each of up to N/X [other] target <rest>" -> delegate to inner target
    if let Some(rest) = low.strip_prefix("each of ") {
        return parse_subject_or_target(rest, vocab);
    }

    // "another target <rest>"
    if let Some(rest) = low.strip_prefix("another target ") {
        let mut subj = finish_target(rest, TargetCountAst::Exact(1), vocab)?;
        if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
            filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(subj);
    }

    // "one or more target <rest>" / "one or two target <rest>" / "one, two, or three target <rest>"
    if let Some(rest) = low.strip_prefix("one or more target ") {
        return finish_target(rest, TargetCountAst::UpTo(99), vocab);
    }
    if let Some(rest) = low.strip_prefix("one or two target ") {
        return finish_target(rest, TargetCountAst::UpTo(2), vocab);
    }
    if let Some(rest) = low.strip_prefix("one, two, or three target ") {
        return finish_target(rest, TargetCountAst::UpTo(3), vocab);
    }

    // "up to N/X [other] target <rest>" OR non-targeting "up to N/X <flt>" (Gix's Command, Nihiloor, Krenko's Buzzcrusher, SOLDIER Military Program)
    if let Some(after) = low.strip_prefix("up to ") {
        let (cnt_tok, rest, excl_self) = if let Some((c, r)) = after.split_once(" other target ") {
            (c, r, true)
        } else if let Some((c, r)) = after.split_once(" target ") {
            (c, r, false)
        } else {
            ("", "", false)
        };
        if !cnt_tok.is_empty() {
            let count = if cnt_tok == "x" || cnt_tok == "that many" {
                TargetCountAst::UpToX
            } else if let Some(n) = parse_number_word(cnt_tok)
                && n >= 0
            {
                TargetCountAst::UpTo(u16::try_from(n).unwrap_or(1))
            } else {
                return Err(format!("bad_upto_target:{raw}"));
            };
            let mut subj = finish_target(rest, count, vocab)?;
            if excl_self && let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
                filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Ok(subj);
        }
        // Non-targeting "up to N/X/that many <flt>"
        if let Some(flt_raw) = after.strip_prefix("that many ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(SubjectRefAst::Group(Box::new(flt)));
        }
        if let Some((c, flt_raw)) = after.split_once(' ')
            && (c == "x" || parse_number_word(c).is_some())
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(SubjectRefAst::Group(Box::new(flt)));
        }
    }
    // Non-targeting "any number of <flt>" (Blessing of Frost, Nahiri's Resolve, Angelic Aberration)
    if let Some(flt_raw) = low.strip_prefix("any number of ")
        && !flt_raw.starts_with("target ")
        && !flt_raw.starts_with("other target ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }
    // "the first <flt> you cast each turn" (Peri Brown, Maelstrom Nexus, etc.)
    if let Some(flt_raw) = low
        .strip_prefix("the first ")
        .and_then(|r| r.strip_suffix(" each turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }

    // "any number of [other] target <rest>"
    if let Some(rest) = low.strip_prefix("any number of other target ") {
        let mut subj = finish_target(rest, TargetCountAst::AnyNumber, vocab)?;
        if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
            filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        }
        return Ok(subj);
    }
    if let Some(rest) = low.strip_prefix("any number of target ") {
        return finish_target(rest, TargetCountAst::AnyNumber, vocab);
    }

    // "<N|X> [other] target <rest>"
    if let Some((cnt_tok, rest)) = low.split_once(" other target ") {
        let count = if cnt_tok == "x" {
            Some(TargetCountAst::ChosenX)
        } else if let Some(n) = parse_number_word(cnt_tok)
            && n > 0
        {
            Some(TargetCountAst::Exact(u16::try_from(n).unwrap_or(1)))
        } else {
            None
        };
        if let Some(c) = count {
            let mut subj = finish_target(rest, c, vocab)?;
            if let SubjectRefAst::TargetObject { ref mut filter, .. } = subj {
                filter.flags = filter.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
            }
            return Ok(subj);
        }
    }
    if let Some((cnt_tok, rest)) = low.split_once(" target ") {
        if cnt_tok == "x" {
            return finish_target(rest, TargetCountAst::ChosenX, vocab);
        }
        if let Some(n) = parse_number_word(cnt_tok)
            && n > 0
        {
            return finish_target(
                rest,
                TargetCountAst::Exact(u16::try_from(n).unwrap_or(1)),
                vocab,
            );
        }
    }

    if let Some(rest) = low.strip_prefix("target ") {
        return finish_target(rest, TargetCountAst::Exact(1), vocab);
    }

    if let Some(rest) = low.strip_prefix("each player who ")
        && let Ok(cond) = parse_condition(&format!("a player {rest}"), vocab).or_else(|_| {
            rest.strip_prefix("controls ")
                .ok_or_else(String::new)
                .and_then(|f| parse_condition(&format!("you control {f}"), vocab))
        })
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachPlayer,
            condition: Box::new(cond),
        });
    }
    if let Some(rest) = low.strip_prefix("each opponent who ")
        && let Ok(cond) = parse_condition(&format!("an opponent {rest}"), vocab).or_else(|_| {
            rest.strip_prefix("controls ")
                .ok_or_else(String::new)
                .and_then(|f| parse_condition(&format!("an opponent controls {f}"), vocab))
        })
    {
        return Ok(SubjectRefAst::PlayerMatching {
            player: PlayerRefAst::EachOpponent,
            condition: Box::new(cond),
        });
    }

    if let Some(rest) = low.strip_prefix("each other ") {
        let mut flt = parse_object_filter(rest, vocab)?;
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(SubjectRefAst::Each(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("each ") {
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(SubjectRefAst::Each(Box::new(flt)));
    }

    if let Some(rest) = low.strip_prefix("all other ") {
        let mut flt = parse_object_filter(rest, vocab)?;
        flt.flags = flt.flags.union(ObjectFilterFlags::EXCLUDE_SELF);
        return Ok(SubjectRefAst::All(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("all ") {
        let flt = parse_object_filter(rest, vocab)?;
        return Ok(SubjectRefAst::All(Box::new(flt)));
    }
    if let Some(rest) = low.strip_prefix("those ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(SubjectRefAst::Group(Box::new(flt)));
    }

    let first_err = match parse_object_filter(raw, vocab) {
        Ok(flt) => return Ok(SubjectRefAst::Group(Box::new(flt))),
        Err(e) => e,
    };

    // Composed multi-subject references: "you and permanents you control",
    // "you or a permanent you control", "target artifact and target creature".
    // Bounded to avoid exponential backtracking on long non-subject clauses.
    if raw.len() <= 84 && (low.matches(" and ").count() + low.matches(" or ").count()) <= 2 {
        for sep in [", and ", " and ", " or "] {
            if let Some((left, right)) = low.split_once(sep) {
                let l = left.trim();
                let r = right.trim();
                if !l.is_empty()
                    && !r.is_empty()
                    && let (Ok(ls), Ok(rs)) = (
                        parse_subject_or_target(l, vocab),
                        parse_subject_or_target(r, vocab),
                    )
                {
                    let mut parts = match ls {
                        SubjectRefAst::Compound(v) => v,
                        other => vec![other],
                    };
                    match rs {
                        SubjectRefAst::Compound(v) => parts.extend(v),
                        other => parts.push(other),
                    }
                    return Ok(SubjectRefAst::Compound(parts));
                }
            }
        }
    }

    Err(first_err)
}

fn finish_target(
    rest: &str,
    count: TargetCountAst,
    vocab: &Vocabulary,
) -> Result<SubjectRefAst, String> {
    match rest.trim() {
        "player" | "players" => Ok(SubjectRefAst::TargetPlayer {
            opponent_only: false,
            count,
        }),
        "opponent" | "opponents" => Ok(SubjectRefAst::TargetPlayer {
            opponent_only: true,
            count,
        }),
        other => {
            let flt = parse_object_filter(other, vocab)?;
            Ok(SubjectRefAst::TargetObject {
                filter: Box::new(flt),
                count,
            })
        }
    }
}

/// Strictly parse a dynamic numeric expression into `ValueExprAst`.
pub fn parse_value_expr(phrase: &str, vocab: &Vocabulary) -> Result<ValueExprAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_value".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = low_full
        .strip_suffix(" as this ability resolves")
        .or_else(|| low_full.strip_suffix(" as this spell resolves"))
        .or_else(|| low_full.strip_suffix(" as you cast ~"))
        .or_else(|| low_full.strip_suffix(" as you cast this spell"))
        .unwrap_or(&low_full)
        .trim()
        .to_string();

    if low == "x" {
        return Ok(ValueExprAst::ChosenX);
    }
    if low == "twice x" || low == "2x" || low == "two times x" {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(2)),
            Box::new(ValueExprAst::ChosenX),
        ));
    }
    if low == "four times x" {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(4)),
            Box::new(ValueExprAst::ChosenX),
        ));
    }
    if low == "five times x" {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(5)),
            Box::new(ValueExprAst::ChosenX),
        ));
    }
    if low == "its base power" {
        return Ok(ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            stat: StatKind::Power,
        });
    }
    if low == "its base toughness" {
        return Ok(ValueExprAst::ObjectStat {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            stat: StatKind::Toughness,
        });
    }
    if low == "that much"
        || low == "that many"
        || low == "that amount"
        || low == "that amount of"
        || low == "that excess damage"
        || low == "the excess damage dealt this way"
        || low == "the amount of excess damage dealt this way"
        || low == "the number of attractions you've visited this turn"
        || low == "the number of cards looked at while scrying this way"
        || low == "the life total of an opponent with the most life"
        || low == "the last chosen number"
        || low == "that number"
        || low == "the total power of the exiled cards"
        || low == "their total toughness"
        || low == "the number of untapped lands they controlled at the beginning of this turn"
        || low == "the amount of {e} paid this way"
        || low == "the total amount of noncombat damage dealt to your opponents this turn"
        || low == "the amount of mana that player paid this way"
        || low == "the highest number"
        || low == "the highest number you noted for cards named ~"
        || low == "the greatest number noted for it this turn"
        || low == "the total power of all stickers on permanents you control"
        || low == "the damage dealt to you this turn"
        || low == "the other result"
        || low == "the life you've lost this turn"
        || low == "the total life lost by your opponents this turn"
        || low == "the difference"
        || low == "the difference between the chosen creatures' powers"
        || low == "the result"
        || low == "that result"
        || low == "the excess"
        || low == "the amount of life you gained"
        || low == "the amount of life you gained this turn"
        || low == "the number of cards you've drawn this turn"
        || low == "the number of cards you drew this turn"
        || low == "the greatest number of cards an opponent has drawn this turn"
        || low
            == "the number of nontoken creatures you had enter the battlefield under your control this turn"
        || low == "the number of colors of mana spent to cast ~"
        || low == "the number of colors of mana spent to cast it"
        || low == "the number of colors that spell is"
        || low == "the number of times ~ has mutated"
        || low == "the number of spells you've cast this turn"
        || low == "the number of spells cast this turn"
        || low == "the number of spells you've cast from anywhere other than your hand this turn"
        || low == "the number of noncreature spells you've cast this turn"
        || low == "the number of times you've cast your commander from the command zone this game"
        || low == "the number of colors in your commanders' color identity"
        || low == "the number of colors in your commander's color identity"
        || low == "the amount of excess damage dealt to that creature this way"
        || low == "the excess damage dealt to that creature this way"
        || low == "the excess damage dealt to that permanent this way"
        || low == "that excess damage"
        || low == "the amount of excess damage dealt this way"
        || low == "the number of times it was kicked"
        || low == "the number of times that spell was kicked"
        || low == "the number of times it's been cast from the command zone this game"
        || low == "the number of opponents who control more creatures than you"
        || low == "the number of times this ability has resolved this turn"
        || low == "the total mana value of those cards"
        || low == "the toughness of the creature you sacrificed this way"
        || low == "the greatest power among other creatures you control"
        || low == "the total toughness of creatures you control"
        || low == "the number of treasure tokens you created this turn"
        || low == "the number of cards that player has drawn this turn"
        || low == "the power of that card or creature"
        || low == "that wall's mana value"
        || low == "that equipment's mana value"
        || low == "the number of times ~ was kicked"
        || low == "the number of tokens you created this turn"
        || low == "the number of counters it had on it"
        || low == "the number of unlocked doors among rooms you control"
        || low == "the number of creatures those opponents control"
        || low == "the life paid as ~ entered"
        || low == "the number of different kinds of counters among permanents you control"
        || low == "the total mana value of all cards revealed this way"
        || low == "the number of opponents who control more lands than you"
        || low == "the number of opponents who control an artifact"
        || low == "the number of opponents who investigated this way"
        || low == "the number of players you attacked with a creature this turn"
        || low == "the number of opponents who have three or more poison counters"
        || low == "the total of those results"
        || low == "the difference between those players' life totals"
        || low == "the number of different color pairs among permanents you control"
        || low
            == "the number of your opponents who were dealt combat damage by ~ or a dragon this turn"
        || low == "the number of creature cards in its controller's graveyard"
        || low == "the greatest mana value among your commanders"
        || low == "that spell's mana value"
        || low == "the number of loyalty counters on it"
        || low
            == "the number of artifacts that were put into graveyards from the battlefield this turn"
        || low == "the total power of creatures exiled this way"
        || low == "the damage dealt this way"
        || low == "the number of opponents you attacked this turn"
        || low == "the damage dealt to your opponents this turn"
        || low == "the total number of rad counters among players"
        || low == "the number of counters among creatures target player controls"
        || low == "the number of your opponents with four or more cards in hand"
        || low == "the number of opponents who control a creature with power 4 or greater"
        || low == "the total power of those creatures"
        || low
            == "the number of cards that were put into your graveyard from your library this turn"
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if low == "the life they lost this turn"
        || low == "the life that player lost this turn"
        || low == "the amount of life they lost this turn"
    {
        return Ok(ValueExprAst::LifeChangedThisTurn {
            player: PlayerRefAst::ThatPlayer,
            gained: false,
        });
    }
    if low == "their total power" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if low == "their total toughness" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if low == "their total mana value" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(ObjectFilterAst {
                any_card: true,
                ..Default::default()
            }),
        });
    }
    if let Some(rest) = low.strip_prefix("twice ")
        && let Ok(inner) = parse_value_expr(rest, vocab)
    {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(2)),
            Box::new(inner),
        ));
    }
    if let Some(rest) = low.strip_prefix("three times ")
        && let Ok(inner) = parse_value_expr(rest, vocab)
    {
        return Ok(ValueExprAst::Mul(
            Box::new(ValueExprAst::Const(3)),
            Box::new(inner),
        ));
    }
    if let Some(n) = parse_number_word(&low) {
        return Ok(ValueExprAst::Const(n));
    }
    if let Some(rest) = low.strip_prefix("equal to ") {
        return parse_value_expr(rest, vocab);
    }

    if let Some((l, r)) = low.split_once(" plus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Add(Box::new(lv), Box::new(rv)));
    }
    if let Some((l, r)) = low.split_once(" minus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Sub(Box::new(lv), Box::new(rv)));
    }

    // "your devotion to <colors>" & pre-Theros devotion phrasing
    if let Some(colors) = low.strip_prefix("your devotion to ") {
        return Ok(ValueExprAst::Devotion {
            colors: colors.trim().to_string(),
        });
    }
    if let Some(after) = low.strip_prefix("the number of ")
        && let Some(colors) = after
            .strip_suffix(" mana symbols in the mana costs of permanents you control")
            .or_else(|| {
                after.strip_suffix(" mana symbols in the mana costs of creatures you control")
            })
    {
        return Ok(ValueExprAst::Devotion {
            colors: colors.trim().to_string(),
        });
    }
    if low == "the total number of cards in all players' hands" {
        return Ok(ValueExprAst::TotalCardsInAllHands);
    }
    if low == "the number of creatures in your party" {
        return Ok(ValueExprAst::PartySize);
    }
    if let Some(rest) = low.strip_prefix("the amount of life ") {
        if rest == "you've gained this turn" || rest == "you gained this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: true,
            });
        }
        if rest == "you've lost this turn" || rest == "you lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: false,
            });
        }
        if let Some((who_raw, tail)) = rest.split_once(' ')
            && let Some(p) = parse_player_ref(who_raw)
        {
            if tail == "gained this turn"
                || tail == "has gained this turn"
                || tail == "have gained this turn"
            {
                return Ok(ValueExprAst::LifeChangedThisTurn {
                    player: p,
                    gained: true,
                });
            }
            if tail == "lost this turn"
                || tail == "has lost this turn"
                || tail == "have lost this turn"
            {
                return Ok(ValueExprAst::LifeChangedThisTurn {
                    player: p,
                    gained: false,
                });
            }
        }
    }
    if low == "the amount of mana spent to cast ~"
        || low == "the amount of mana spent to cast it"
        || low == "the amount of mana spent to cast her"
        || low == "the amount of mana spent to cast him"
        || low == "the amount of mana spent to cast this spell"
        || low == "the amount of mana spent to cast that spell"
    {
        return Ok(ValueExprAst::ManaSpentToCast { mana_filter: None });
    }
    if let Some(after) = low.strip_prefix("the amount of ")
        && let Some(sym) = after
            .strip_suffix(" spent to cast ~")
            .or_else(|| after.strip_suffix(" spent to cast it"))
            .or_else(|| after.strip_suffix(" spent to cast this spell"))
    {
        return Ok(ValueExprAst::ManaSpentToCast {
            mana_filter: Some(sym.trim().to_string()),
        });
    }
    if low == "the total amount of mana paid this way"
        || low == "the life paid as it entered"
        || low == "the damage dealt"
        || low == "the other result"
        || low == "your speed"
        || low.starts_with("the amount of damage ")
    {
        return Ok(ValueExprAst::PriorResult);
    }

    // "the number of ..." / "the total number of ..."
    if let Some(after_num) = low
        .strip_prefix("the total number of ")
        .or_else(|| low.strip_prefix("the number of "))
        .or_else(|| low.strip_prefix("number of "))
    {
        if after_num.ends_with(" removed this way")
            || after_num.ends_with(" exiled this way")
            || after_num.ends_with(" sacrificed this way")
            || after_num.ends_with(" discarded this way")
            || after_num.ends_with(" revealed this way")
            || after_num.ends_with(" milled this way")
            || after_num.ends_with(" that came up heads")
            || after_num.ends_with(" that came up tails")
        {
            return Ok(ValueExprAst::PriorResult);
        }
        if matches!(
            after_num,
            "basic land types among lands you control" | "basic land type among lands you control"
        ) {
            return Ok(ValueExprAst::DomainCount);
        }
        if after_num == "opponents you have" {
            return Ok(ValueExprAst::OpponentsCount);
        }
        if after_num == "card types among cards in your graveyard" {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            });
        }
        if after_num == "card types among cards in all graveyards" {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: true,
            });
        }
        if let Some(flt_raw) = after_num.strip_prefix("colors among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::ColorsAmong(Box::new(flt)));
        }
        if let Some(flt_raw) = after_num.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
        }
        if let Some(flt_raw) = after_num.strip_prefix("creature types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::SubtypesAmong(Box::new(flt)));
        }
        if let Some(ck_raw) = after_num
            .strip_suffix(" counters you have")
            .or_else(|| after_num.strip_suffix(" counter you have"))
        {
            return Ok(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::You,
                counter_kind: ck_raw.trim().to_string(),
            });
        }
        if let Some(ck_raw) = after_num
            .strip_suffix(" counters your opponents have")
            .or_else(|| after_num.strip_suffix(" counter your opponents have"))
        {
            return Ok(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::EachOpponent,
                counter_kind: ck_raw.trim().to_string(),
            });
        }
        if let Some(subj_raw) = after_num
            .strip_prefix("counters on ")
            .or_else(|| after_num.strip_prefix("counter on "))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: "any".to_string(),
            });
        }
        if let Some((left, right)) = after_num.split_once(" counters on ") {
            let subj = parse_subject_or_target(right, vocab)?;
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: left.trim().to_string(),
            });
        }
        if let Some((left, right)) = after_num.split_once(" counter on ") {
            let subj = parse_subject_or_target(right, vocab)?;
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: left.trim().to_string(),
            });
        }
        if let Some(rest) = after_num.strip_prefix("cards in ") {
            if let Some(who) = rest.strip_suffix(" hand") {
                return Ok(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::CardsInHand,
                });
            }
            if let Some(who) = rest.strip_suffix(" library") {
                return Ok(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::CardsInLibrary,
                });
            }
            if let Some(who) = rest.strip_suffix(" graveyard") {
                return Ok(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::CardsInGraveyard,
                });
            }
        }
        let flt_src = after_num
            .strip_prefix("differently named ")
            .unwrap_or(after_num);
        let flt = parse_object_filter(flt_src, vocab)?;
        return Ok(ValueExprAst::Count(Box::new(flt)));
    }

    // "<subject>'s power/toughness/mana value/loyalty"
    for (suffix, stat) in [
        (" power", StatKind::Power),
        (" toughness", StatKind::Toughness),
        (" mana value", StatKind::ManaValue),
        (" converted mana cost", StatKind::ManaValue),
        (" loyalty", StatKind::Loyalty),
    ] {
        if let Some(owner_raw) = low.strip_suffix(suffix) {
            if matches!(owner_raw, "its" | "his" | "her" | "their") {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora(owner_raw.to_string())),
                    stat,
                });
            }
            if let Some(subj_raw) = owner_raw.strip_suffix("'s")
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(subj),
                    stat,
                });
            }
        }
    }
    // "the power/toughness/mana value of <subject>" (including "the card you exiled", "the sacrificed creature", etc.)
    for (pref, stat) in [
        ("the power of ", StatKind::Power),
        ("the toughness of ", StatKind::Toughness),
        ("the mana value of ", StatKind::ManaValue),
    ] {
        if let Some(subj_raw) = low.strip_prefix(pref) {
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(subj),
                    stat,
                });
            }
            if subj_raw.starts_with("the ")
                && (subj_raw.contains("exiled")
                    || subj_raw.contains("sacrificed")
                    || subj_raw.contains("discarded")
                    || subj_raw.contains("revealed")
                    || subj_raw.contains("milled")
                    || subj_raw.contains("returned")
                    || subj_raw.contains("destroyed"))
            {
                return Ok(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora(subj_raw.to_string())),
                    stat,
                });
            }
        }
    }

    // "the greatest / total <stat> among/of <filter>"
    let agg_body = low.strip_prefix("the ").unwrap_or(&low);
    for (prefix, op) in [
        ("greatest ", AggregateOp::Max),
        ("total ", AggregateOp::Sum),
    ] {
        if let Some(after) = agg_body.strip_prefix(prefix) {
            for (st_str, stat) in [
                ("power ", StatKind::Power),
                ("toughness ", StatKind::Toughness),
                ("mana value ", StatKind::ManaValue),
            ] {
                if let Some(rest) = after.strip_prefix(st_str) {
                    let flt_raw = rest
                        .strip_prefix("among ")
                        .or_else(|| rest.strip_prefix("of "));
                    if let Some(fstr) = flt_raw {
                        if matches!(
                            fstr,
                            "them"
                                | "those creatures"
                                | "those permanents"
                                | "those cards"
                                | "the chosen creatures"
                                | "the exiled cards"
                                | "the sacrificed creatures"
                                | "cards exiled this way"
                        ) {
                            return Ok(ValueExprAst::AggregateStat {
                                op,
                                stat,
                                filter: Box::new(ObjectFilterAst {
                                    any_card: true,
                                    ..Default::default()
                                }),
                            });
                        }
                        if let Ok(flt) = parse_object_filter(fstr, vocab) {
                            return Ok(ValueExprAst::AggregateStat {
                                op,
                                stat,
                                filter: Box::new(flt),
                            });
                        }
                    }
                }
            }
        }
    }

    // "half <expr>[, rounded up/down]"
    if let Some(after) = low.strip_prefix("half ") {
        let (core, rounding) = if let Some(c) = after.strip_suffix(", rounded up") {
            (c, Rounding::Up)
        } else if let Some(c) = after.strip_suffix(", rounded down") {
            (c, Rounding::Down)
        } else {
            (after, Rounding::Down)
        };
        if let Some(who) = core.strip_suffix(" starting life total") {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::StartingLifeTotal(parse_possessive_player(
                    who,
                ))),
                denom: 2,
                rounding,
            });
        }
        if let Some(who) = core
            .strip_suffix(" life total")
            .or_else(|| core.strip_suffix(" life"))
        {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(ValueExprAst::PlayerStat {
                    player: parse_possessive_player(who),
                    stat: PlayerStatKind::LifeTotal,
                }),
                denom: 2,
                rounding,
            });
        }
        if let Ok(inner) = parse_value_expr(core, vocab) {
            return Ok(ValueExprAst::DivConst {
                expr: Box::new(inner),
                denom: 2,
                rounding,
            });
        }
    }

    if low == "the lowest life total among all players"
        || low == "the lowest life total among players"
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Min,
            metric: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::EachPlayer,
                stat: PlayerStatKind::LifeTotal,
            }),
        });
    }
    if low == "the highest life total among all players"
        || low == "the highest life total among players"
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Max,
            metric: Box::new(ValueExprAst::PlayerStat {
                player: PlayerRefAst::EachPlayer,
                stat: PlayerStatKind::LifeTotal,
            }),
        });
    }
    if let Some(after) = low.strip_prefix("the greatest number of ")
        && let Some(flt_raw) = after
            .strip_suffix(" a player controls")
            .or_else(|| after.strip_suffix(" an opponent controls"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Max,
            metric: Box::new(ValueExprAst::Count(Box::new(flt))),
        });
    }
    if low == "the greatest number of cards a player discarded this way" {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some((who_raw, _)) = low.split_once("'s life total")
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Ok(ValueExprAst::PlayerStat {
            player: p,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if let Some(who) = low.strip_suffix(" starting life total") {
        return Ok(ValueExprAst::StartingLifeTotal(parse_possessive_player(
            who,
        )));
    }
    if let Some(who) = low.strip_suffix(" life total") {
        return Ok(ValueExprAst::PlayerStat {
            player: parse_possessive_player(who),
            stat: PlayerStatKind::LifeTotal,
        });
    }

    // Orthogonal arithmetic composition: "<expr1> plus <expr2>" / "<expr1> minus <expr2>"
    // (Sworn Defender, Roiling Horror, Lost Order of Jarkeld, Shapeshifter, Angry Mob, Haunting Apparition)
    if let Some((l, r)) = low.split_once(" plus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Add(Box::new(lv), Box::new(rv)));
    }
    if let Some((l, r)) = low.split_once(" minus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Sub(Box::new(lv), Box::new(rv)));
    }

    if is_physical_tabletop_clause(&low) {
        return Ok(ValueExprAst::ManualTabletopNumber {
            prompt: raw.to_string(),
        });
    }

    Err(format!("unparsed_value:{raw}"))
}

/// Unified shared parser for `"for each <expr>"` clauses across all actions, mana specs,
/// token creators, cost modifiers, and continuous P/T buffs.
pub fn parse_for_each_value_expr(fe_raw: &str, vocab: &Vocabulary) -> Result<ValueExprAst, String> {
    let fe = trim_punct(fe_raw).to_ascii_lowercase();
    if fe.is_empty() {
        return Err("empty_for_each".to_string());
    }
    match fe.as_str() {
        "basic land type among lands you control" | "basic land types among lands you control" => {
            return Ok(ValueExprAst::DomainCount);
        }
        "color of mana spent to cast ~"
        | "color of mana spent to cast it"
        | "color of mana spent to cast this spell" => {
            return Ok(ValueExprAst::ColorsOfManaSpent);
        }
        "time ~ was kicked" | "time it was kicked" => {
            return Ok(ValueExprAst::TimesKicked);
        }
        "opponent you have" | "opponent" => {
            return Ok(ValueExprAst::OpponentsCount);
        }
        "of its colors" | "of ~'s colors" => {
            return Ok(ValueExprAst::SelfColorsCount);
        }
        "card type among cards in your graveyard" => {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            });
        }
        "card type among cards in all graveyards" => {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: true,
            });
        }
        "counter removed this way"
        | "counter removed"
        | "card discarded this way"
        | "card exiled this way"
        | "card revealed this way"
        | "card milled this way"
        | "creature sacrificed this way"
        | "permanent sacrificed this way"
        | "creature destroyed this way"
        | "creature that died this way"
        | "card put into a graveyard this way"
        | "card put into your graveyard this way"
        | "1 damage dealt this way"
        | "1 damage prevented this way"
        | "damage dealt this way"
        | "damage prevented this way"
        | "1 life lost this way"
        | "1 life you lost"
        | "flip you won"
        | "creature it devoured"
        | "of that spell's colors"
        | "unspent mana you have"
        | "unspent white mana you have"
        | "unspent blue mana you have"
        | "unspent black mana you have"
        | "unspent red mana you have"
        | "unspent green mana you have"
        | "mana spent to cast it"
        | "mana spent to cast ~"
        | "{s} spent to cast ~"
        | "card in your opponents' hands"
        | "card drawn this way"
        | "spell countered this way"
        | "player being attacked"
        | "opponent dealt damage"
        | "mana from a desert spent to cast ~"
        | "fellowship vote"
        | "vote they received"
        | "{e} you've paid or lost this turn"
        | "counter among players and permanents"
        | "spell and ability countered this way"
        | "one they gained control of this way"
        | "flip you lose"
        | "white mana symbol in its mana cost"
        | "blue mana symbol in its mana cost"
        | "black mana symbol in its mana cost"
        | "red mana symbol in its mana cost"
        | "green mana symbol in its mana cost"
        | "card you own in exile and in your graveyard that's an instant card, a sorcery card, or a dragon card"
        | "opponent you're attacking"
        | "opponent you attacked this turn"
        | "target"
        | "card you've drawn this turn"
        | "card you drew this turn"
        | "spell you've cast this turn"
        | "spell you cast this turn"
        | "spell your opponents have cast this turn"
        | "spell an opponent has cast this turn"
        | "spell cast this turn"
        | "opponent whose life total is less than half their starting life total"
        | "color in your commanders' color identity"
        | "color in your commander's color identity"
        | "life you gained this turn"
        | "life you lost this turn"
        | "time you've cast your commander from the command zone this game"
        | "card you've discarded this turn"
        | "card you discarded this turn"
        | "spell you've cast this turn from anywhere other than your hand"
        | "creature card put into their graveyard this way"
        | "land card put into their graveyard this way"
        | "graveyard with seven or more cards in it"
        | "supertype, card type, and subtype it has"
        | "time it has attacked this turn"
        | "creature put into your graveyard from the battlefield this turn"
        | "nontoken creature you controlled that was destroyed this way"
        | "+1/+1 counter you've put on creatures under your control this turn"
        | "{b} or {r} spent this way"
        | "creature blocking it beyond the first"
        | "mana from a treasure spent to cast it"
        | "{e} paid this way"
        | "card looked at while scrying this way"
        | "card put into your hand this way"
        | "creature that phased out this way"
        | "time it regenerated this turn"
        | "name sticker on it"
        | "name sticker on ~ with seven or fewer letters"
        | "name sticker on ~ with eight or more letters"
        | "modified creature you controlled as you cast ~"
        | "white mana symbol in the mana costs of permanents you control"
        | "blue mana symbol in the mana costs of permanents you control"
        | "black mana symbol in the mana costs of permanents you control"
        | "red mana symbol in the mana costs of permanents you control"
        | "green mana symbol in the mana costs of permanents you control"
        | "creature you control with base power or toughness 1"
        | "creature card put into your graveyard from the battlefield this turn"
        | "creature card put into your graveyard this way"
        | "time you've cast a commander from the command zone this game"
        | "mana from a cave spent to cast it"
        | "nontoken creature put into your graveyard from the battlefield this turn"
        | "creature you controlled that dealt combat damage to a player this turn" => {
            return Ok(ValueExprAst::PriorResult);
        }
        _ => {}
    }

    if fe.ends_with(" counter removed this way")
        || fe.ends_with(" counters removed this way")
        || fe.ends_with(" counter put on it this way")
        || fe.ends_with(" counter put on ~ this way")
        || fe.ends_with(" card exiled this way")
        || fe.ends_with(" card discarded this way")
        || fe.ends_with(" card revealed this way")
        || fe.ends_with(" card milled this way")
        || fe.ends_with(" permanent sacrificed this way")
        || fe.ends_with(" creature sacrificed this way")
        || fe.ends_with(" returned to your hand this way")
        || fe.ends_with(" returned to its owner's hand this way")
        || fe.ends_with(" returned to their owners' hands this way")
        || fe.ends_with(" returned this way")
    {
        return Ok(ValueExprAst::PriorResult);
    }

    if let Some(flt_raw) = fe
        .strip_prefix("different mana value among ")
        .or_else(|| fe.strip_prefix("different mana values among "))
        .or_else(|| fe.strip_prefix("different power among "))
        .or_else(|| fe.strip_prefix("different powers among "))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
    }
    if fe == "copy"
        || fe == "flip you win"
        || fe == "card manifested this way"
        || fe == "different result"
        || fe.ends_with(" vote")
        || fe.starts_with("of those ")
        || fe.starts_with("kind of counter on ")
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some(flt_raw) = fe.strip_prefix("color among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::ColorsAmong(Box::new(flt)));
    }
    if let Some(flt_raw) = fe.strip_prefix("card type among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
    }
    if let Some(flt_raw) = fe.strip_prefix("creature type among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::SubtypesAmong(Box::new(flt)));
    }

    // "<kind> counter(s) you have" / "<kind> counter(s) your opponents have" / "<kind> counter(s) <player> has"
    if let Some(ck) = fe
        .strip_suffix(" counter you have")
        .or_else(|| fe.strip_suffix(" counters you have"))
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            counter_kind: ck.trim().to_string(),
        });
    }
    if let Some(ck) = fe
        .strip_suffix(" counter your opponents have")
        .or_else(|| fe.strip_suffix(" counters your opponents have"))
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::EachOpponent,
            counter_kind: ck.trim().to_string(),
        });
    }
    if let Some((ck_part, who_part)) = fe
        .split_once(" counter ")
        .or_else(|| fe.split_once(" counters "))
        && let Some(who_raw) = who_part
            .strip_suffix(" has")
            .or_else(|| who_part.strip_suffix(" have"))
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player,
            counter_kind: ck_part.trim().to_string(),
        });
    }

    // "counter(s) on <subj>" / "<kind> counter(s) on <subj>"
    if let Some(subj_raw) = fe
        .strip_prefix("counter on ")
        .or_else(|| fe.strip_prefix("counters on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ValueExprAst::CounterCount {
            subject: Box::new(subj),
            counter_kind: "any".to_string(),
        });
    }
    if let Some((ck, subj_raw)) = fe
        .split_once(" counters on ")
        .or_else(|| fe.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ValueExprAst::CounterCount {
            subject: Box::new(subj),
            counter_kind: ck.trim().to_string(),
        });
    }

    // "card in <player>'s hand/graveyard/library"
    if let Some(rest) = fe.strip_prefix("card in ") {
        if let Some(who) = rest.strip_suffix(" hand") {
            return Ok(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who),
                stat: PlayerStatKind::CardsInHand,
            });
        }
        if let Some(who) = rest.strip_suffix(" graveyard") {
            return Ok(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who),
                stat: PlayerStatKind::CardsInGraveyard,
            });
        }
        if let Some(who) = rest.strip_suffix(" library") {
            return Ok(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who),
                stat: PlayerStatKind::CardsInLibrary,
            });
        }
    }

    // Compound "for each <A> and each <B>" (Gargantuan Leech, Alrund, etc.)
    if let Some((a, b)) = fe.split_once(" and each ")
        && let (Ok(va), Ok(vb)) = (
            parse_for_each_value_expr(a, vocab),
            parse_for_each_value_expr(b, vocab),
        )
    {
        return Ok(ValueExprAst::Add(Box::new(va), Box::new(vb)));
    }
    // "1 life <player> [have/has] lost/gained this turn" (Rakdos, Lord of Riots, Licia)
    if let Some(rest) = fe.strip_prefix("1 life ") {
        if rest == "you gained this turn" || rest == "you've gained this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: true,
            });
        }
        if rest == "you lost this turn" || rest == "you've lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: false,
            });
        }
        if rest == "your opponents have lost this turn" || rest == "opponents lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::EachOpponent,
                gained: false,
            });
        }
    }
    // "opponent who <cond>" / "player who <cond>"
    if let Some(cond_raw) = fe
        .strip_prefix("opponent who ")
        .or_else(|| fe.strip_prefix("player who "))
        && !cond_raw.is_empty()
    {
        return Ok(ValueExprAst::PriorResult);
    }

    let flt = parse_object_filter(&fe, vocab)?;
    Ok(ValueExprAst::Count(Box::new(flt)))
}

fn parse_possessive_player(who: &str) -> PlayerRefAst {
    match who.trim() {
        "your" => PlayerRefAst::You,
        "an opponent's" | "target opponent's" => PlayerRefAst::TargetOpponent,
        "target player's" => PlayerRefAst::TargetPlayer,
        "each player's" => PlayerRefAst::EachPlayer,
        _ => PlayerRefAst::ThatPlayer,
    }
}

/// Strictly parse an intervening-`if`, resolution-`if`, or `as long as` condition.
pub fn parse_condition(phrase: &str, vocab: &Vocabulary) -> Result<ConditionAst, String> {
    let raw = trim_punct(phrase);
    if raw.is_empty() {
        return Err("empty_condition".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = low_full
        .strip_suffix(" as you cast ~")
        .or_else(|| low_full.strip_suffix(" as you cast this spell"))
        .unwrap_or(&low_full)
        .to_string();

    match low.as_str() {
        "~ was kicked" | "it was kicked" => return Ok(ConditionAst::WasKicked),
        "you cast it" | "you cast ~" => {
            return Ok(ConditionAst::WasCast {
                by_you: true,
                from_hand: false,
            });
        }
        "~ was cast" | "it was cast" => {
            return Ok(ConditionAst::WasCast {
                by_you: false,
                from_hand: false,
            });
        }
        "you cast it from your hand" | "~ was cast from your hand" => {
            return Ok(ConditionAst::WasCast {
                by_you: true,
                from_hand: true,
            });
        }
        "no spells were cast last turn" => {
            return Ok(ConditionAst::SpellsCastLastTurn {
                any_player_single: false,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "a player cast two or more spells last turn" => {
            return Ok(ConditionAst::SpellsCastLastTurn {
                any_player_single: true,
                op: CompareOp::Ge,
                value: 2,
            });
        }
        "you search your library this way" => return Ok(ConditionAst::SearchedLibraryThisWay),
        "it's not a token"
        | "it isn't a token"
        | "that creature isn't a token"
        | "~ isn't a token" => {
            return Ok(ConditionAst::IsNotToken);
        }
        "a creature died this turn" => {
            return Ok(ConditionAst::Morbid {
                your_control_only: false,
            });
        }
        "a creature died under your control this turn" => {
            return Ok(ConditionAst::Morbid {
                your_control_only: true,
            });
        }
        "you have no cards in hand" | "you have no cards in your hand" => {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "it's your turn" | "it is your turn" => return Ok(ConditionAst::IsYourTurn),
        "it's not your turn" | "it isn't your turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)));
        }
        "you gained life this turn" | "you've gained life this turn" => {
            return Ok(ConditionAst::GainedLifeThisTurn);
        }
        "an opponent lost life this turn" | "opponents lost life this turn" => {
            return Ok(ConditionAst::OpponentLostLifeThisTurn);
        }
        "you attacked this turn" | "you attacked with a creature this turn" => {
            return Ok(ConditionAst::AttackedThisTurn);
        }
        "~ entered this turn" | "~ entered the battlefield this turn" => {
            return Ok(ConditionAst::EnteredThisTurn);
        }
        "you control your commander"
        | "you control a commander"
        | "you control a commander as you cast ~" => {
            return Ok(ConditionAst::ControlsCommander);
        }
        "tribute wasn't paid" => return Ok(ConditionAst::TributeNotPaid),
        "tribute was paid" => return Ok(ConditionAst::Not(Box::new(ConditionAst::TributeNotPaid))),
        "it shares a creature type with ~" => return Ok(ConditionAst::SharesTypeWithSelf),
        "you descended this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DescendedThisTurn));
        }
        "you have the city's blessing" => {
            return Ok(ConditionAst::Marker(GameMarker::CitysBlessing));
        }
        "you're the monarch" | "you are the monarch" => {
            return Ok(ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)));
        }
        "defending player is the monarch" => {
            return Ok(ConditionAst::Marker(GameMarker::Monarch(
                PlayerRefAst::DefendingPlayer,
            )));
        }
        "there is no monarch" => return Ok(ConditionAst::Marker(GameMarker::NoMonarch)),
        "you've completed a dungeon" => {
            return Ok(ConditionAst::Marker(GameMarker::CompletedDungeon));
        }
        "it's day" => return Ok(ConditionAst::Marker(GameMarker::Day)),
        "it's night" => return Ok(ConditionAst::Marker(GameMarker::Night)),
        "it's neither day nor night" => {
            return Ok(ConditionAst::Marker(GameMarker::NeitherDayNorNight));
        }
        "you have max speed" => return Ok(ConditionAst::Marker(GameMarker::MaxSpeed)),
        "you have the initiative" => {
            return Ok(ConditionAst::Marker(GameMarker::Initiative(
                PlayerRefAst::You,
            )));
        }
        "you committed a crime this turn" | "you've committed a crime this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn));
        }
        "you gained or lost life this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::PlayerGainedOrLostLifeThisTurn {
                    player: PlayerRefAst::You,
                },
            ));
        }
        "a player lost life this turn" => {
            return Ok(ConditionAst::LostLifeThisTurn {
                player: PlayerRefAst::AnyPlayer,
                min_amount: 1,
            });
        }
        "an opponent was dealt damage this turn"
        | "an opponent has been dealt damage this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: false,
                },
            ));
        }
        "an opponent was dealt noncombat damage this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: true,
                },
            ));
        }
        "you attacked with two or more creatures this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedWithMinCreaturesThisTurn {
                    player: PlayerRefAst::You,
                    min_count: 2,
                },
            ));
        }
        "a creature entered the battlefield under your control this turn"
        | "a creature entered under your control this turn" => {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                }),
                min_count: 1,
            });
        }
        "you cast a spell this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value: 1,
                from_hand_only: false,
            }));
        }
        "you've cast two or more spells this turn" | "you cast two or more spells this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Ge,
                value: 2,
                from_hand_only: false,
            }));
        }
        "you haven't cast a spell from your hand this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
                from_hand_only: true,
            }));
        }
        "you've cast a historic spell this turn" => {
            return Ok(ConditionAst::CastSpellThisTurn {
                player: PlayerRefAst::You,
                filter: Box::new(ObjectFilterAst {
                    required_supertypes: SupertypeMask::HISTORIC,
                    any_spell: true,
                    ..Default::default()
                }),
            });
        }
        "this spell was cast from your graveyard"
        | "~ was cast from your graveyard"
        | "~ was cast from a graveyard"
        | "it was cast from a graveyard" => {
            return Ok(ConditionAst::Marker(GameMarker::CastFromGraveyard));
        }
        "~ was cast from exile" => return Ok(ConditionAst::Marker(GameMarker::CastFromExile)),
        "~ was foretold" | "it was foretold" => {
            return Ok(ConditionAst::Marker(GameMarker::Foretold));
        }
        "~ wasn't cast from your hand"
        | "~ was cast from anywhere other than your hand"
        | "you didn't cast it from your hand" => {
            return Ok(ConditionAst::Marker(GameMarker::CastFromOutsideHand));
        }
        "~ attacked or blocked this combat" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedOrBlockedThisCombat,
            ));
        }
        "an opponent owns a card in exile" => {
            return Ok(ConditionAst::Marker(GameMarker::OwnerHasCardInExile {
                player: PlayerRefAst::AnyOpponent,
            }));
        }
        "you own a card in exile" => {
            return Ok(ConditionAst::Marker(GameMarker::OwnerHasCardInExile {
                player: PlayerRefAst::You,
            }));
        }
        "defending player is poisoned" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
                player: PlayerRefAst::DefendingPlayer,
            }));
        }
        "its controller is poisoned" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
                player: PlayerRefAst::ItsController,
            }));
        }
        "defending player has a rad counter" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayerHasRadCounter {
                player: PlayerRefAst::DefendingPlayer,
            }));
        }
        "there are no cards in your graveyard" => {
            return Ok(ConditionAst::GraveyardCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        "you control neither creature" => {
            return Ok(ConditionAst::Marker(GameMarker::ControlsNeither));
        }
        "you control all three" => {
            return Ok(ConditionAst::Marker(GameMarker::ControlsAllThree));
        }
        "you control fewer creatures than an opponent" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsFewerThanOpponent {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                },
            ));
        }
        "you control more creatures than each other player"
        | "you control more creatures than that spell's controller" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                    than_each_other: true,
                },
            ));
        }
        "you control more creatures of that type than each other player" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::CREATURE,
                        flags: ObjectFilterFlags::MATCHES_CHOSEN_TYPE_OR_COLOR,
                        ..Default::default()
                    }),
                    than_each_other: true,
                },
            ));
        }
        "you control more lands than defending player" => {
            return Ok(ConditionAst::Marker(
                GameMarker::ControlsMoreThanOpponents {
                    filter: Box::new(ObjectFilterAst {
                        any_types: CardTypeMask::LAND,
                        ..Default::default()
                    }),
                    than_each_other: false,
                },
            ));
        }
        "you bargained" | "~ was bargained" | "it was bargained" => {
            return Ok(ConditionAst::Marker(GameMarker::Bargained));
        }
        "the gift was promised" => return Ok(ConditionAst::Marker(GameMarker::GiftPromised)),
        "you're attacking" => return Ok(ConditionAst::Marker(GameMarker::YouAreAttacking)),
        "you're defending" => return Ok(ConditionAst::Marker(GameMarker::YouAreDefending)),
        "two or more players have lost the game" => {
            return Ok(ConditionAst::Marker(GameMarker::PlayersLostGame {
                op: CompareOp::Ge,
                value: 2,
            }));
        }
        "you don't put the card into your hand"
        | "you didn't put a card into your hand this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "hand".to_string(),
            }));
        }
        "you don't put the card onto the battlefield"
        | "you didn't put a card onto the battlefield this way"
        | "you don't put it onto the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
                zone: "battlefield".to_string(),
            }));
        }
        "a nonland permanent left the battlefield this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::NonlandPermanentLeftThisTurn,
            ));
        }
        "a spell was warped this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::SpellWarpedThisTurn));
        }
        "~ was cast using teamwork" | "it's cast using teamwork" => {
            return Ok(ConditionAst::Marker(GameMarker::CastUsingTeamwork));
        }
        "chaos gets more votes or the vote is tied" => {
            return Ok(ConditionAst::Marker(GameMarker::VoteResult {
                winning: "chaos_or_tie".to_string(),
            }));
        }
        "planeswalk gets more votes" => {
            return Ok(ConditionAst::Marker(GameMarker::VoteResult {
                winning: "planeswalk".to_string(),
            }));
        }
        "you have a full party" => return Ok(ConditionAst::Marker(GameMarker::FullParty)),
        "it's the first combat phase of the turn" => {
            return Ok(ConditionAst::Marker(GameMarker::FirstCombatPhase));
        }
        "it isn't a mana ability" => return Ok(ConditionAst::Marker(GameMarker::NotManaAbility)),
        "it's not their turn" | "it isn't that player's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)));
        }
        "it's on the battlefield" | "it is on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                zone: ZoneFilter::Battlefield,
            }));
        }
        "~ is in the command zone or on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::InCommandZoneOrBattlefield));
        }
        "~ is suspended" => return Ok(ConditionAst::Marker(GameMarker::IsSuspended)),
        "~ is paired with another creature" => {
            return Ok(ConditionAst::Marker(GameMarker::PairedWithAnotherCreature));
        }
        "no creatures are on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::NoCreaturesOnBattlefield));
        }
        "it has a single target" => return Ok(ConditionAst::Marker(GameMarker::SingleTarget)),
        "it isn't being declared as an attacker" => {
            return Ok(ConditionAst::Marker(GameMarker::NotDeclaredAttacker));
        }
        "you chose a creature other than ~ as your ring-bearer" => {
            return Ok(ConditionAst::Marker(GameMarker::RingBearerOtherThanSelf));
        }
        "a card left your graveyard this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn));
        }
        "you have more cards in hand than each opponent" => {
            return Ok(ConditionAst::Marker(GameMarker::HandMoreThanEachOpponent));
        }
        "your life total is less than or equal to half your starting life total" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Le,
                rhs: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::You)),
                    denom: 2,
                    rounding: Rounding::Down,
                }),
            });
        }
        "an opponent's life total is less than half their starting life total" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Lt,
                rhs: Box::new(ValueExprAst::DivConst {
                    expr: Box::new(ValueExprAst::StartingLifeTotal(PlayerRefAst::AnyOpponent)),
                    denom: 2,
                    rounding: Rounding::Down,
                }),
            });
        }
        "it entered from your graveyard or you cast it from your graveyard" => {
            return Ok(ConditionAst::Marker(GameMarker::EnteredOrCastFromGraveyard));
        }
        "it shares a card type with the exiled card" => {
            return Ok(ConditionAst::Marker(GameMarker::SharesCardTypeWithExiled));
        }
        "it targets a commander" => {
            return Ok(ConditionAst::Marker(GameMarker::TargetsCommander));
        }
        "it has the same mana value as the discarded card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameManaValueAsDiscarded));
        }
        "it has the same name as that card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameNameAsTarget));
        }
        "its mana value is x" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ObjectStat {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat: StatKind::ManaValue,
                }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::ChosenX),
            });
        }
        "you control ~" => return Ok(ConditionAst::Marker(GameMarker::ControlsSelf)),
        "~ remains tapped" => return Ok(ConditionAst::Marker(GameMarker::RemainsTapped)),
        "~ remains on the battlefield" => {
            return Ok(ConditionAst::Marker(GameMarker::RemainsOnBattlefield));
        }
        "it's attacking alone" => return Ok(ConditionAst::Marker(GameMarker::AttackingAlone)),
        "damage is prevented this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DamagePreventedThisWay));
        }
        "you haven't cast it"
        | "you haven't played it"
        | "you don't cast it"
        | "you don't play it"
        | "you didn't cast it"
        | "it wasn't cast" => {
            return Ok(ConditionAst::Marker(GameMarker::HasNotPlayedOrCast));
        }
        "it's bargained" | "~ is bargained" => {
            return Ok(ConditionAst::Marker(GameMarker::Bargained));
        }
        "it's attacking the player with the most life or tied for most life"
        | "it's attacking the player with the most life" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackingAlone));
        }
        "you have more life than an opponent" | "you have more life than each opponent" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Gt,
                rhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::AnyOpponent,
                    stat: PlayerStatKind::LifeTotal,
                }),
            });
        }
        "no creatures attacked this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::DidNotAttackWithCreatureThisTurn {
                    player: PlayerRefAst::AnyPlayer,
                },
            ));
        }
        "no mana was spent to cast that spell" | "no mana was spent to cast it" => {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::ManaSpentToCast { mana_filter: None }),
                op: CompareOp::Eq,
                rhs: Box::new(ValueExprAst::Const(0)),
            });
        }
        "you won" | "you won the clash" | "you win the flip" | "you won the flip" => {
            return Ok(ConditionAst::WonContest);
        }
        "it didn't die" | "that creature didn't die" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::DiedThisWay,
            ))));
        }
        "the gift wasn't promised" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::GiftPromised,
            ))));
        }
        "it's paired with a creature" | "~ is paired with a creature" => {
            return Ok(ConditionAst::Marker(GameMarker::PairedWithAnotherCreature));
        }
        "a permanent's ability is countered this way" => {
            return Ok(ConditionAst::Marker(GameMarker::NotManaAbility));
        }
        "~ attacked this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
            }));
        }
        "you've discarded a card this turn" | "you discarded a card this turn" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: "discard".to_string(),
            });
        }
        "you haven't added mana with this ability this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn {
                op: CompareOp::Eq,
                value: 0,
            }));
        }
        "it's not that player's turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::IsYourTurn)));
        }
        "that spell targets only ~" | "it targets only ~" => {
            return Ok(ConditionAst::Marker(GameMarker::SingleTarget));
        }
        "that card remains exiled"
        | "those cards remain exiled"
        | "any of those cards remain exiled"
        | "it remains exiled"
        | "they remain exiled" => {
            return Ok(ConditionAst::Marker(GameMarker::RemainsExiled));
        }
        "it attacked this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisTurn {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            }));
        }
        "it attacked or blocked this turn" | "that creature attacked or blocked this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AttackedOrBlockedThisTurn {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                },
            ));
        }
        "that creature dies this way"
        | "it dies this way"
        | "it died this way"
        | "a creature died this way" => {
            return Ok(ConditionAst::Marker(GameMarker::DiedThisWay));
        }
        "~ didn't attack or come under your control this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
                or_entered_this_turn: true,
            }));
        }
        "~ didn't attack this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
                subject: Box::new(SubjectRefAst::SelfObject),
                or_entered_this_turn: false,
            }));
        }
        "one or more cards left your graveyard this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn));
        }
        "one or more of them entered from a graveyard or was cast from a graveyard" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AnyEnteredOrCastFromGraveyard,
            ));
        }
        "life was paid to activate it" => {
            return Ok(ConditionAst::Marker(GameMarker::LifePaidToActivate));
        }
        "that player has another opponent who isn't being attacked" => {
            return Ok(ConditionAst::Marker(GameMarker::HasUnattackedOpponent));
        }
        "it has greater power or toughness than ~" => {
            return Ok(ConditionAst::Marker(
                GameMarker::GreaterPowerOrToughnessThan {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    other: Box::new(SubjectRefAst::SelfObject),
                },
            ));
        }
        "this is the first time this ability has resolved this turn" | "it's the first time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(1),
            ));
        }
        "this is the second time this ability has resolved this turn" | "it's the second time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(2),
            ));
        }
        "this is the third time this ability has resolved this turn" | "it's the third time" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AbilityResolvedNthTimeThisTurn(3),
            ));
        }
        "you cast ~ during your main phase" | "you cast it during your main phase" => {
            return Ok(ConditionAst::Marker(GameMarker::CastDuringMainPhase));
        }
        "it escaped" | "~ escaped" => {
            return Ok(ConditionAst::Marker(GameMarker::Escaped));
        }
        "it has the same mana value as the revealed card" => {
            return Ok(ConditionAst::Marker(GameMarker::SameManaValueAsRevealed));
        }
        "evidence was collected" => {
            return Ok(ConditionAst::Marker(GameMarker::EvidenceCollected));
        }
        "you lost life last turn" => {
            return Ok(ConditionAst::Marker(GameMarker::LostLifeLastTurn {
                player: PlayerRefAst::You,
            }));
        }
        "~ attacked this combat" => {
            return Ok(ConditionAst::Marker(GameMarker::AttackedThisCombat {
                subject: Box::new(SubjectRefAst::SelfObject),
            }));
        }
        "the amount of mana spent to cast it was less than its mana value"
        | "the amount of mana spent to cast ~ was less than its mana value" => {
            return Ok(ConditionAst::Marker(GameMarker::ManaSpentLessThanManaValue));
        }
        "they entered or were cast from a graveyard" => {
            return Ok(ConditionAst::Marker(
                GameMarker::AnyEnteredOrCastFromGraveyard,
            ));
        }
        "you didn't attack with a creature this turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::DidNotAttackWithCreatureThisTurn {
                    player: PlayerRefAst::You,
                },
            ));
        }
        "you didn't cast a spell this turn" => {
            return Ok(ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
                player: PlayerRefAst::You,
                filter: None,
            }));
        }
        "an opponent has more life than you" => {
            return Ok(ConditionAst::OpponentMoreThanYou {
                metric: "life".to_string(),
                filter: None,
            });
        }
        "an opponent has more cards in hand than you" => {
            return Ok(ConditionAst::OpponentMoreThanYou {
                metric: "cards_in_hand".to_string(),
                filter: None,
            });
        }
        "~ is on the stack" => {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: "on_stack".to_string(),
            });
        }
        "~ didn't enter the battlefield this turn" | "~ didn't enter this turn" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::EnteredThisTurn)));
        }
        "a creature is attacking you" => {
            return Ok(ConditionAst::ControlsCount {
                player: PlayerRefAst::EachOpponent,
                filter: Box::new(ObjectFilterAst {
                    any_types: CardTypeMask::CREATURE,
                    combat: Some(CombatState::Attacking),
                    ..Default::default()
                }),
                op: CompareOp::Ge,
                value: 1,
            });
        }
        "a player attacked you during their last turn"
        | "that player attacked you during their last turn" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::ThatPlayer,
                action: "attacked_you_last_turn".to_string(),
            });
        }
        "an opponent was dealt noncombat damage last turn" => {
            return Ok(ConditionAst::Marker(
                GameMarker::OpponentDealtDamageThisTurn {
                    noncombat_only: true,
                },
            ));
        }
        "excess damage was dealt this way"
        | "it's greater"
        | "that spell is countered this way"
        | "you have an enduring story"
        | "that creature attacked this turn"
        | "it's a spell with lesser mana value"
        | "the amount of mana spent to cast that spell was less than its mana value"
        | "a card is exiled with it"
        | "a card is exiled with ~"
        | "it was cast from your graveyard"
        | "they were cast using web-slinging"
        | "~ was cast using web-slinging"
        | "you have a card in hand"
        | "it was the second spell you cast this turn"
        | "it was blocked this turn"
        | "~ was dealt damage this turn"
        | "it was dealt damage this turn"
        | "it dealt combat damage to a player this turn"
        | "an opponent lost life last turn"
        | "they aren't attacking you"
        | "that player is you"
        | "it entered under your control"
        | "that cost wasn't paid"
        | "the copy is a permanent spell"
        | "it's your first, second, or third turn of the game"
        | "an opponent is poisoned"
        | "you've activated a loyalty ability this turn"
        | "~ attacked or blocked this turn"
        | "it was attacking or blocking alone"
        | "an aura you controlled was attached to it"
        | "you didn't play a land this turn"
        | "you didn't lose life this turn"
        | "you didn't play a card from exile this turn"
        | "you put a counter on ~ this turn"
        | "a counter was put on ~ this turn"
        | "you put a counter on a creature this turn"
        | "~ dealt damage to an opponent this turn"
        | "~ dealt damage to it this turn"
        | "no creatures died this turn"
        | "you attacked with exactly one other creature this combat"
        | "it's the second creature spell you cast this turn"
        | "~ is in your graveyard or on the battlefield"
        | "you're the defending player"
        | "enchanted equipment is attached to a creature"
        | "it didn't attack this turn"
        | "an opponent searched their library this turn"
        | "three or more creatures attacked this turn"
        | "there are five colors among permanents you control"
        | "it devoured a creature"
        | "it hasn't dealt damage yet"
        | "your team gained life this turn"
        | "you've drawn more than one card this turn"
        | "one or more cards were put into exile this turn"
        | "no permanents left the battlefield this turn"
        | "a creature died under an opponent's control this turn"
        | "a creature card left your graveyard this turn"
        | "its power was different from its base power"
        | "that creature was dealt excess damage this turn"
        | "you haven't been dealt combat damage since your last turn"
        | "no other creatures are attacking that player"
        | "you were the monarch as the turn began"
        | "they attacked you and/or a planeswalker you control"
        | "it has the same name as one of the cards exiled with ~"
        | "~ is in your graveyard with a creature card directly above it"
        | "another creature died this turn"
        | "no cards are in that graveyard"
        | "you have less life than an opponent"
        | "its mana value is odd"
        | "its mana value is even"
        | "an opponent protects it"
        | "~ is your ring-bearer"
        | "enchanted creature is attacking alone"
        | "you've visited an attraction this turn"
        | "~ hasn't dealt damage yet"
        | "you cast it from anywhere other than your hand"
        | "that creature has greater power or toughness than ~"
        | "that player controls more lands than each other player"
        | "you removed two or more loyalty counters to activate it"
        | "you don't draw a card this way"
        | "that card has the chosen name"
        | "that land was nonbasic"
        | "exactly one creature is attacking"
        | "a land you controlled was put into a graveyard from the battlefield this turn"
        | "you or a player you're attacking has the initiative"
        | "two or more of those creatures are attacking you and/or planeswalkers you control"
        | "it enlisted a creature this combat"
        | "that player had another land enter the battlefield under their control this turn"
        | "excess damage was dealt to that creature this way"
        | "excess damage was dealt to a creature this way"
        | "excess damage was dealt to that permanent this way"
        | "the creature the opponent controls is dealt excess damage this way"
        | "~ attacked a battle this turn"
        | "it attacked during its controller's last turn"
        | "it attacked during your last turn"
        | "~'s power is even"
        | "~'s power is odd"
        | "that artifact is put into a graveyard this way"
        | "that creature is put into a graveyard this way"
        | "~ is still on the battlefield"
        | "they control two or more basic lands"
        | "they control a commander"
        | "an opponent has been dealt noncombat damage this turn"
        | "you've scried or surveilled this turn"
        | "~ is on the battlefield or in your graveyard"
        | "an opponent was dealt combat damage by a legendary creature this turn"
        | "it's a land or double-faced card"
        | "three or more creature cards are above ~"
        | "you've paid or lost four or more {e} this turn"
        | "red sources you controlled dealt 4 or more noncombat damage this turn"
        | "~ blocked or was blocked by a blue creature this turn"
        | "you've rolled three or more dice this turn"
        | "you've been attacked this step"
        | "you're attacking two or more opponents"
        | "at least one creature is blocking ~"
        | "ten or more lands are on the battlefield"
        | "there are twenty-five or more loyalty counters among jaces you control"
        | "mana from a treasure was spent to cast it or activate it"
        | "two or more equipment are attached to it"
        | "an enchantment was put into your graveyard from the battlefield this turn"
        | "~ regenerated this turn"
        | "it shares a card type with the card exiled this way"
        | "no player does"
        | "the player doesn't"
        | "you win"
        | "you paid one or more {e} this way"
        | "one or more +1/+1 counters are moved this way"
        | "two cards that share a card type are discarded this way"
        | "that creature was cast for its warp cost"
        | "that card is returned to its owner's hand this way"
        | "you gain control of a creature this way"
        | "a creature spell you cast this turn was countered by a spell or ability an opponent controlled"
        | "a player other than you lost life this turn"
        | "an enchantment is on the battlefield"
        | "excess damage was dealt to a permanent this way"
        | "~ is enchanted, equipped, and has a counter on it"
        | "this is the first or second time this ability has resolved this turn"
        | "they control fewer lands than you"
        | "the discovered card's mana value is less than 10"
        | "that creature shares a color with the mana that land produced"
        | "that spell targets a commander you control"
        | "you control two or more nonland, nontoken permanents with the same name as one another"
        | "your library has no cards in it"
        | "you control more creatures than attacking player"
        | "you control more lands than attacking player" => {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player: PlayerRefAst::You,
                action: low,
            });
        }
        "they're not a token" | "they aren't a token" => {
            return Ok(ConditionAst::IsNotToken);
        }
        "~ is in the command zone" => {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: "in_command_zone".to_string(),
            });
        }
        "it wasn't cast this way" | "you don't cast that card this way" => {
            return Ok(ConditionAst::Not(Box::new(
                ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: "cast_this_way".to_string(),
                },
            )));
        }
        "you haven't completed tomb of annihilation" => {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
                GameMarker::CompletedDungeon,
            ))));
        }
        _ => {}
    }

    if let Some(after_atk) = low.strip_prefix("you attacked with ")
        && let Some(cnt_str) = after_atk.strip_suffix(" or more creatures this turn")
        && let Some(n) = parse_number_word(cnt_str.trim())
        && n > 0
    {
        return Ok(ConditionAst::Marker(
            GameMarker::AttackedWithMinCreaturesThisTurn {
                player: PlayerRefAst::You,
                min_count: u16::try_from(n).unwrap_or(2),
            },
        ));
    }
    if low.starts_with("it's blocking or blocked by ")
        || low.starts_with("at least one ") && low.ends_with(" is milled this way")
        || low.starts_with("you won ") && low.ends_with(" flips this way")
        || low.starts_with("target player has fewer than ") && low.ends_with(" counters")
        || low.starts_with("you put fewer than ") && low.ends_with(" this way")
        || low.starts_with("those creatures have total power ") && low.ends_with(" or greater")
        || low.starts_with("an opponent has at least ")
            && low.ends_with(" more cards in hand than you")
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if let Some(flt_raw) = low
        .strip_suffix(" is in your graveyard")
        .or_else(|| low.strip_suffix(" is in a graveyard"))
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::GraveyardCount {
            player: if low.ends_with(" is in a graveyard") {
                PlayerRefAst::AnyPlayer
            } else {
                PlayerRefAst::You
            },
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // Orthogonal negated subtype/status/mana-spent/keyword conditions:
    // "it's not a <flt>", "it's not <status>", "<mana> wasn't spent to cast it/~", "it didn't have <kw>"
    if let Some(flt_raw) = low
        .strip_prefix("it's not ")
        .or_else(|| low.strip_prefix("it isn't "))
    {
        if matches!(
            flt_raw,
            "suspected"
                | "tapped"
                | "untapped"
                | "monstrous"
                | "renowned"
                | "saddled"
                | "crewed"
                | "goaded"
                | "blocked"
        ) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                status: flt_raw.to_string(),
            })));
        }
        if let Ok(flt) = parse_object_filter(flt_raw, vocab) {
            return Ok(ConditionAst::Not(Box::new(ConditionAst::ControlsCount {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: 1,
            })));
        }
    }
    if let Some(spent) = low
        .strip_suffix(" wasn't spent to cast it")
        .or_else(|| low.strip_suffix(" wasn't spent to cast ~"))
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::ManaSpentToCast(
            spent.trim().to_string(),
        ))));
    }
    if let Some(kw) = low.strip_prefix("it didn't have ")
        && parse_keyword_list(kw, vocab).is_some()
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            keyword: kw.to_string(),
        })));
    }
    if let Some(after) = low
        .strip_prefix("there's ")
        .or_else(|| low.strip_prefix("there is "))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        let flt_clean = flt_raw.replace(" total in all graveyards", " in all graveyards");
        if let Ok(flt) = parse_object_filter(&flt_clean, vocab) {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }
    if let Some((who_raw, flt_raw)) = low
        .split_once(" controls no ")
        .or_else(|| low.split_once(" control no "))
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some((who_raw, rest)) = low.split_once(" controls more ")
        && let Some(flt_raw) = rest.strip_suffix(" than you")
        && let (Some(_player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::OpponentMoreThanYou {
            metric: "permanents".to_string(),
            filter: Some(Box::new(flt)),
        });
    }
    if low.starts_with("you roll a ")
        || low.ends_with(" was beheld")
        || low.ends_with(" crewed it this turn")
        || low.ends_with(" attacked this combat")
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "<N> or more creatures died [under your control] this turn" / "<N> or more cards have been exiled with ~" / "<N> or more <flt> are in your graveyard"
    if let Some((cnt_raw, tail)) = low.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        if matches!(
            tail,
            "creatures died this turn"
                | "creatures died under your control this turn"
                | "cards have been exiled with ~"
        ) {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PriorResult),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some(flt_raw) = tail.strip_suffix(" are in your graveyard")
            && let Ok(flt) = parse_object_filter(&format!("{flt_raw} in your graveyard"), vocab)
        {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }
    // "there are fewer than <N> <flt>" (Shadowborn Demon)
    if let Some(after) = low.strip_prefix("there are fewer than ")
        && let Some((cnt_raw, flt_raw)) = after.split_once(' ')
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Lt,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    // "you sacrificed <N> or more <flt> this turn" / "<player> was dealt <N> or more [combat] damage this turn" / "you've been dealt <N> or more damage this turn" / "it has dealt <N> or more damage to that player this turn"
    if (low.starts_with("you sacrificed ") && low.ends_with(" this turn"))
        || (low.contains(" was dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("you've been dealt ") && low.ends_with(" damage this turn"))
        || (low.starts_with("it has dealt ") && low.ends_with(" damage to that player this turn"))
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "you have <N> or more cards in your library" / "you have exactly <N> cards in your hand"
    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(cnt_raw) = after.strip_suffix(" or more cards in your library")
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player: PlayerRefAst::You,
                    stat: PlayerStatKind::CardsInLibrary,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some(rest) = after.strip_prefix("exactly ")
            && let Some(cnt_raw) = rest
                .strip_suffix(" cards in your hand")
                .or_else(|| rest.strip_suffix(" cards in hand"))
            && let Some(n) = parse_number_word(cnt_raw)
        {
            return Ok(ConditionAst::HandCount {
                player: PlayerRefAst::You,
                op: CompareOp::Eq,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }
    // "<player> controls <flt>" (e.g. "that player controls a Plains") / "you control <subj>" / "that mana is spent on <flt>" / "that target is <colors>"
    if let Some((who_raw, flt_raw)) = low.split_once(" controls ")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::ControlsCount {
            player,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(subj_raw) = low.strip_prefix("you control ")
        && parse_subject_or_target(subj_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if low.starts_with("that mana is spent on ")
        || low.starts_with("any of that mana is spent on ")
        || low.starts_with("that target is ")
        || low.starts_with("~ is enchanted by ")
        || low.starts_with("they were attacked this turn by ")
        || (low.starts_with("you revealed ") && low.ends_with(" as you cast ~"))
        || (low.ends_with(" or more creatures are attacking")
            && parse_number_word(
                low.strip_suffix(" or more creatures are attacking")
                    .unwrap_or(""),
            )
            .is_some())
        || (low.contains(" have exactly ") && low.ends_with(" cards in hand"))
        || (low.ends_with(" cards were put into your graveyard from anywhere this turn"))
        || (low.starts_with("it's the first time ") && low.ends_with(" this turn"))
        || (low.starts_with("a player was dealt ") && low.ends_with(" this turn"))
        || (low.starts_with("you had another ") && low.ends_with(" last turn"))
        || low.starts_with("it doesn't have the same name as another ")
        || (low.contains(" or more mana was spent to cast "))
        || low == "another permanent with the same name is on the battlefield"
        || low == "it has the same name as a permanent"
        || low
            == "it has the greatest power or is tied for greatest power among creatures on the battlefield"
        || low == "you've controlled ~ continuously since the beginning of your most recent turn"
        || (low.starts_with("~ or another ")
            && low.ends_with(" entered the battlefield under your control this turn"))
        || (low.starts_with("there are ")
            && low.ends_with(" permanent types among cards in your graveyard"))
        || (low.starts_with("there are at least ") && low.ends_with(" in your graveyard"))
        || low.starts_with("it shares a color with ")
        || low.starts_with("an opponent controls at least ")
        || low == "the number of permanents is even"
        || low
            == "you've cast a spell from a graveyard or activated an ability of a card in a graveyard this turn"
        || low.starts_with("you both own and control ~ and ")
        || low == "it's your main phase"
        || low == "that spell's mana cost or that ability's activation cost contains {x}"
        || low == "you don't copy a spell this way"
        || low == "you don't cast a spell this way"
        || low == "you don't or can't make an exchange"
        || low == "it shares a card type with a card exiled with ~"
        || low == "that spell doesn't target a creature you control"
        || low == "a creature not named ~ died this turn"
        || low == "mana from a treasure was spent to activate this ability"
        || low == "you've cast both a creature spell and a noncreature spell this turn"
        || low == "a player has more life than each other player"
        || low == "it's an art sticker"
        || low == "you haven't activated an exhaust ability this turn"
        || ((low.starts_with("there is exactly ") || low.starts_with("there are exactly "))
            && low.contains(" counter"))
        || (low.starts_with("there are ") && low.contains(" or more counters among "))
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    // "no <flt> were <participle> this way" (Haunting Imitation, etc.)
    if let Some(after_no) = low.strip_prefix("no ")
        && let Some(before_tw) = after_no.strip_suffix(" this way")
    {
        for passive in [
            " were revealed",
            " were exiled",
            " were discarded",
            " were destroyed",
            " were sacrificed",
            " were milled",
            " were put onto the battlefield",
            " were put into your hand",
        ] {
            if let Some(flt_raw) = before_tw.strip_suffix(passive)
                && parse_object_filter(flt_raw.trim(), vocab).is_ok()
            {
                return Ok(ConditionAst::Not(Box::new(
                    ConditionAst::PlayerActionThisTurn {
                        player: PlayerRefAst::You,
                        action: low,
                    },
                )));
            }
        }
    }

    // Orthogonal "this way" resolution conditions:
    // "a <flt> was sacrificed/exiled/discarded/dealt damage this way",
    // "you put/discard/sacrifice/return <flt> ... this way", "an opponent created a token this way"
    if let Some(before_tw) = low.strip_suffix(" this way") {
        for passive in [
            " was sacrificed",
            " were sacrificed",
            " was discarded",
            " were discarded",
            " was exiled",
            " were exiled",
            " was destroyed",
            " were destroyed",
            " was countered",
            " was milled",
            " was revealed",
            " was created",
            " was returned",
            " were returned",
            " is destroyed",
            " is exiled",
            " is sacrificed",
            " is discarded",
            " is countered",
            " is returned",
            " is put onto the battlefield",
            " is put into a graveyard",
            " is put into your hand",
            " is dealt damage",
            " was dealt damage",
            " enters",
        ] {
            if let Some(flt_raw) = before_tw.strip_suffix(passive)
                && (parse_object_filter(flt_raw, vocab).is_ok()
                    || parse_player_ref(flt_raw).is_some())
            {
                return Ok(ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action: low,
                });
            }
        }
        for (pref, player) in [
            ("you put ", PlayerRefAst::You),
            ("you discard ", PlayerRefAst::You),
            ("you discarded ", PlayerRefAst::You),
            ("you sacrifice ", PlayerRefAst::You),
            ("you sacrificed ", PlayerRefAst::You),
            ("you exile ", PlayerRefAst::You),
            ("you exiled ", PlayerRefAst::You),
            ("you return ", PlayerRefAst::You),
            ("you returned ", PlayerRefAst::You),
            ("you create ", PlayerRefAst::You),
            ("you created ", PlayerRefAst::You),
            ("you mill ", PlayerRefAst::You),
            ("you milled ", PlayerRefAst::You),
            ("you reveal ", PlayerRefAst::You),
            ("you revealed ", PlayerRefAst::You),
            ("you cast ", PlayerRefAst::You),
            ("an opponent created ", PlayerRefAst::AnyOpponent),
            ("an opponent sacrificed ", PlayerRefAst::AnyOpponent),
            ("an opponent discarded ", PlayerRefAst::AnyOpponent),
        ] {
            if before_tw.starts_with(pref) {
                return Ok(ConditionAst::PlayerActionThisTurn {
                    player,
                    action: low,
                });
            }
        }
    }

    // Orthogonal turn-event conditions:
    // "a <flt> entered the battlefield under an opponent's control this turn"
    // "a <flt> left the battlefield [under your control] this turn"
    // "a <flt> was put into a graveyard from the battlefield this turn"
    // "any of that damage was dealt by a <flt>"
    if let Some(flt_raw) = low
        .strip_suffix(" entered the battlefield under an opponent's control this turn")
        .or_else(|| low.strip_suffix(" entered under an opponent's control this turn"))
        .or_else(|| low.strip_suffix(" left the battlefield under your control this turn"))
        .or_else(|| low.strip_suffix(" left the battlefield this turn"))
        .or_else(|| low.strip_suffix(" was put into a graveyard from the battlefield this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
            filter: Box::new(flt),
            min_count: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("any of that damage was dealt by ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }
    if low.starts_with("it was crewed by ") {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }

    // "<mana> was spent to cast it/~"
    if let Some(spent) = low
        .strip_suffix(" was spent to cast it")
        .or_else(|| low.strip_suffix(" was spent to cast ~"))
    {
        return Ok(ConditionAst::ManaSpentToCast(spent.trim().to_string()));
    }

    // "it was kicked with its {M} kicker" (CR 702.33c multi-slot kicker)
    if let Some(slot_raw) = low
        .strip_prefix("~ was kicked with its ")
        .or_else(|| low.strip_prefix("it was kicked with its "))
        && let Some(slot) = slot_raw.strip_suffix(" kicker")
    {
        return Ok(ConditionAst::WasKickedSlot(slot.trim().to_string()));
    }
    if low.starts_with("it was kicked") || low.starts_with("~ was kicked") {
        return Ok(ConditionAst::WasKicked);
    }
    // "the {M} cost was paid" (Devastating Mastery, alternative/additional cost checks)
    if let Some(after) = low.strip_prefix("the ")
        && let Some(cost_raw) = after.strip_suffix(" cost was paid")
    {
        return Ok(ConditionAst::ManaSpentToCast(cost_raw.trim().to_string()));
    }
    // "you control <flt> named <A> and <B>" / "you controlled <flt>" (Power Plant Worker, Throne of Empires, Orator of Ojutai)
    if let Some(after) = low.strip_prefix("you control ")
        && let Some((flt_raw, _names)) = after.split_once(" named ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("you controlled ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }
    if let Some(flt_raw) = low.strip_prefix("you revealed ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action: low,
        });
    }

    // "you win" / "you lose" / "you win the flip" / "you win the clash"
    if matches!(
        low.as_str(),
        "you win" | "you win the flip" | "you win the clash" | "you won the flip"
    ) {
        return Ok(ConditionAst::WonContest);
    }
    if matches!(
        low.as_str(),
        "you lose" | "you lose the flip" | "you lose the clash"
    ) {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::WonContest)));
    }

    // "it targets <filter>" / "~ targets <filter>"
    if let Some(flt_raw) = low
        .strip_prefix("it targets ")
        .or_else(|| low.strip_prefix("~ targets "))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }

    // "you attacked with creatures with total power N or greater this combat"
    if let Some(after) = low.strip_prefix("you attacked with ")
        && let Some((flt_raw, rhs)) = after.split_once(" with total power ")
        && let Some(n_tok) = rhs
            .strip_suffix(" or greater this combat")
            .or_else(|| rhs.strip_suffix(" or greater"))
        && let (Ok(flt), Some(n)) = (
            parse_object_filter(flt_raw, vocab),
            parse_number_word(n_tok),
        )
    {
        return Ok(ConditionAst::TotalStatCompare {
            filter: Box::new(flt),
            stat: StatKind::Power,
            op: CompareOp::Ge,
            value: n,
        });
    }

    // "<player> discarded a card this turn" / "<player> drew a card this turn" / "<player> lost life this turn"
    for (suf, act) in [
        (" discarded a card this turn", "discarded_card"),
        (" drew a card this turn", "drew_card"),
        (" drew two or more cards this turn", "drew_2plus"),
        (" gained life this turn", "gained_life"),
        (" lost life this turn", "lost_life"),
        (" sacrificed a permanent this turn", "sacrificed_permanent"),
        (" sacrificed a creature this turn", "sacrificed_creature"),
        (" attacked this turn", "attacked"),
    ] {
        if let Some(who_raw) = low.strip_suffix(suf)
            && let Some(player) = parse_player_ref(who_raw)
        {
            return Ok(ConditionAst::PlayerActionThisTurn {
                player,
                action: act.to_string(),
            });
        }
    }

    // "you have N or more opponents"
    if let Some(after) = low.strip_prefix("you have ")
        && let Some(cnt_raw) = after.strip_suffix(" or more opponents")
        && let Some(n) = parse_number_word(cnt_raw)
    {
        return Ok(ConditionAst::OpponentCountCompare {
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
        });
    }

    // "<filter> have total power/toughness N or greater"
    if let Some((flt_raw, rhs)) = low.split_once(" have total power ")
        && let Some(n_tok) = rhs.strip_suffix(" or greater")
        && let (Ok(flt), Some(n)) = (
            parse_object_filter(flt_raw, vocab),
            parse_number_word(n_tok),
        )
    {
        return Ok(ConditionAst::TotalStatCompare {
            filter: Box::new(flt),
            stat: StatKind::Power,
            op: CompareOp::Ge,
            value: n,
        });
    }

    // "<player> has/have N or more/less life" / "<player> has/have N or more/fewer/no cards in hand" / "<player> has/have N or more cards in their graveyard"
    if let Some((who_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if rhs == "no cards in hand"
            || rhs == "no cards in their hand"
            || rhs == "no cards in your hand"
        {
            return Ok(ConditionAst::HandCount {
                player,
                op: CompareOp::Eq,
                value: 0,
            });
        }
        if let Some(rest) = rhs
            .strip_suffix(" cards in hand")
            .or_else(|| rhs.strip_suffix(" cards in their hand"))
            .or_else(|| rhs.strip_suffix(" cards in your hand"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
        if let Some(rest) = rhs
            .strip_suffix(" cards in their graveyard")
            .or_else(|| rhs.strip_suffix(" cards in your graveyard"))
            .or_else(|| rhs.strip_suffix(" cards in graveyard"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::GraveyardCount {
                    player,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::GraveyardCount {
                    player,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
        if let Some(rest) = rhs.strip_suffix(" life more than your starting life total")
            && let Some(n_tok) = rest.strip_prefix("at least ")
            && let Ok(n) = n_tok.trim().parse::<i32>()
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::PlayerStat {
                    player,
                    stat: PlayerStatKind::LifeTotal,
                }),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Add(
                    Box::new(ValueExprAst::StartingLifeTotal(player)),
                    Box::new(ValueExprAst::Const(n)),
                )),
            });
        }
        if let Some(rest) = rhs.strip_suffix(" life") {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player,
                    op: CompareOp::Ge,
                    value: n,
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or less")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player,
                    op: CompareOp::Le,
                    value: n,
                });
            }
        }
        if let Some(before_cnt) = rhs
            .strip_suffix(" counters")
            .or_else(|| rhs.strip_suffix(" counter"))
            && let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::PlayerCounterCompare {
                player,
                counter_kind: kind.trim().to_string(),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            });
        }
    }

    // "<player>'s / a library has N or fewer cards in it"
    if let Some((who_raw, rest)) = low.split_once(" library has ")
        && let Some(n_tok) = rest.strip_suffix(" or fewer cards in it")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::Compare {
            lhs: Box::new(ValueExprAst::PlayerStat {
                player: parse_possessive_player(who_raw),
                stat: PlayerStatKind::CardsInLibrary,
            }),
            op: CompareOp::Le,
            rhs: Box::new(ValueExprAst::Const(n)),
        });
    }

    if let Some(rest) = low
        .strip_prefix("you've gained ")
        .or_else(|| low.strip_prefix("you gained "))
        && rest.ends_with(" life this turn")
    {
        return Ok(ConditionAst::GainedLifeThisTurn);
    }
    if let Some(rest) = low
        .strip_prefix("an opponent has cast ")
        .or_else(|| low.strip_prefix("an opponent cast "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::AnyOpponent,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low
        .strip_prefix("you've cast ")
        .or_else(|| low.strip_prefix("you cast "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter: Box::new(flt),
        });
    }
    if let Some(rest) = low.strip_prefix("an opponent controls more ")
        && let Some(flt_raw) = rest.strip_suffix(" than you")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::OpponentMoreThanYou {
            metric: "controls".to_string(),
            filter: Some(Box::new(flt)),
        });
    }

    if low.contains("seven or more cards") && low.contains("graveyard") {
        return Ok(ConditionAst::Threshold);
    }
    if low.contains("four or more card types among cards in your graveyard") {
        return Ok(ConditionAst::Delirium);
    }
    if low.contains("three or more artifacts") && low.contains("you control") {
        return Ok(ConditionAst::Metalcraft);
    }
    if low.contains("permanent you controlled left the battlefield this turn")
        || low.contains("permanent left the battlefield under your control this turn")
    {
        return Ok(ConditionAst::Revolt);
    }

    // "<player> lost N or more life this turn"
    if let Some((who_raw, rhs)) = low.split_once(" lost ")
        && let Some(n_tok) = rhs.strip_suffix(" or more life this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::LostLifeThisTurn {
            player,
            min_amount: n,
        });
    }

    // "<player> has drawn / drew N or more cards this turn"
    if let Some((who_raw, rest)) = low
        .split_once(" has drawn ")
        .or_else(|| low.split_once(" have drawn "))
        .or_else(|| low.split_once("'ve drawn "))
        .or_else(|| low.split_once(" drew "))
        && let Some(n_tok) = rest.strip_suffix(" or more cards this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
        }));
    }

    // "<player> cast / has cast N or more spells this turn"
    if let Some((who_raw, rest)) = low
        .split_once(" has cast ")
        .or_else(|| low.split_once(" have cast "))
        .or_else(|| low.split_once("'ve cast "))
        .or_else(|| low.split_once(" cast "))
        && let Some(n_tok) = rest.strip_suffix(" or more spells this turn")
        && let (Some(player), Some(n)) = (parse_player_ref(who_raw), parse_number_word(n_tok))
    {
        return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(2),
            from_hand_only: false,
        }));
    }

    // "<player> sacrificed / has sacrificed <filter> this turn"
    if let Some((who_raw, rest)) = low
        .split_once("'ve sacrificed ")
        .or_else(|| low.split_once(" has sacrificed "))
        .or_else(|| low.split_once(" sacrificed "))
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::SacrificedThisTurn {
            player,
            filter: Box::new(flt),
        }));
    }

    // "<filter> was put into your graveyard from anywhere this turn"
    if let Some(flt_raw) = low
        .strip_suffix(" was put into your graveyard from anywhere this turn")
        .or_else(|| low.strip_suffix(" was put into your graveyard this turn"))
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }

    // "your devotion to <colors> is less than <N>" / "<N> or greater"
    if let Some(after) = low.strip_prefix("your devotion to ")
        && let Some((colors, rhs)) = after.split_once(" is ")
    {
        if let Some(n_tok) = rhs.strip_prefix("less than ")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::DevotionCompare {
                colors: colors.trim().to_string(),
                op: CompareOp::Lt,
                value: u16::try_from(n.max(0)).unwrap_or(5),
            });
        }
        if let Some(n_tok) = rhs.strip_suffix(" or greater")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::DevotionCompare {
                colors: colors.trim().to_string(),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(5),
            });
        }
    }

    // "<N or more / a / an / another> <filter> entered [the battlefield] under your control this turn"
    if let Some(before_ent) = low
        .strip_suffix(" entered the battlefield under your control this turn")
        .or_else(|| low.strip_suffix(" entered under your control this turn"))
    {
        if let Some((cnt_tok, flt_raw)) = before_ent.split_once(" or more ")
            && let (Some(n), Ok(flt)) = (
                parse_number_word(cnt_tok),
                parse_object_filter(flt_raw, vocab),
            )
        {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: u16::try_from(n.max(1)).unwrap_or(1),
            });
        }
        if let Ok(flt) = parse_object_filter(before_ent, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: 1,
            });
        }
    }

    // "there is a/an <filter>" (e.g. "there is a Desert card in your graveyard")
    if let Some(flt_raw) = low.strip_prefix("there is ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "there are N or more mana values among cards in your graveyard"
    if let Some(after) = low.strip_prefix("there are ")
        && let Some(n_tok) =
            after.strip_suffix(" or more mana values among cards in your graveyard")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DifferentManaValuesInGraveyard {
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(4),
            },
        ));
    }

    // "<subject> doesn't have <keyword>" / "<subject> has <keyword>"
    if let Some((subj_raw, kw_raw)) = low.split_once(" doesn't have ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw_raw.trim().to_string(),
        })));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        && vocab.is_evergreen_keyword(kw_raw.trim())
    {
        return Ok(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw_raw.trim().to_string(),
        });
    }

    // Zone location conditions: "~ is in your graveyard", "~ is in your hand", "~ is in exile"
    for (phrase_lit, subj, zone) in [
        (
            "~ is in your graveyard",
            SubjectRefAst::SelfObject,
            ZoneFilter::YourGraveyard,
        ),
        (
            "~ is in your hand",
            SubjectRefAst::SelfObject,
            ZoneFilter::YourHand,
        ),
        (
            "~ is in exile",
            SubjectRefAst::SelfObject,
            ZoneFilter::Exile,
        ),
        (
            "~ is on the battlefield",
            SubjectRefAst::SelfObject,
            ZoneFilter::Battlefield,
        ),
        (
            "it is in your graveyard",
            SubjectRefAst::Anaphora("it".to_string()),
            ZoneFilter::YourGraveyard,
        ),
        (
            "it's in your graveyard",
            SubjectRefAst::Anaphora("it".to_string()),
            ZoneFilter::YourGraveyard,
        ),
    ] {
        if low == phrase_lit {
            return Ok(ConditionAst::Marker(GameMarker::InZone {
                subject: Box::new(subj),
                zone,
            }));
        }
    }

    for status in [
        "tapped",
        "untapped",
        "attacking",
        "blocking",
        "blocked",
        "enchanted",
        "equipped",
        "modified",
        "monstrous",
        "renowned",
        "suspected",
        "saddled",
        "solved",
        "prepared",
        "face down",
        "kicked",
    ] {
        if let Some(subj_raw) = low.strip_suffix(&format!(" is {status}"))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(subj),
                status: status.to_string(),
            });
        }
        if low == format!("it's {status}")
            || low == format!("he's {status}")
            || low == format!("she's {status}")
        {
            return Ok(ConditionAst::HasStatus {
                subject: Box::new(SubjectRefAst::SelfObject),
                status: status.to_string(),
            });
        }
    }

    // "you controlled <subj>" / "this ability has been activated N or more times this turn"
    if let Some(subj_raw) = low.strip_prefix("you controlled ")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::ControlledSubject(
            Box::new(subj),
        )));
    }
    if let Some(rest) = low.strip_prefix("this ability has been activated ")
        && let Some(cnt_raw) = rest
            .strip_suffix(" times this turn")
            .or_else(|| rest.strip_suffix(" time this turn"))
    {
        let (n_tok, op) = if let Some(v) = cnt_raw.strip_suffix(" or more") {
            (v, CompareOp::Ge)
        } else if let Some(v) = cnt_raw.strip_suffix(" or fewer") {
            (v, CompareOp::Le)
        } else {
            (cnt_raw, CompareOp::Eq)
        };
        if let Some(n) = parse_number_word(n_tok) {
            return Ok(ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn {
                op,
                value: u16::try_from(n.max(0)).unwrap_or(1),
            }));
        }
    }

    // Generalized "<player> is the monarch" / "<player> didn't cast <filter> this turn"
    // "<player> has more cards in hand than <player2>" / "<subj> entered this turn" / "<subj> was/wasn't cast" / "<subj> is exiled"
    if let Some(who_raw) = low.strip_suffix(" is the monarch")
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(ConditionAst::Marker(GameMarker::Monarch(player)));
    }
    if let Some((who_raw, rest)) = low.split_once(" didn't cast ")
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(player), Ok(flt)) = (
            parse_player_ref(who_raw),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player,
            filter: Some(Box::new(flt)),
        }));
    }
    if let Some((p1_raw, p2_raw)) = low.split_once(" has more cards in hand than ")
        && let (Some(p1), Some(p2)) = (parse_player_ref(p1_raw), parse_player_ref(p2_raw))
    {
        return Ok(ConditionAst::Marker(GameMarker::PlayerHasMoreHandThan {
            player: p1,
            than: p2,
        }));
    }
    if let Some(subj_raw) = low
        .strip_suffix(" entered this turn")
        .or_else(|| low.strip_suffix(" entered the battlefield this turn"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(
            Box::new(subj),
        )));
    }
    if let Some(subj_raw) = low.strip_suffix(" wasn't cast")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectWasCast {
            subject: Box::new(subj),
            was_cast: false,
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" was cast")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::SubjectWasCast {
            subject: Box::new(subj),
            was_cast: true,
        }));
    }
    if let Some(subj_raw) = low.strip_suffix(" is exiled")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(GameMarker::InZone {
            subject: Box::new(subj),
            zone: ZoneFilter::Exile,
        }));
    }
    // "a graveyard has N or more cards in it" / "N or more cards are in your graveyard"
    if let Some(rest) = low.strip_prefix("a graveyard has ")
        && let Some(n_tok) = rest.strip_suffix(" or more cards in it")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::GraveyardCount {
            player: PlayerRefAst::AnyPlayer,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    if let Some(n_tok) = low.strip_suffix(" or more cards are in your graveyard")
        && let Some(n) = parse_number_word(n_tok)
    {
        return Ok(ConditionAst::GraveyardCount {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    // "<player> has/have exactly N life" / "fewer than N cards in hand"
    if let Some((who_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" have "))
        && let Some(player) = parse_player_ref(who_raw)
    {
        if let Some(rest) = rhs.strip_prefix("exactly ")
            && let Some(n_tok) = rest.strip_suffix(" life")
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::LifeCompare {
                player,
                op: CompareOp::Eq,
                value: n,
            });
        }
        if let Some(rest) = rhs.strip_prefix("fewer than ")
            && let Some(n_tok) = rest
                .strip_suffix(" cards in hand")
                .or_else(|| rest.strip_suffix(" cards in your hand"))
            && let Some(n) = parse_number_word(n_tok)
        {
            return Ok(ConditionAst::HandCount {
                player,
                op: CompareOp::Lt,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }

    // "<player> control(s) [don't control] [at least N | N or more/fewer] <filter>"
    for (prefix, player, neg) in [
        ("you don't control ", PlayerRefAst::You, true),
        ("you control ", PlayerRefAst::You, false),
        ("any player controls ", PlayerRefAst::AnyPlayer, false),
        ("no opponent controls ", PlayerRefAst::EachOpponent, true),
        ("no player controls ", PlayerRefAst::AnyPlayer, true),
        (
            "your opponents don't control ",
            PlayerRefAst::EachOpponent,
            true,
        ),
        ("your opponents control ", PlayerRefAst::EachOpponent, false),
        ("opponents control ", PlayerRefAst::EachOpponent, false),
        (
            "defending player doesn't control ",
            PlayerRefAst::DefendingPlayer,
            true,
        ),
        (
            "defending player controls ",
            PlayerRefAst::DefendingPlayer,
            false,
        ),
        (
            "an opponent doesn't control ",
            PlayerRefAst::AnyOpponent,
            true,
        ),
        ("an opponent controls ", PlayerRefAst::AnyOpponent, false),
        (
            "another player controls ",
            PlayerRefAst::EachOtherPlayer,
            false,
        ),
        ("your team controls ", PlayerRefAst::You, false),
    ] {
        if let Some(rest) = low.strip_prefix(prefix) {
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("exactly one ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Eq,
                    value: 1,
                });
            }
            if !neg
                && let Some(flt_raw) = rest.strip_prefix("no ")
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if let Some(after_al) = rest.strip_prefix("at least ")
                && let Some((cnt_tok, flt_raw)) = after_al.split_once(' ')
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
            if let Some((cnt_tok, flt_raw)) = rest.split_once(" or more ")
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                });
            }
            if let Some((cnt_tok, flt_raw)) = rest.split_once(" or fewer ")
                && let Some(n) = parse_number_word(cnt_tok)
                && let Ok(flt) = parse_object_filter(flt_raw, vocab)
            {
                return Ok(ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Ok(flt) = parse_object_filter(rest, vocab) {
                let c = ConditionAst::ControlsCount {
                    player,
                    filter: Box::new(flt),
                    op: CompareOp::Ge,
                    value: 1,
                };
                return Ok(if neg {
                    ConditionAst::Not(Box::new(c))
                } else {
                    c
                });
            }
        }
    }

    // "there are no [<kind>] counters on <subj>" / "there are N or more <kind> counters on <subj>"
    if let Some(after) = low.strip_prefix("there are no ")
        && let Some((kind_part, subj_raw)) = after
            .split_once(" counters on ")
            .or_else(|| after.split_once(" counter on "))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: if kind_part.trim().is_empty() {
                "any".to_string()
            } else {
                kind_part.trim().to_string()
            },
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, rest)) = after.split_once(" or more ")
        && let Some((kind_part, subj_raw)) = rest
            .split_once(" counters on ")
            .or_else(|| rest.split_once(" counter on "))
        && let (Some(n), Ok(subj)) = (
            parse_number_word(cnt_tok),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: kind_part.trim().to_string(),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }

    // "there are no <filter>" / "there are <filter>" / "there are N or more <filter>"
    if let Some(flt_raw) = low.strip_prefix("there are no ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Eq,
            value: 0,
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_tok, flt_raw)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_tok)
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(1),
        });
    }
    if let Some(flt_raw) = low.strip_prefix("there are ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(flt),
            op: CompareOp::Ge,
            value: 1,
        });
    }

    // "you have N or more/less life" / "you have N or more/fewer cards in hand"
    if let Some(after) = low.strip_prefix("you have ") {
        if let Some(rest) = after.strip_suffix(" life") {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player: PlayerRefAst::You,
                    op: CompareOp::Ge,
                    value: n,
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or less")
                && let Ok(n) = n_tok.parse::<i32>()
            {
                return Ok(ConditionAst::LifeCompare {
                    player: PlayerRefAst::You,
                    op: CompareOp::Le,
                    value: n,
                });
            }
        }
        if let Some(rest) = after
            .strip_suffix(" cards in hand")
            .or_else(|| after.strip_suffix(" cards in your hand"))
        {
            if let Some(n_tok) = rest.strip_suffix(" or more")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player: PlayerRefAst::You,
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
            if let Some(n_tok) = rest.strip_suffix(" or fewer")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::HandCount {
                    player: PlayerRefAst::You,
                    op: CompareOp::Le,
                    value: u16::try_from(n.max(0)).unwrap_or(0),
                });
            }
        }
    }

    // "<filter> is/was milled/discarded/exiled/revealed this way"
    for (suf, act) in [
        (" is milled this way", "milled"),
        (" was milled this way", "milled"),
        (" is discarded this way", "discarded"),
        (" was discarded this way", "discarded"),
        (" is exiled this way", "exiled"),
        (" was exiled this way", "exiled"),
        (" is revealed this way", "revealed"),
        (" was revealed this way", "revealed"),
    ] {
        if let Some(flt_raw) = low.strip_suffix(suf)
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::Marker(
                GameMarker::ZoneActionHappenedThisWay {
                    filter: Box::new(flt),
                    action: act.to_string(),
                },
            ));
        }
    }

    // "<subject> isn't <filter>" / "<subject> is/was <filter>" (fully generic across any SubjectRefAst!)
    if let Some((subj_raw, rest)) = low
        .split_once(" isn't ")
        .or_else(|| low.split_once(" wasn't "))
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        })));
    }
    if let Some(rest) = low.strip_prefix("it's ")
        && let Ok(flt) = parse_object_filter(rest, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
            filter: Box::new(flt),
        });
    }
    if let Some((subj_raw, rest)) = low.split_once(" is ").or_else(|| low.split_once(" was "))
        && !rest.starts_with("greater than")
        && !rest.starts_with("less than")
        && !rest.starts_with("equal to")
        && !rest.ends_with(" or greater")
        && !rest.ends_with(" or less")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(rest, vocab),
        )
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        });
    }

    // "<subj>'s power/toughness/mana value is N or less/greater"
    for (stat_mid, stat) in [
        ("'s mana value is ", StatKind::ManaValue),
        ("'s power is ", StatKind::Power),
        ("'s toughness is ", StatKind::Toughness),
    ] {
        if let Some((subj_raw, rest)) = low.split_once(stat_mid)
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                (v, CompareOp::Ge)
            } else if let Some(v) = rest.strip_suffix(" or less") {
                (v, CompareOp::Le)
            } else {
                (rest, CompareOp::Eq)
            };
            if let Ok(n) = n_tok.trim().parse::<i32>() {
                return Ok(ConditionAst::StatCompare {
                    subject: Box::new(subj),
                    stat,
                    op,
                    value: n,
                });
            }
        }
    }
    for (prefix, stat) in [
        ("its mana value is ", StatKind::ManaValue),
        ("its power is ", StatKind::Power),
        ("its toughness is ", StatKind::Toughness),
    ] {
        if let Some(rest) = low.strip_prefix(prefix) {
            let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                (v, CompareOp::Ge)
            } else if let Some(v) = rest.strip_suffix(" or less") {
                (v, CompareOp::Le)
            } else {
                (rest, CompareOp::Eq)
            };
            if let Ok(n) = n_tok.trim().parse::<i32>() {
                return Ok(ConditionAst::StatCompare {
                    subject: Box::new(SubjectRefAst::Anaphora("it".to_string())),
                    stat,
                    op,
                    value: n,
                });
            }
        }
    }

    // "<subject> has/had power/toughness/mana value/counters"
    if let Some((subj_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" had ")) {
        for (st_pref, stat) in [
            ("power ", StatKind::Power),
            ("toughness ", StatKind::Toughness),
            ("mana value ", StatKind::ManaValue),
        ] {
            if let Some(rest) = rhs.strip_prefix(st_pref) {
                let (n_tok, op) = if let Some(v) = rest.strip_suffix(" or greater") {
                    (v, CompareOp::Ge)
                } else if let Some(v) = rest.strip_suffix(" or less") {
                    (v, CompareOp::Le)
                } else {
                    continue;
                };
                if let Ok(n) = n_tok.parse::<i32>()
                    && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
                {
                    return Ok(ConditionAst::StatCompare {
                        subject: Box::new(subj),
                        stat,
                        op,
                        value: n,
                    });
                }
            }
        }
        // "<subject> has/had [no|a|an|one or more|N or more] [<kind>] counter(s) on it"
        if let Some(before_on) = rhs.strip_suffix(" on it")
            && let Some(before_cnt) = before_on
                .strip_suffix(" counters")
                .or_else(|| before_on.strip_suffix(" counter"))
                .or_else(|| (before_on == "counters").then_some(""))
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            if let Some(no_kind) = before_cnt.strip_prefix("no ") {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: if no_kind.trim().is_empty() {
                        "any".to_string()
                    } else {
                        no_kind.trim().to_string()
                    },
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if before_cnt == "no" {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: "any".to_string(),
                    op: CompareOp::Eq,
                    value: 0,
                });
            }
            if let Some((n_tok, kind)) = before_cnt.split_once(" or more ")
                && let Some(n) = parse_number_word(n_tok)
            {
                return Ok(ConditionAst::CounterCompare {
                    subject: Box::new(subj),
                    counter_kind: kind.trim().to_string(),
                    op: CompareOp::Ge,
                    value: u16::try_from(n.max(0)).unwrap_or(1),
                });
            }
            let kind = before_cnt
                .strip_prefix("one or more ")
                .or_else(|| before_cnt.strip_prefix("a "))
                .or_else(|| before_cnt.strip_prefix("an "))
                .unwrap_or(before_cnt);
            let kclean =
                if kind.trim().is_empty() || kind.trim() == "one or more" || kind.trim() == "a" {
                    "any"
                } else {
                    kind.trim()
                };
            return Ok(ConditionAst::CounterCompare {
                subject: Box::new(subj),
                counter_kind: kclean.to_string(),
                op: CompareOp::Ge,
                value: 1,
            });
        }
    }

    // "<subject> doesn't have a <kind> counter on it"
    if let Some((subj_raw, rhs)) = low.split_once(" doesn't have ")
        && let Some(before_on) = rhs.strip_suffix(" on it")
        && let Some(before_cnt) = before_on
            .strip_suffix(" counters")
            .or_else(|| before_on.strip_suffix(" counter"))
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        let kind = before_cnt
            .strip_prefix("a ")
            .or_else(|| before_cnt.strip_prefix("an "))
            .unwrap_or(before_cnt)
            .trim();
        return Ok(ConditionAst::CounterCompare {
            subject: Box::new(subj),
            counter_kind: if kind.is_empty() {
                "any".to_string()
            } else {
                kind.to_string()
            },
            op: CompareOp::Eq,
            value: 0,
        });
    }

    // "<subject> has/had power/toughness/mana value greater/less than <ValueExpr>" (Drizzt Do'Urden, etc.)
    if let Some((subj_raw, rhs)) = low.split_once(" has ").or_else(|| low.split_once(" had ")) {
        for (st_pref, stat) in [
            ("power ", StatKind::Power),
            ("toughness ", StatKind::Toughness),
            ("mana value ", StatKind::ManaValue),
        ] {
            if let Some(rest) = rhs.strip_prefix(st_pref)
                && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
            {
                let cmp_opt = rest
                    .strip_prefix("greater than or equal to ")
                    .map(|r| (CompareOp::Ge, r))
                    .or_else(|| {
                        rest.strip_prefix("less than or equal to ")
                            .map(|r| (CompareOp::Le, r))
                    })
                    .or_else(|| {
                        rest.strip_prefix("greater than ")
                            .map(|r| (CompareOp::Gt, r))
                    })
                    .or_else(|| rest.strip_prefix("less than ").map(|r| (CompareOp::Lt, r)));
                if let Some((op, val_raw)) = cmp_opt
                    && let Ok(val) = parse_value_expr(val_raw, vocab)
                {
                    return Ok(ConditionAst::Compare {
                        lhs: Box::new(ValueExprAst::ObjectStat {
                            subject: Box::new(subj),
                            stat,
                        }),
                        op,
                        rhs: Box::new(val),
                    });
                }
            }
        }
    }

    // Pass N orthogonal conditions:
    // - "you have exactly N cards in hand" (Library of Alexandria)
    // - "you were dealt N or more damage this turn" (Boarded Window)
    // - "a <kind> counter was put on <flt> this turn" (Fairgrounds Trumpeter)
    // - "<flt> died under your control this turn" (White Glove Gourmand)
    // - "you attacked with <flt> this turn" (Avengers Assemble!)
    // - "it targets one or more <flt>" (Orvar)
    // - "~ is attached to <flt>" / "~ is enchanted by <flt>" (Summoning Materia, Timber Paladin)
    // - "the top card of your library is <flt/color>" (Vampire Nocturnus, Crown of Convergence)
    // - "there are N or more [card types | <kind> counters] among <flt>" (Keen-Eyed Curator, Backstreet Bruiser)
    // - "you've cast N or more <flt> spells this turn" (Lyse Hext)
    // - "his/its/the {M}/<kw> cost was paid" (Leonardo, Devastating Mastery)
    if let Some(after) = low.strip_prefix("you have exactly ")
        && let Some(cnt_raw) = after.strip_suffix(" cards in hand")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        return Ok(ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }
    if let Some(after) = low.strip_prefix("you were dealt ")
        && let Some(cnt_raw) = after.strip_suffix(" or more damage this turn")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DamageDealtToSubjectThisTurn {
                subject: Box::new(SubjectRefAst::Player(PlayerRefAst::You)),
                min_amount: u16::try_from(n.max(1)).unwrap_or(1),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("a ")
        && let Some((ck, rest)) = after.split_once(" counter was put on ")
        && rest.ends_with(" this turn")
    {
        return Ok(ConditionAst::Marker(
            GameMarker::PutCountersOnSubjectThisTurn {
                player: PlayerRefAst::You,
                subject: Box::new(SubjectRefAst::Anaphora(rest.to_string())),
                counter_kind: ck.trim().to_string(),
            },
        ));
    }
    if let Some(flt_raw) = low.strip_suffix(" died under your control this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you attacked with ")
        && let Some(flt_raw) = after.strip_suffix(" this turn")
        && let Ok(_flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::AttackedThisTurn);
    }
    if let Some(after) = low
        .strip_prefix("it targets one or more ")
        .or_else(|| low.strip_prefix("that spell targets one or more "))
        && let Ok(flt) = parse_object_filter(after, vocab)
    {
        return Ok(ConditionAst::TargetsFilter(Box::new(flt)));
    }
    if let Some(after) = low
        .strip_prefix("~ is attached to ")
        .or_else(|| low.strip_prefix("~ is enchanted by "))
        && !after.is_empty()
    {
        return Ok(ConditionAst::HasStatus {
            subject: Box::new(SubjectRefAst::SelfObject),
            status: format!("attached:{after}"),
        });
    }
    if let Some(pred_raw) = low.strip_prefix("the top card of your library is ")
        && let Ok(flt) = parse_object_filter(pred_raw, vocab)
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(SubjectRefAst::Anaphora("top_of_library".to_string())),
            filter: Box::new(flt),
        });
    }
    if let Some(after) = low.strip_prefix("there are ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(n) = parse_number_word(cnt_raw.trim())
    {
        if let Some(flt_raw) = rest.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::Compare {
                lhs: Box::new(ValueExprAst::CardTypesAmong(Box::new(flt))),
                op: CompareOp::Ge,
                rhs: Box::new(ValueExprAst::Const(n)),
            });
        }
        if let Some((_, flt_raw)) = rest.split_once(" counters among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ConditionAst::ZoneObjectCount {
                filter: Box::new(flt),
                op: CompareOp::Ge,
                value: u16::try_from(n.max(0)).unwrap_or(0),
            });
        }
    }
    if let Some(after) = low.strip_prefix("you've cast ")
        && let Some((cnt_raw, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) = rest.strip_suffix(" this turn")
        && let (Some(n), Ok(_flt)) = (
            parse_number_word(cnt_raw.trim()),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(0),
            from_hand_only: false,
        }));
    }
    if let Some(cost_kind) = low
        .strip_prefix("the ")
        .or_else(|| low.strip_prefix("his "))
        .or_else(|| low.strip_prefix("her "))
        .or_else(|| low.strip_prefix("~'s "))
        .and_then(|r| r.strip_suffix(" cost was paid"))
        && !cost_kind.is_empty()
    {
        return Ok(ConditionAst::Marker(GameMarker::KeywordCostPaid(
            cost_kind.to_string(),
        )));
    }

    // Pass M orthogonal conditions: "you don't put ... into your hand this way",
    // "damage [from <flt>] is prevented this way", "most common color", "you have N or more <flt> in your graveyard"
    if low.starts_with("you don't put ") && low.ends_with(" into your hand this way") {
        return Ok(ConditionAst::Marker(GameMarker::DidNotPutToZone {
            zone: "your hand".to_string(),
        }));
    }
    if low.starts_with("damage ") && low.ends_with(" is prevented this way") {
        return Ok(ConditionAst::Marker(GameMarker::DamagePreventedThisWay));
    }
    if low.ends_with(" is the most common color among all permanents or is tied for most common")
        || low.ends_with(" is the most common color among all permanents")
    {
        return Ok(ConditionAst::Marker(GameMarker::ControlsGreatestStat {
            player: PlayerRefAst::AnyPlayer,
            filter: Box::new(ObjectFilterAst {
                any_permanent: true,
                ..Default::default()
            }),
            stat: StatKind::ManaValue,
        }));
    }
    if let Some(after) = low.strip_prefix("you have ")
        && let Some((cnt_part, rest)) = after.split_once(" or more ")
        && let Some(flt_raw) = rest.strip_suffix(" in your graveyard")
        && let (Some(n), Ok(flt)) = (
            parse_number_word(cnt_part.trim()),
            parse_object_filter(flt_raw, vocab),
        )
    {
        let mut gflt = flt;
        gflt.zone = Some(ZoneFilter::YourGraveyard);
        return Ok(ConditionAst::ZoneObjectCount {
            filter: Box::new(gflt),
            op: CompareOp::Ge,
            value: u16::try_from(n.max(0)).unwrap_or(0),
        });
    }

    // Generic voting result condition: "<option> gets more votes [or the vote is tied]"
    if let Some(opt) = low
        .strip_suffix(" gets more votes or the vote is tied")
        .or_else(|| low.strip_suffix(" gets more votes"))
        && !opt.is_empty()
    {
        let tied = low.ends_with(" or the vote is tied");
        return Ok(ConditionAst::Marker(GameMarker::VoteResult {
            winning: if tied {
                format!("{opt}_or_tie")
            } else {
                opt.to_string()
            },
        }));
    }

    // Orthogonal Pass I conditions (turn history, zone membership, stat comparisons, keyword checks)
    if low == "you created a token this turn" || low == "you created one or more tokens this turn" {
        return Ok(ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }));
    }
    if low == "you've lost life this turn" || low == "you lost life this turn" {
        return Ok(ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount: 1,
        });
    }
    if low == "you've played a land or cast a spell this turn from anywhere other than your hand" {
        return Ok(ConditionAst::Marker(
            GameMarker::LandOrSpellOutsideHandThisTurn {
                player: PlayerRefAst::You,
            },
        ));
    }
    if let Some(subj_raw) = low.strip_suffix(" isn't on the battlefield")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Not(Box::new(ConditionAst::Marker(
            GameMarker::InZone {
                subject: Box::new(subj),
                zone: ZoneFilter::Battlefield,
            },
        ))));
    }
    if let Some(flt_raw) =
        low.strip_suffix(" was put into a graveyard from the battlefield this turn")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::CardPutIntoGraveyardThisTurn {
                filter: Box::new(flt),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("you had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under your control this turn")
    {
        let clean = flt_raw
            .strip_prefix("a ")
            .or_else(|| flt_raw.strip_prefix("an "))
            .or_else(|| flt_raw.strip_prefix("one or more "))
            .unwrap_or(flt_raw);
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count: 1,
            });
        }
    }
    if let Some(after) = low.strip_prefix("an opponent had ")
        && let Some(flt_raw) =
            after.strip_suffix(" enter the battlefield under their control this turn")
    {
        let (min_count, clean) = if let Some(r) = flt_raw.strip_prefix("two or more ") {
            (2u16, r)
        } else if let Some(r) = flt_raw.strip_prefix("three or more ") {
            (3u16, r)
        } else {
            (
                1u16,
                flt_raw
                    .strip_prefix("a ")
                    .or_else(|| flt_raw.strip_prefix("an "))
                    .unwrap_or(flt_raw),
            )
        };
        if let Ok(flt) = parse_object_filter(clean, vocab) {
            return Ok(ConditionAst::EnteredUnderYourControlThisTurn {
                filter: Box::new(flt),
                min_count,
            });
        }
    }
    if let Some((num_raw, tail)) = low.split_once(" or more damage was dealt to ")
        && let Some(subj_raw) = tail.strip_suffix(" this turn")
        && let (Some(n), Ok(subj)) = (
            parse_number_word(num_raw.trim()),
            parse_subject_or_target(subj_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(
            GameMarker::DamageDealtToSubjectThisTurn {
                subject: Box::new(subj),
                min_amount: u16::try_from(n.max(1)).unwrap_or(1),
            },
        ));
    }
    if let Some(kw) = low
        .strip_prefix("its ")
        .and_then(|r| r.strip_suffix(" cost was paid"))
        && !kw.is_empty()
    {
        return Ok(ConditionAst::Marker(GameMarker::KeywordCostPaid(
            kw.to_string(),
        )));
    }
    if let Some(after) = low.strip_prefix("you control the ")
        && let Some((flt_raw, rest)) = after.split_once(" with the greatest ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        let stat = if rest.starts_with("mana value") {
            Some(StatKind::ManaValue)
        } else if rest.starts_with("power") {
            Some(StatKind::Power)
        } else if rest.starts_with("toughness") {
            Some(StatKind::Toughness)
        } else {
            None
        };
        if let Some(stat) = stat {
            return Ok(ConditionAst::Marker(GameMarker::ControlsGreatestStat {
                player: PlayerRefAst::You,
                filter: Box::new(flt),
                stat,
            }));
        }
    }
    if let Some((subj_raw, rest)) = low.split_once(" is the only ")
        && let Some(flt_raw) = rest.strip_suffix(" in your graveyard")
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::OnlyObjectInZone {
            subject: Box::new(subj),
            filter: Box::new(flt),
            zone: ZoneFilter::YourGraveyard,
        }));
    }
    if let Some(after) = low.strip_prefix("you've put one or more ")
        && let Some((ck, tail)) = after.split_once(" counters on ")
        && let Some(subj_raw) = tail.strip_suffix(" this turn")
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::PutCountersOnSubjectThisTurn {
                player: PlayerRefAst::You,
                subject: Box::new(subj),
                counter_kind: ck.trim().to_string(),
            },
        ));
    }
    if let Some(after) = low.strip_prefix("all ")
        && let Some((flt_raw, pred_raw)) = after.split_once(" are ")
        && let (Ok(flt), Ok(pred)) = (
            parse_object_filter(flt_raw, vocab),
            parse_object_filter(pred_raw, vocab),
        )
    {
        return Ok(ConditionAst::Marker(GameMarker::AllMatchingAre {
            filter: Box::new(flt),
            predicate: Box::new(pred),
        }));
    }
    if let Some(after) = low.strip_prefix("a player controls more ")
        && let Some(flt_raw) = after.strip_suffix(" than each other player")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ConditionAst::Marker(
            GameMarker::ControlsMoreThanOpponents {
                filter: Box::new(flt),
                than_each_other: true,
            },
        ));
    }
    if let Some((subj_raw, kw_raw)) = low.split_once(" has ")
        && let (Ok(subj), Some(kws)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_keyword_list(kw_raw, vocab),
        )
        && let Some(kw) = kws.into_iter().next()
    {
        return Ok(ConditionAst::HasKeyword {
            subject: Box::new(subj),
            keyword: kw,
        });
    }
    if let Some((subj_raw, flt_raw)) = low
        .split_once(" was a ")
        .or_else(|| low.split_once(" was an "))
        .or_else(|| low.split_once(" were a "))
        .or_else(|| low.split_once(" were an "))
        && let (Ok(subj), Ok(flt)) = (
            parse_subject_or_target(subj_raw, vocab),
            parse_object_filter(flt_raw, vocab),
        )
    {
        return Ok(ConditionAst::MatchesFilter {
            subject: Box::new(subj),
            filter: Box::new(flt),
        });
    }

    // Orthogonal numeric comparison across ANY two ValueExprAst expressions (present OR past tense "is"/"was"):
    // Also supports elliptical possessive RHS ("that creature's power is greater than ~'s")!
    if let Some((lhs_raw, rhs_raw)) = low.split_once(" is ").or_else(|| low.split_once(" was "))
        && let Ok(lhs) = parse_value_expr(lhs_raw, vocab)
    {
        let parsed_cmp = if let Some(r) = rhs_raw.strip_prefix("greater than or equal to ") {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = rhs_raw.strip_prefix("less than or equal to ") {
            Some((CompareOp::Le, r))
        } else if let Some(r) = rhs_raw.strip_prefix("greater than ") {
            Some((CompareOp::Gt, r))
        } else if let Some(r) = rhs_raw.strip_prefix("less than ") {
            Some((CompareOp::Lt, r))
        } else if let Some(r) = rhs_raw.strip_prefix("equal to ") {
            Some((CompareOp::Eq, r))
        } else if let Some(r) = rhs_raw.strip_prefix("exactly ") {
            Some((CompareOp::Eq, r))
        } else if let Some(r) = rhs_raw
            .strip_suffix(" or greater")
            .or_else(|| rhs_raw.strip_suffix(" or more"))
        {
            Some((CompareOp::Ge, r))
        } else if let Some(r) = rhs_raw
            .strip_suffix(" or less")
            .or_else(|| rhs_raw.strip_suffix(" or fewer"))
        {
            Some((CompareOp::Le, r))
        } else {
            Some((CompareOp::Eq, rhs_raw))
        };
        if let Some((op, r_str)) = parsed_cmp {
            let rhs_res = parse_value_expr(r_str, vocab).or_else(|_| {
                if let ValueExprAst::ObjectStat { stat, .. } = &lhs
                    && let Some(owner_raw) = r_str.strip_suffix("'s")
                    && let Ok(owner_subj) = parse_subject_or_target(owner_raw, vocab)
                {
                    Ok(ValueExprAst::ObjectStat {
                        subject: Box::new(owner_subj),
                        stat: *stat,
                    })
                } else {
                    Err("no_rhs".to_string())
                }
            });
            if let Ok(rhs) = rhs_res {
                return Ok(ConditionAst::Compare {
                    lhs: Box::new(lhs),
                    op,
                    rhs: Box::new(rhs),
                });
            }
        }
    }

    // Compound boolean conditions ("<c1> and <c2>" / "<c1> or [if] <c2>"), with shared subject carry-forward ("you control a Squirrel or returned ...")
    if let Some((l, r)) = low
        .split_once(" and if ")
        .or_else(|| low.split_once(" and "))
    {
        if let (Ok(lc), Ok(rc)) = (parse_condition(l, vocab), parse_condition(r, vocab)) {
            return Ok(ConditionAst::And(Box::new(lc), Box::new(rc)));
        }
        if l.starts_with("you ")
            && let (Ok(lc), Ok(rc)) = (
                parse_condition(l, vocab),
                parse_condition(&format!("you {r}"), vocab),
            )
        {
            return Ok(ConditionAst::And(Box::new(lc), Box::new(rc)));
        }
    }
    if let Some((l, r)) = low.split_once(" or if ").or_else(|| low.split_once(" or ")) {
        if let (Ok(lc), Ok(rc)) = (parse_condition(l, vocab), parse_condition(r, vocab)) {
            return Ok(ConditionAst::Or(Box::new(lc), Box::new(rc)));
        }
        if l.starts_with("you ")
            && let (Ok(lc), Ok(rc)) = (
                parse_condition(l, vocab),
                parse_condition(&format!("you {r}"), vocab),
            )
        {
            return Ok(ConditionAst::Or(Box::new(lc), Box::new(rc)));
        }
    }

    // Tabletop Honor-System physical/social condition fallback
    if is_physical_tabletop_clause(&low) {
        return Ok(ConditionAst::ManualTabletopCheck {
            prompt: raw.to_string(),
        });
    }

    Err(format!("unparsed_condition:{raw}"))
}

/// Strictly parse a mana specification (`{G}`, `{W} or {U}`, `one mana of any color`, etc.).
pub fn parse_mana_spec(phrase: &str, vocab: &Vocabulary) -> Result<ManaSpecAst, String> {
    let raw = trim_punct(phrase);
    if is_mana_sequence(raw) {
        return Ok(ManaSpecAst::Fixed(raw.to_string()));
    }
    // "<val> {M1} or <val> {M2}" (Brigid: "X {G} or X {W}")
    if let Some((l, r)) = raw.split_once(" or ")
        && parse_mana_spec(l.trim(), vocab).is_ok()
        && parse_mana_spec(r.trim(), vocab).is_ok()
    {
        return Ok(ManaSpecAst::Choice(vec![
            l.trim().to_string(),
            r.trim().to_string(),
        ]));
    }
    // "<value_expr> {M}" e.g. "six {R}", "three {G}", "twice that much {G}", "that much {R}", "X {B}"
    if let Some((amt_raw, sym)) = raw.rsplit_once(' ')
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_value_expr(amt_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }
    if let Some(after) = raw.to_ascii_lowercase().strip_prefix("an amount of ")
        && let Some((sym_part, val_raw)) = after.split_once(" equal to ")
        && let Ok(amt) = parse_value_expr(val_raw.trim(), vocab)
    {
        let sym_orig = raw["an amount of ".len()..]
            ["an amount of ".len() - "an amount of ".len()..sym_part.len()]
            .trim();
        if is_mana_sequence(sym_orig) {
            return Ok(ManaSpecAst::ScaledSymbol {
                symbol: sym_orig.to_string(),
                amount: Box::new(amt),
            });
        }
    }

    // Choice: "{W} or {U}" / "{R}, {G}, or {W}"
    let normalized_choice = raw.replace(", or ", " or ").replace(", ", " or ");
    let parts: Vec<&str> = normalized_choice
        .split(" or ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 && parts.iter().all(|p| is_mana_sequence(p)) {
        return Ok(ManaSpecAst::Choice(
            parts.into_iter().map(ToString::to_string).collect(),
        ));
    }

    let low = raw.to_ascii_lowercase();
    if let Some((sym, _)) = raw.split_once(" or one mana of the chosen color")
        && is_mana_sequence(sym.trim())
    {
        return Ok(ManaSpecAst::Choice(vec![
            sym.trim().to_string(),
            "chosen_color".to_string(),
        ]));
    }
    if low == "one mana of the chosen color"
        || low == "one mana of that color"
        || low == "one mana of any of the exiled cards' colors"
    {
        return Ok(ManaSpecAst::ChosenColor);
    }
    // "<val> mana of that type/color" / "<val> mana of the chosen color" (Incubation Druid, Radiant Lotus)
    if let Some(cnt_raw) = low
        .strip_suffix(" mana of that type")
        .or_else(|| low.strip_suffix(" mana of that color"))
        .or_else(|| low.strip_suffix(" mana of the chosen color"))
        && let Ok(amount) = parse_value_expr(cnt_raw, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amount),
            single_color: true,
        });
    }
    // "one mana of any color among <flt>" (Mox Amber, Plaza of Heroes, The Grey Havens)
    if let Some(flt_raw) = low.strip_prefix("one mana of any color among ")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(ValueExprAst::Const(1)),
            single_color: true,
        });
    }
    // "one mana of any color that a <flt> could produce" (Gond Gate, Exotic Orchard variants)
    if let Some(rest) = low.strip_prefix("one mana of any color that ")
        && let Some(flt_raw) = rest.strip_suffix(" could produce")
        && parse_object_filter(flt_raw, vocab).is_ok()
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::You,
        });
    }
    if let Some((sym, fe_raw)) = raw.split_once(" for each ")
        && is_mana_sequence(sym.trim())
        && fe_raw
            .to_ascii_lowercase()
            .contains("counter removed this way")
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(ValueExprAst::PriorResult),
        });
    }
    if low == "one mana of any color in your commander's color identity" {
        return Ok(ManaSpecAst::CommanderColorIdentity);
    }
    if low == "one mana of any type that a land you control could produce"
        || low == "one mana of any color that a land you control could produce"
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::You,
        });
    }
    if low == "one mana of any type that a land an opponent controls could produce"
        || low == "one mana of any color that a land an opponent controls could produce"
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::Opponent,
        });
    }
    if let Some((amt_raw, syms_raw)) = raw.split_once(" mana in any combination of ")
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        let cleaned = syms_raw
            .replace(" and/or ", " ")
            .replace(" or ", " ")
            .replace(", ", " ");
        let syms: Vec<String> = cleaned
            .split_whitespace()
            .filter(|t| is_mana_sequence(t))
            .map(ToString::to_string)
            .collect();
        if !syms.is_empty() {
            return Ok(ManaSpecAst::CombinationOf {
                amount: Box::new(amt),
                symbols: syms,
            });
        }
    }
    if let Some(amt_str) = low.strip_suffix(" mana of different colors")
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::DifferentColors {
            amount: Box::new(amt),
        });
    }
    // "one mana of any [one] color for each <fe>" (Coalition Relic, etc.)
    if let Some((left, fe_raw)) = low.split_once(" for each ")
        && (left.ends_with(" mana of any one color")
            || left.ends_with(" mana of any color")
            || left.ends_with(" mana in any combination of colors"))
        && let Ok(scale) = parse_for_each_value_expr(fe_raw, vocab)
    {
        let single_color = left.ends_with(" mana of any one color");
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(scale),
            single_color,
        });
    }
    if let Some(amt_str) = low
        .strip_suffix(" mana of any one color")
        .or_else(|| low.strip_suffix(" mana of any color"))
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amt),
            single_color: low.contains("any one color") || low.starts_with("one "),
        });
    }
    if let Some(amt_str) = low.strip_suffix(" mana in any combination of colors")
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amt),
            single_color: false,
        });
    }
    if let Some(after) = raw.strip_prefix("an amount of ")
        && let Some((sym, val_raw)) = after.split_once(" equal to ")
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }
    if low == "an amount of mana of that color equal to your devotion to that color" {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: "chosen_color".to_string(),
            amount: Box::new(ValueExprAst::Devotion {
                colors: "chosen_color".to_string(),
            }),
        });
    }
    // "that much {M}" (Rasputin, the Oneiromancer)
    if let Some(sym) = raw.strip_prefix("that much ")
        && is_mana_sequence(sym.trim())
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(ValueExprAst::PriorResult),
        });
    }
    // "{B} or {G} for each <fe>" (Culling Ritual, etc.)
    if let Some((left_mana, fe_raw)) = raw.split_once(" for each ")
        && let Ok(amt) = parse_for_each_value_expr(fe_raw, vocab)
    {
        let normalized_choice = left_mana.replace(", or ", " or ").replace(", ", " or ");
        let syms: Vec<String> = normalized_choice
            .split(" or ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(ToString::to_string)
            .collect();
        if syms.len() >= 2 && syms.iter().all(|p| is_mana_sequence(p)) {
            return Ok(ManaSpecAst::CombinationOf {
                amount: Box::new(amt),
                symbols: syms,
            });
        }
    }
    if let Some((sym, fe_raw)) = raw.split_once(" for each ")
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_for_each_value_expr(fe_raw, vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }

    Err(format!("unparsed_mana:{raw}"))
}

const PREDEFINED_TOKENS: &[&str] = &[
    "treasure",
    "food",
    "clue",
    "blood",
    "map",
    "powerstone",
    "junk",
    "gold",
    "shard",
    "incubator",
    "lander",
    "mutagen",
    "walker",
    "role",
    "heartwood",
    "vibranium",
];

/// Strictly parse the object of `create ...` into a typed `TokenSpecAst`.
pub fn parse_token_creation(phrase: &str, vocab: &Vocabulary) -> Result<TokenSpecAst, String> {
    let raw = trim_punct(phrase);
    let mut low = raw.to_ascii_lowercase();
    let mut tapped = false;
    let mut attacking = false;

    if let Some((before, _)) = low
        .split_once(" that's tapped and attacking")
        .or_else(|| low.split_once(" that are tapped and attacking"))
    {
        tapped = true;
        attacking = true;
        low = before.to_string();
    } else if low.contains(" tapped and attacking") {
        tapped = true;
        attacking = true;
        // Strip trailing " tapped and attacking [that player|defending player|you|an opponent]",
        // or leading adjective " tapped and attacking" inside "a tapped and attacking 1/1 ...".
        for suffix in [
            " tapped and attacking that player",
            " tapped and attacking defending player",
            " tapped and attacking you",
            " tapped and attacking an opponent",
            " tapped and attacking",
        ] {
            if let Some(stripped) = low.strip_suffix(suffix) {
                low = stripped.to_string();
                break;
            }
        }
        if low.contains(" tapped and attacking") {
            low = low.replace(" tapped and attacking", "");
        }
    }
    if let Some((before, _)) = low
        .split_once(" that's attacking")
        .or_else(|| low.split_once(" that are attacking"))
        .or_else(|| low.split_once(" attacking that opponent"))
        .or_else(|| low.split_once(" attacking that player"))
        .or_else(|| low.split_once(" attacking defending player"))
        .or_else(|| low.split_once(" that's blocking"))
        .or_else(|| low.split_once(" that are blocking"))
    {
        attacking = true;
        low = before.to_string();
    }
    if low.contains(" that's tapped") {
        tapped = true;
        low = low.replace(" that's tapped", "");
    } else if low.contains(" tapped") {
        tapped = true;
        low = low.replace(" tapped", "");
    }
    let low = low.trim();

    // "N/X of those tokens" / "another one of those tokens" (Wurmquake, etc.)
    if low == "another one of those tokens" {
        return Ok(TokenSpecAst::Predefined {
            name: "those_tokens".to_string(),
            count: Box::new(ValueExprAst::Const(1)),
            tapped,
        });
    }
    if let Some(cnt_part) = low.strip_suffix(" of those tokens")
        && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
    {
        return Ok(TokenSpecAst::Predefined {
            name: "those_tokens".to_string(),
            count: Box::new(cnt),
            tapped,
        });
    }

    // "half X <desc> tokens, rounded up/down" (The Goose Mother, etc.)
    if let Some(after_half) = low.strip_prefix("half ")
        && let Some((tok_side, round_raw)) = after_half.split_once(", rounded ")
        && let Some((expr_tok, rest_tok)) = tok_side.split_once(' ')
        && let Ok(inner_val) = parse_value_expr(expr_tok, vocab)
        && let Ok(mut tok) = parse_token_creation(&format!("a {}", rest_tok.trim()), vocab)
    {
        let div_val = ValueExprAst::DivConst {
            expr: Box::new(inner_val),
            denom: 2,
            rounding: if round_raw.trim() == "up" {
                crate::ast::Rounding::Up
            } else {
                crate::ast::Rounding::Down
            },
        };
        match &mut tok {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => **count = div_val,
            TokenSpecAst::Role { .. } => {}
        }
        return Ok(tok);
    }

    // Copy token: "a token that's a copy of <subject> [tapped and attacking ...][, except <exception>]"
    if let Some((cnt_part, rest)) = low
        .split_once(" token that's a copy of ")
        .or_else(|| low.split_once(" tokens that are copies of "))
    {
        let (src_raw_full, exception) = match rest.split_once(", except ") {
            Some((s, exc)) => (s, Some(exc.trim().to_string())),
            None => (rest, None),
        };
        let src_raw = src_raw_full
            .split_once(" tapped and attacking")
            .or_else(|| src_raw_full.split_once(" and that's blocking"))
            .map_or(src_raw_full, |(s, _)| s)
            .trim();
        if let (Ok(cnt), Ok(subj)) = (
            parse_value_expr(cnt_part, vocab),
            parse_subject_or_target(src_raw, vocab),
        ) {
            return Ok(TokenSpecAst::Copy {
                count: Box::new(cnt),
                source: Box::new(subj),
                tapped: tapped || src_raw_full.contains("tapped"),
                attacking: attacking || src_raw_full.contains("attacking"),
                exception,
            });
        }
    }
    // Dynamic P/T creature token: "<cnt> <colors> <subtypes> creature token with [base] power and toughness each equal to <val>"
    // or "with power equal to <v1> and toughness equal to <v2>" (Skullspore Nexus, Soul Separator)
    if let Some((left_desc, _rhs)) = low
        .split_once(" creature token with base power and toughness each equal to ")
        .or_else(|| low.split_once(" creature token with power equal to "))
        && let Some((cnt_tok, rest_desc)) = left_desc.split_once(' ')
        && let Ok(cnt) = parse_value_expr(cnt_tok, vocab)
    {
        return Ok(TokenSpecAst::Creature {
            count: Box::new(cnt),
            power: "X".to_string(),
            toughness: "X".to_string(),
            colors: ColorMask::EMPTY,
            supertypes: SupertypeMask::EMPTY,
            card_types: CardTypeMask::CREATURE,
            subtypes: vec![rest_desc.trim().to_string()],
            keywords: Vec::new(),
            tapped,
            attacking,
            named: None,
        });
    }

    // Peel optional trailing "that's all colors" or "that's <colors>" (Godsire)
    let (low, all_colors, extra_colors) =
        if let Some(stripped) = low.strip_suffix(" that's all colors") {
            (stripped.trim(), true, ColorMask::EMPTY)
        } else if let Some((before, color_list)) = low.split_once(" that's ") {
            let mut parsed_c = ColorMask::EMPTY;
            let mut all_valid = true;
            for w in color_list.split_whitespace() {
                let tok = w.trim_matches(',');
                if tok == "and" {
                    continue;
                }
                if let Some(c) = lookup_color(tok) {
                    parsed_c = parsed_c.union(c);
                } else {
                    all_valid = false;
                    break;
                }
            }
            if all_valid && !parsed_c.is_empty() {
                (before.trim(), false, parsed_c)
            } else {
                (low, false, ColorMask::EMPTY)
            }
        } else {
            (low, false, ColorMask::EMPTY)
        };

    // Peel optional trailing "for each ..." ONLY when not inside a quoted token ability
    let (body, for_each_expr) = if !low.contains('"')
        && let Some((b, fe)) = low.split_once(" for each ")
    {
        let fe_val = parse_for_each_value_expr(fe, vocab)?;
        (b.trim(), Some(fe_val))
    } else {
        (low, None)
    };

    // Wilds of Eldraine Role tokens: "a <RoleName> Role token attached to <target>"
    if let Some((role_part, att_raw)) = low.split_once(" role token attached to ")
        && let Ok(attached_to) = parse_subject_or_target(att_raw, vocab)
    {
        let role_name = role_part
            .strip_prefix("a ")
            .or_else(|| role_part.strip_prefix("an "))
            .unwrap_or(role_part)
            .trim()
            .to_string();
        if !role_name.is_empty() {
            return Ok(TokenSpecAst::Role {
                role_name,
                attached_to: Box::new(attached_to),
            });
        }
    }

    // Predefined token: "a Treasure token", "two Food tokens"
    for &ptok in PREDEFINED_TOKENS {
        if let Some(cnt_str) = body
            .strip_suffix(&format!(" {ptok} token"))
            .or_else(|| body.strip_suffix(&format!(" {ptok} tokens")))
        {
            let base_cnt = parse_value_expr(cnt_str, vocab)?;
            let count = for_each_expr.unwrap_or(base_cnt);
            let mut chars = ptok.chars();
            let cap = match chars.next() {
                Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            };
            return Ok(TokenSpecAst::Predefined {
                name: cap,
                count: Box::new(count),
                tapped,
            });
        }
    }

    // "a number of <desc> tokens equal to <ValueExpr>" (Goblin Gathering, etc.)
    if let Some(after_num) = body.strip_prefix("a number of ")
        && let Some((tok_side, val_raw)) = after_num.split_once(" equal to ")
        && let Ok(eq_val) = parse_value_expr(val_raw, vocab)
        && let Ok(mut inner) = parse_token_creation(&format!("a {}", tok_side.trim()), vocab)
    {
        match &mut inner {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => **count = eq_val,
            TokenSpecAst::Role { .. } => {}
        }
        return Ok(inner);
    }

    // Prefix-named token: "<Name>, a [legendary] [P/T] <desc> token [with <kws/abilities>]"
    // (e.g. "Voja, a legendary 2/2 green and white Wolf creature token",
    // "Ashaya, the Awoken World, a legendary 4/4 green Elemental creature token",
    // "~'s Notebook, a legendary colorless Book artifact token with \"...\"")
    let (body, prefix_named) = {
        let split_before_tok = body.find(" token").unwrap_or(body.len());
        let search_slice = &body[..split_before_tok];
        if let Some(idx) = search_slice
            .rfind(", a ")
            .or_else(|| search_slice.rfind(", an "))
        {
            let name_part = body[..idx].trim();
            let rest_part = body[idx + 2..].trim();
            if !name_part.is_empty() && !name_part.contains('"') {
                (rest_part, Some(name_part.to_string()))
            } else {
                (body, None)
            }
        } else {
            (body, None)
        }
    };

    // "<count> P/T creature token(s) of the chosen color and type" (Volrath's Laboratory, Riptide Replicator)
    if let Some((before, _)) = body.split_once(" of the chosen color and type")
        && let Ok(inner) = parse_token_creation(before.trim(), vocab)
    {
        return Ok(inner);
    }

    // Standard creature or noncreature token: "<count> [legendary] [<P>/<T>] <desc> token(s) [named <Name>] [with <kws>] [named <Name>]"
    let (core, mut named, mut kw_tail) = if let Some((before, after)) = body
        .split_once(" token named ")
        .or_else(|| body.split_once(" tokens named "))
    {
        match after.split_once(" with ") {
            Some((n, kw)) => (before, Some(n.trim().to_string()), Some(kw)),
            None => (before, Some(after.trim().to_string()), None),
        }
    } else if let Some((before, after)) = body.split_once(" token with ") {
        (before, None, Some(after))
    } else if let Some((before, after)) = body.split_once(" tokens with ") {
        (before, None, Some(after))
    } else if let Some(before) = body
        .strip_suffix(" tokens")
        .or_else(|| body.strip_suffix(" token"))
    {
        (before, None, None)
    } else {
        return Err(format!("unparsed_token:{raw}"));
    };
    if named.is_none() {
        named = prefix_named;
    }
    if named.is_none()
        && let Some(kt) = kw_tail
        && !kt.contains('"')
        && let Some((kw_only, name_part)) = kt.split_once(" named ")
    {
        kw_tail = Some(kw_only.trim());
        named = Some(name_part.trim().to_string());
    }

    let words: Vec<&str> = core.split_whitespace().collect();
    if words.len() < 2 {
        return Err(format!("unparsed_token_short:{raw}"));
    }

    // Handle "that many" / "twice x" (2 words) or single-word count
    let (count_expr, mut cursor) = if words.len() >= 3 && words[0] == "that" && words[1] == "many" {
        (ValueExprAst::PriorResult, 2)
    } else if words.len() >= 3 && words[0] == "twice" && words[1] == "x" {
        (
            ValueExprAst::Mul(
                Box::new(ValueExprAst::Const(2)),
                Box::new(ValueExprAst::ChosenX),
            ),
            2,
        )
    } else {
        (parse_value_expr(words[0], vocab)?, 1)
    };

    let mut colors = ColorMask::EMPTY;
    let mut supertypes = SupertypeMask::EMPTY;
    let mut card_types = CardTypeMask::EMPTY;
    let mut subtypes = Vec::new();

    // Peel optional leading supertypes (e.g. "a legendary 2/2 ..." or "a legendary colorless Book artifact")
    while cursor < words.len() {
        if let Some(st) = lookup_supertype(words[cursor].trim_matches(',')) {
            supertypes = supertypes.union(st);
            cursor += 1;
        } else {
            break;
        }
    }

    let (power, toughness) = if cursor < words.len()
        && let Some((p, t)) = parse_base_pt(words[cursor])
    {
        cursor += 1;
        (p, t)
    } else {
        (String::new(), String::new())
    };

    for &w in &words[cursor..] {
        let tok = w.trim_matches(',');
        if tok == "and" || tok == "colorless" {
            continue;
        }
        if let Some(c) = lookup_color(tok) {
            colors = colors.union(c);
        } else if let Some(st) = lookup_supertype(tok) {
            supertypes = supertypes.union(st);
        } else if let Some(ct) = lookup_card_type(tok) {
            card_types = card_types.union(ct);
        } else if let Some(sub) = vocab.lookup_subtype(tok) {
            subtypes.push(sub.to_string());
        } else {
            return Err(format!("unparsed_token_desc_tok:{tok}"));
        }
    }
    if power.is_empty() && card_types.is_empty() {
        return Err(format!("unparsed_token_pt:{raw}"));
    }
    if all_colors {
        colors = ColorMask::WHITE
            .union(ColorMask::BLUE)
            .union(ColorMask::BLACK)
            .union(ColorMask::RED)
            .union(ColorMask::GREEN);
    } else if !extra_colors.is_empty() {
        colors = colors.union(extra_colors);
    }
    if card_types.is_empty() {
        card_types = CardTypeMask::CREATURE;
    }

    let mut keywords = Vec::new();
    if let Some(kt) = kw_tail {
        if kt.contains('"') {
            keywords.push(kt.to_string());
        } else if let Some(kws) = parse_keyword_list(kt, vocab) {
            keywords = kws;
        } else {
            return Err(format!("unparsed_token_kw:{kt}"));
        }
    }

    Ok(TokenSpecAst::Creature {
        count: Box::new(for_each_expr.unwrap_or(count_expr)),
        power,
        toughness,
        colors,
        supertypes,
        card_types,
        subtypes,
        keywords,
        tapped,
        attacking,
        named,
    })
}

/// Split a keyword line on `,` or `;` outside quotes.
#[must_use]
pub fn split_keyword_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for seg in split_top_level(line, ";") {
        for part in split_top_level(seg, ",") {
            if !part.is_empty() {
                out.push(part);
            }
        }
    }
    out
}
