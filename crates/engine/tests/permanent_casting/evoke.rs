use super::*;
use spellwake_engine::{
    characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec},
    mana::CastingPayment,
    program::TriggeredAbilityId,
};

#[test]
fn evoke_sacrifice_follows_the_permanents_current_controller_and_original_incarnation() {
    let mut cards = seeds();
    let mut evoker = creature(
        100,
        A,
        ObjectDestination::Hand,
        Some(ManaCost::generic(8.into())),
        4,
    );
    evoker.program = evoker
        .program
        .with_evoke(ManaCost::free(), TriggeredAbilityId(1));
    cards.push(evoker);
    cards.push(spell(
        101,
        B,
        Instruction::CreateContinuousEffect {
            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                ControlRecipient::ResolvingController,
            )]),
            duration: ResolvedDuration::UntilEndOfTurn,
        },
        ManaCost::free(),
        BodyTarget::SingleCreature,
    ));
    let game = setup(Some(Step::PrecombatMain), cards);
    let cast = accept(
        &game,
        A,
        GameCommand::CastWithResources {
            spell: object(&game, 100),
            target: None,
            payment: CastingPayment {
                use_alternative: true,
                ..Default::default()
            },
        },
    );
    let entered = resolve(cast.state()).into_state();
    let permanent = object(&entered, 100);
    assert_eq!(entered.execution().window().entries().len(), 1);
    let paid = pass(&entered);
    let steal = targeted(paid.state(), B, 101, TargetRef::Object(permanent));
    let owned = resolve(steal.state());
    assert_eq!(
        owned
            .state()
            .execution()
            .current_characteristics(permanent)
            .unwrap()
            .unwrap()
            .controller,
        Some(B)
    );
    let sac = resolve(owned.state());
    assert_eq!(physical(sac.state(), 100).zone(), ObjectZone::Graveyard(A));
    let bounce = targeted(&entered, A, 13, TargetRef::Object(permanent));
    let returned = resolve(bounce.state());
    let expired = resolve(returned.state());
    assert_eq!(physical(expired.state(), 100).zone(), ObjectZone::Hand(A));
}
