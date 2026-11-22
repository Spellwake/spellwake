//! Bounded actual observer. It never calls the historical pending-work projection.
use super::*;
use spellwake_engine::{
    effects::PendingWorkKind,
    occurrence::{RulesOccurrence, RulesRecord},
};

fn frame(state: &GameState) -> Result<Value, PreparedError> {
    let Some(decision) = state.pending_decision() else {
        return Ok(Value::Null);
    };
    let value = match decision.frame() {
        RuleFrame::LibraryTop(f) => {
            json!({"tag":"LibraryTop","kind":match f.kind() { LibraryChoiceKind::Search => return Err("search outside this prepared profile".into()), LibraryChoiceKind::Scry => "Scry", LibraryChoiceKind::Surveil => "Surveil" },
            "revision":f.revision().to_string(),"entry":projection::opaque(f.entry()),"instruction":f.instruction(),"chooser":f.chooser().0,
            "domain_top_first":f.domain_top_first().iter().map(projection::opaque).collect::<Vec<_>>()})
        }
        RuleFrame::ResolutionGraveyardOrder(_) => projection::frame(decision.frame(), state)?,
        _ => return Err("prepared diagnostic unsupported native frame".into()),
    };
    Ok(
        json!({"id":decision.id().0.to_string(),"basis":projection::basis(decision.basis()),"chooser":decision.chooser().0,"frame":value}),
    )
}
pub(super) fn state(
    state: &GameState,
    aliases: &BTreeMap<CardId, super::super::Alias>,
) -> Result<Value, PreparedError> {
    require(
        state.opening().is_none()
            && state.entropy().is_none()
            && state.pending_turn_work().is_none(),
        "prepared diagnostic unsupported root lifecycle",
    )?;
    let effect = state.execution();
    require(
        !effect.has_check_replacement_work(),
        "prepared diagnostic unsupported replacement-bearing Checks",
    )?;
    let window = effect.window();
    let cards = window.cards();
    let pending = match effect.pending_work_kind() {
        None => Value::Null,
        Some(PendingWorkKind::LibraryTop) => {
            json!({"tag":"LibraryTop","cursor":effect.resolution_state().map(projection::resolution_state).ok_or("prepared library cursor absent")?})
        }
        _ => return Err("prepared diagnostic unsupported pending native work".into()),
    };
    let objects = cards.iter().map(|object| {
        let card = object.card_id().ok_or("prepared diagnostic nonphysical object")?;
        let alias = aliases.get(&card).ok_or("prepared diagnostic physical binding missing")?;
        Ok(json!({"alias":alias.alias,"card_id":card.0,"definition":alias.definition_ref,"object":projection::object(object)}))
    }).collect::<Result<Vec<_>, PreparedError>>()?;
    let turn = state
        .turn()
        .ok_or("prepared diagnostic scheduled turn missing")?;
    Ok(
        json!({"basis":projection::basis(state.basis()),"turn":{"position":crate::damage_trace::turn_position_view(turn.position()),"lands_played":turn.lands_played()},
        "objects":objects,"physical_programs":effect.programs().map(|(card,p)|json!({"card_id":card.0,"program":program::view(p)})).collect::<Vec<_>>(),
        "stack_window":projection::stack_window(window),"stack_payloads":projection::stack_payloads(effect),"resolving_payload":projection::resolving_payload(effect),
        "resolution_cursor":effect.resolution_state().map(projection::resolution_state),"instruction_cursor":effect.instruction_index(),
        "pending_work":pending,"decision":frame(state)?,
        "resources":([A,B].iter().map(|player| json!({"player":player.0,"life":effect.life_total(*player),"mana":ManaType::ALL.iter().map(|kind| (match kind { ManaType::White=>"White",ManaType::Blue=>"Blue",ManaType::Black=>"Black",ManaType::Red=>"Red",ManaType::Green=>"Green",ManaType::Colourless=>"Colourless" },effect.mana(*player,*kind))).collect::<BTreeMap<_,_>>(),
            "library_top_first":cards.library(*player).iter().map(projection::opaque).collect::<Vec<_>>(),"graveyard_top_first":cards.graveyard(*player).iter().map(projection::opaque).collect::<Vec<_>>(),
            "hand_membership_unordered":cards.iter().filter(|c| c.zone()==spellwake_engine::object_identity::ObjectZone::Hand(*player)).map(|c|projection::opaque(c.id())).collect::<Vec<_>>()})).collect::<Vec<_>>()),
        "characteristic_timestamps":effect.characteristic_timestamps().map(|(object,timestamp)|json!({"object":projection::opaque(object),"sequence":timestamp.sequence().to_string()})).collect::<Vec<_>>(),
        "outcome":state.outcome().map(|o| match o {spellwake_engine::draw::GameOutcome::Winner(p)=>json!({"tag":"Winner","player":p.0}),spellwake_engine::draw::GameOutcome::Draw=>json!({"tag":"Draw"})})}),
    )
}
pub(super) fn candidate(
    candidate: &NativeCandidate,
    before: &GameState,
    aliases: &BTreeMap<CardId, super::super::Alias>,
) -> Result<Value, PreparedError> {
    let actions = candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::Event(event) => Some(event),
            _ => None,
        })
        .flat_map(|e| &e.occurrences)
        .filter_map(|o| match &o.value {
            RulesOccurrence::LibraryAction(a) => Some(a),
            _ => None,
        })
        .map(projection::library_action)
        .collect::<Vec<_>>();
    Ok(
        json!({"tag":"Ok","base":projection::candidate_base(candidate.base()),"state":state(candidate.state(),aliases)?,
        "rules_journal":projection::journal(candidate.rules_journal()),"library_actions":actions,"draw_journal":projection::draws(candidate.draw_journal()),
        "checks":crate::check_trace::check_journal_view(candidate.check_journal(),before.execution().window().cards(),|card|card.0.to_string())?,
        "mana_payment":candidate.mana_payment().map(projection::payment),"mana_production":candidate.mana_production().map(projection::production),
        "land_play":candidate.land_play().map(|l|json!({"player":l.player.0,"before":projection::object(&l.before),"after":projection::object(&l.after)}))}),
    )
}
