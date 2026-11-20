package forge.ai;

import com.google.common.collect.ListMultimap;
import com.google.common.collect.Multimap;
import forge.card.ColorSet;
import forge.card.ICardFace;
import forge.card.mana.ManaCost;
import forge.card.mana.ManaCostShard;
import forge.deck.Deck;
import forge.deck.DeckSection;
import forge.game.*;
import forge.game.player.Player;
import forge.game.player.DelayedReveal;
import forge.game.player.PlayerActionConfirmMode;
import forge.game.player.PlayerView;
import forge.game.player.PlayerController;
import forge.game.ability.effects.RollDiceEffect;
import forge.game.card.*;
import forge.game.card.sticker.Sticker;
import forge.game.combat.Combat;
import forge.game.cost.*;
import forge.game.keyword.KeywordInterface;
import forge.game.mana.Mana;
import forge.game.mana.ManaConversionMatrix;
import forge.game.mana.ManaCostBeingPaid;
import forge.game.replacement.ReplacementEffect;
import forge.game.spellability.*;
import forge.game.staticability.StaticAbility;
import forge.game.trigger.WrappedAbility;
import forge.game.zone.PlayerZone;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import forge.util.ITriggerEvent;
import forge.util.collect.FCollectionView;
import org.apache.commons.lang3.tuple.ImmutablePair;
import org.apache.commons.lang3.tuple.Pair;

import java.util.Collection;
import java.util.List;
import java.util.Map;
import java.util.function.Predicate;


import com.google.gson.GsonBuilder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.io.IOException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import org.testng.Assert;
import org.testng.annotations.Test;
import forge.model.FModel;
import forge.item.IPaperCard;

import forge.game.event.*;

import forge.card.mana.ManaAtom;
import forge.game.mana.ManaPool;

import forge.game.player.PlaySpellAbility;
import com.google.common.eventbus.Subscribe;

/** Original strict probes for actual paid targeted nonmana artifact activations.
 * The three selected printed abilities are isolated; this is not whole-card support.
 * Float/setup and one disclosed source intervention are outside the gameplay loop.
 */
public class ActivatedAbilityReferenceTest extends AITest {
    private static final String COMMIT = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256 = "d9dbd81b65183e29120f3423ffb07a773a32af4f01c99f258465d48281e46e43";
    private static final String COLORS = "WUBRGC";
    @Test public void rodOfRuin() throws IOException { run("rod-of-ruin"); }
    @Test public void pyriteSpellbomb() throws IOException { run("pyrite-spellbomb"); }
    @Test public void aetherSpellbomb() throws IOException { run("aether-spellbomb"); }

    private void run(String scenario) throws IOException {
        final List<Map<String,Object>> probes = new ArrayList<>(); boolean completed=false;
        try {
            if (scenario.equals("rod-of-ruin")) {
                probes.add(probe("rod-normal","Rod of Ruin",false,false,null,false));
                probes.add(probe("rod-stolen","Rod of Ruin",true,false,null,false));
                probes.add(probe("rod-tapped-rejected","Rod of Ruin",false,true,null,false));
                probes.add(probe("rod-source-departed","Rod of Ruin",false,false,"source",false));
                probes.add(probe("rod-target-departed","Rod of Ruin",false,false,"target",false));
            } else if (scenario.equals("pyrite-spellbomb")) {
                probes.add(probe("pyrite-normal","Pyrite Spellbomb",false,false,null,false));
                probes.add(probe("pyrite-tapped-stolen","Pyrite Spellbomb",true,true,null,false));
                probes.add(probe("pyrite-target-departed","Pyrite Spellbomb",false,false,"target",false));
            } else if (scenario.equals("aether-spellbomb")) {
                probes.add(probe("aether-normal","Aether Spellbomb",false,false,null,false));
                probes.add(probe("aether-stolen-target-owner","Aether Spellbomb",true,false,null,true));
                probes.add(probe("aether-target-departed","Aether Spellbomb",false,false,"target",false));
            } else throw new AssertionError("Unknown scenario");
            completed=true;
        } finally {
            final Map<String,Object> trace=map("schema_version",1,"scenario",scenario,"reference","forge","commit",COMMIT,
                "expected_sha256",EXPECTED_SHA256,"completed",completed,"strict_choices",true,"ai_policy_used",false,
                "activation_path","PlaySpellAbility/CostPayment/CostTap-or-CostSacrifice/CostPartMana/MagicStack",
                "actual_nonmana_activations",true,"actual_mana_payment",true,"actual_stack_resolution",true,
                "direct_effect_resolution",false,"produced_pool_injection",false,"priority_loop_executed",false,
                "reference_whole_command_rollback_compared",false,"reference_pre_cost_snapshot_protocol_compared",false,
                "qualification","isolated-creature-target-nonmana-artifact-activated-abilities","probes",probes);
            final String directory=System.getProperty("spellwake.evidence.dir"); Assert.assertNotNull(directory);
            Files.write(Paths.get(directory,scenario+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),
                java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario="+scenario+" completed="+completed+" probes="+probes.size());
        }
    }
    private Map<String,Object> probe(String id,String name,boolean stolen,boolean tapped,String departure,boolean stolenTarget) {
        final Game game=initAndCreateGame(); final Script script=new Script(game);
        for(Player player:game.getPlayers()) player.addController(game.getNextTimestamp(),player,new StrictController(game,player,script),false);
        script.seed(name,stolen,tapped,departure,stolenTarget); script.observe("initial",null);
        final boolean accepted=script.activate(); script.observe(accepted?"activated":"rejected",accepted);
        if(accepted) {
            if("source".equals(departure)) {
                // Reference-only trusted intervention: native cannot bounce this artifact.
                script.game.getAction().moveToHand(script.current(script.source),null);
                script.observe("source-departed",null);
            }
            if("target".equals(departure)) {
                script.castResponse(); script.observe("response-cast",true);
                script.resolve(); script.observe("response-resolved",null);
            }
            script.resolve(); script.observe("resolved",null);
        }
        return script.finish(id,name,departure,stolenTarget,tapped);
    }
    private static byte color(char c) { return ManaAtom.fromName(String.valueOf(c)); }
    private static Map<String,Object> pool(ManaPool pool) {
        final Map<String,Object> values=new LinkedHashMap<>();
        for(char c:COLORS.toCharArray()) values.put(String.valueOf(c),pool.getAmountOfColor(color(c))); return values;
    }
    private static Map<String,Object> map(Object... pairs) {
        final Map<String,Object> values=new LinkedHashMap<>();
        for(int i=0;i<pairs.length;i+=2) values.put((String)pairs[i],pairs[i+1]); return values;
    }
    public static final class Script {
        final Game game; final Player actor; final Player other;
        final List<Map<String,Object>> snapshots=new ArrayList<>(); final List<Map<String,Object>> callbacks=new ArrayList<>();
        final List<Map<String,Object>> events=new ArrayList<>(); final List<Map<String,Object>> actions=new ArrayList<>();
        final Map<String,Object> metadata=new LinkedHashMap<>(); final List<Map<String,Object>> abilityDomain=new ArrayList<>();
        Card source; Card target; Card response; SpellAbility selected; Player payer; String spend;
        int abilitySelections; int targetSelections; int tapDecisions; int sacrificeDecisions; int manaDecisions; int manaPayments;
        Throwable eventFailure;
        Script(Game game) { this.game=game; actor=game.getPlayers().get(1); other=game.getPlayers().get(0); }
        String alias(Player player) { if(player==actor) return "A"; if(player==other) return "B"; throw new AssertionError("Unknown player"); }
        Card current(Card original) { final Card result=game.getCardState(original); Assert.assertNotNull(result); return result; }
        Card seedCard(String alias,String name,Player owner,Player controller,ZoneType zone) {
            final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name); Assert.assertNotNull(paper);
            final Card card=Card.fromPaperCard(paper,owner); card.setGameTimestamp(game.getNextTimestamp());
            if(controller!=owner) card.setController(controller,game.getNextTimestamp()); controller.getZone(zone).add(card);
            metadata.put(alias,map("name",paper.getName(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),
                "oracle",paper.getRules().getOracleText(),"types",paper.getRules().getType().toString(),"initial_identity",identity(card)));
            return card;
        }
        void seed(String name,boolean stolen,boolean tapped,String departure,boolean stolenTarget) {
            source=seedCard("source",name,stolen?other:actor,actor,ZoneType.Battlefield);
            target=seedCard("target","Hill Giant",stolenTarget?actor:other,other,ZoneType.Battlefield);
            Assert.assertTrue(source.isArtifact()); Assert.assertFalse(source.isCreature()); Assert.assertTrue(target.isCreature());
            if(tapped) source.setTapped(true);
            if("target".equals(departure)) response=seedCard("response","Unsummon",other,other,ZoneType.Hand);
            Assert.assertTrue(actor.getManaPool().isEmpty()); Assert.assertTrue(other.getManaPool().isEmpty());
            for(char c:COLORS.toCharArray()) {
                actor.getManaPool().addMana(new Mana(color(c),source,null,actor));
                actor.getManaPool().addMana(new Mana(color(c),source,null,actor));
                other.getManaPool().addMana(new Mana(color(c),source,null,other));
            }
            game.getPhaseHandler().setPriority(actor); game.subscribeToEvents(this);
        }
        Map<String,Object> physical(Card original) {
            final Card card=current(original); final ZoneType zone=game.getZoneOf(card).getZoneType(); final boolean battlefield=zone==ZoneType.Battlefield;
            final String normalized=zone==ZoneType.Battlefield?"battlefield":zone==ZoneType.Stack?"stack":
                zone==ZoneType.Hand?"hand:"+alias(card.getOwner()):zone==ZoneType.Graveyard?"graveyard:"+alias(card.getOwner()):zone.name();
            return map("owner",alias(card.getOwner()),"controller",battlefield||zone==ZoneType.Stack?alias(card.getController()):null,
                "zone",normalized,"tapped",battlefield?card.isTapped():null);
        }
        Map<String,Object> identity(Card card) {
            return map("card_id",card.getId(),"timestamp",card.getGameTimestamp(),"java_identity",System.identityHashCode(card),
                "owner",alias(card.getOwner()),"controller",alias(card.getController()),"tapped",card.isTapped(),
                "host_zone",card.getZone()==null?null:card.getZone().getZoneType().name());
        }
        List<Map<String,Object>> stack() {
            final List<Map<String,Object>> result=new ArrayList<>();
            for(SpellAbilityStackInstance item:game.getStack()) {
                final SpellAbility ability=item.getSpellAbility(); final List<Map<String,Object>> targets=new ArrayList<>();
                for(Card card:ability.getTargets().getTargetCards()) targets.add(identity(card));
                result.add(map("entry_id",item.getId(),"ability_id",ability.getId(),"kind",ability.isSpell()?"card-spell":"activated-ability",
                    "controller",alias(ability.getActivatingPlayer()),"is_activated",ability.isActivatedAbility(),"is_mana",ability.isManaAbility(),
                    "host",identity(ability.getHostCard()),"api",ability.getApi().name(),"targets",targets,
                    "paying_mana",paidMana(ability)));
            }
            return result;
        }
        List<String> paidMana(SpellAbility ability) {
            final List<String> paid=new ArrayList<>();
            for(Mana mana:ability.getPayingMana()) for(char c:COLORS.toCharArray()) if(mana.getColor()==color(c)) paid.add(String.valueOf(c));
            return paid;
        }
        void observe(String label,Object accepted) {
            final Card liveTarget=current(target); final boolean battlefield=liveTarget.isInZone(ZoneType.Battlefield);
            final Map<String,Object> targetState=physical(target); targetState.put("power",battlefield?liveTarget.getNetPower():null);
            targetState.put("toughness",battlefield?liveTarget.getNetToughness():null); targetState.put("damage",battlefield?liveTarget.getDamage():null);
            final SpellAbility top=game.getStack().isEmpty()?null:game.getStack().peekAbility();
            snapshots.add(map("label",label,"source",physical(source),"target",targetState,
                "pools",map("A",pool(actor.getManaPool()),"B",pool(other.getManaPool())),"stack_size",game.getStack().size(),
                "stack_kind",top==null?null:top.isSpell()?"card-spell":"activated-ability","stack_controller",top==null?null:alias(top.getActivatingPlayer()),
                "source_on_stack",current(source).isInZone(ZoneType.Stack),"accepted",accepted,"response",response==null?null:physical(response),
                "raw",map("source_identity",identity(current(source)),"target_identity",identity(liveTarget),
                    "response_identity",response==null?null:identity(current(response)),"stack",stack(),
                    "priority_player",alias(game.getPhaseHandler().getPriorityPlayer()),"source_same_java_object",current(source)==source)));
            if(eventFailure!=null) throw new AssertionError("Event observation failed",eventFailure);
        }
        boolean activate() {
            final forge.game.ability.ApiType api=source.getName().equals("Aether Spellbomb")?forge.game.ability.ApiType.ChangeZone:forge.game.ability.ApiType.DealDamage;
            for(SpellAbility candidate:source.getNonManaAbilities()) {
                abilityDomain.add(map("api",candidate.getApi().name(),"cost",candidate.getPayCosts().toString(),"activated",candidate.isActivatedAbility()));
                if(candidate.getApi()==api && candidate.isActivatedAbility()) { Assert.assertNull(selected); selected=candidate; }
            }
            Assert.assertNotNull(selected); Assert.assertFalse(selected.isSpell()); Assert.assertFalse(selected.isManaAbility());
            Assert.assertTrue(selected.usesTargeting()); selected.setActivatingPlayer(actor); payer=actor;
            spend=source.getName().equals("Rod of Ruin")?"URC":source.getName().equals("Pyrite Spellbomb")?"R":"U";
            final boolean canPlay=selected.canPlay(); final int before=events.size();
            final boolean accepted=canPlay&&PlaySpellAbility.playSpellAbility(actor.getController(),actor,selected);
            actions.add(map("kind","activation","can_play",canPlay,"accepted",accepted,"selected_api",api.name(),"selected_spend",characters(spend),
                "cost",selected.getPayCosts().toString(),"events_before",before,"events_after",events.size(),"stack",stack()));
            if(accepted) { Assert.assertEquals(game.getStack().size(),1); Assert.assertEquals(abilitySelections,1); Assert.assertEquals(targetSelections,1);
                Assert.assertEquals(manaPayments,1); Assert.assertFalse(current(source).isInZone(ZoneType.Stack)); }
            else { Assert.assertFalse(canPlay); Assert.assertEquals(events.size(),before); Assert.assertEquals(callbacks.size(),0); }
            return accepted;
        }
        void castResponse() {
            payer=other; selected=current(response).getFirstSpellAbility(); spend="U"; selected.setActivatingPlayer(other);
            game.getPhaseHandler().setPriority(other); Assert.assertTrue(selected.isSpell()); Assert.assertTrue(selected.canPlay());
            Assert.assertTrue(PlaySpellAbility.playSpellAbility(other.getController(),other,selected)); Assert.assertEquals(game.getStack().size(),2);
            actions.add(map("kind","paid-response","accepted",true,"selected_spend",characters(spend),"stack",stack()));
        }
        SpellAbility select(Player player,Card host,List<SpellAbility> options) {
            Assert.assertSame(player,payer); Assert.assertSame(host,selected.getHostCard()); Assert.assertEquals(options.size(),1); Assert.assertSame(options.get(0),selected);
            abilitySelections++; callbacks.add(map("kind","ability-selection","chooser",alias(player),"domain_size",options.size(),"ability_id",selected.getId())); return selected;
        }
        boolean target(Player player,SpellAbility ability) {
            Assert.assertSame(player,payer); Assert.assertSame(ability,selected); Assert.assertTrue(ability.canTarget(current(target)));
            Assert.assertTrue(current(target).isCreature()); ability.getTargets().add(current(target)); Assert.assertTrue(ability.isTargetNumberValid());
            targetSelections++; callbacks.add(map("kind","target-selection","chooser",alias(player),"ability_id",ability.getId(),"target",identity(current(target)))); return true;
        }
        List<CostPart> order(Player player,List<CostPart> costs) {
            Assert.assertSame(player,payer); Assert.assertEquals(costs.size(),2); int mana=0; int sourceCost=0;
            final List<String> names=new ArrayList<>();
            for(CostPart cost:costs) { names.add(cost.getClass().getSimpleName()); if(cost instanceof CostPartMana) mana++; else if(cost instanceof CostTap||cost instanceof CostSacrifice) sourceCost++; else throw new AssertionError("unexpected cost"); }
            Assert.assertEquals(mana,1); Assert.assertEquals(sourceCost,1); callbacks.add(map("kind","cost-order","chooser",alias(player),"parts",names)); return costs;
        }
        boolean pay(Player player,ManaCost toPay,CostPartMana cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect) {
            Assert.assertSame(player,payer); Assert.assertSame(ability,selected); Assert.assertFalse(effect); Assert.assertFalse(toPay.isZero()); manaPayments++;
            callbacks.add(map("kind","mana-payment","chooser",alias(player),"ability_id",ability.getId(),"actual_cost",toPay.toString()));
            return PlaySpellAbility.payManaCost(player.getController(),toPay,cost,ability,player,prompt,matrix,effect);
        }
        boolean apply(Player player,ManaCostBeingPaid toPay,SpellAbility ability,boolean effect) {
            Assert.assertSame(player,payer); Assert.assertSame(ability,selected); Assert.assertFalse(effect); final List<Map<String,Object>> steps=new ArrayList<>();
            player.pushPaidForSA(ability); ability.setManaCostBeingPaid(toPay);
            try {
                for(char c:spend.toCharArray()) {
                    final boolean success=player.getManaPool().tryPayCostWithColor(color(c),ability,toPay,ability.getPayingMana());
                    steps.add(map("color",String.valueOf(c),"accepted",success,"pool",pool(player.getManaPool()),"paid",toPay.isPaid())); Assert.assertTrue(success);
                }
                Assert.assertTrue(toPay.isPaid()); callbacks.add(map("kind","mana-application","chooser",alias(player),"ability_id",ability.getId(),"steps",steps,"paid_mana",paidMana(ability)));
                return true;
            } finally { ability.setManaCostBeingPaid(null); player.popPaidForSA(); }
        }
        void resolve() {
            Assert.assertFalse(game.getStack().isEmpty()); final int before=game.getStack().size();
            final SpellAbility top=game.getStack().peekAbility(); final List<Map<String,Object>> beforeStack=stack();
            game.getStack().resolveStack(); game.getAction().checkStateEffects(true); Assert.assertFalse(game.isGameOver()); Assert.assertEquals(game.getStack().size(),before-1);
            actions.add(map("kind","resolve-stack","ability_id",top.getId(),"was_spell",top.isSpell(),"before_stack",beforeStack,"after_stack",stack()));
        }
        @Subscribe public void onEvent(GameEvent event) {
            if(eventFailure!=null) return;
            try {
                if(event instanceof GameEventCardTapped tapped) events.add(map("kind","tap","source_id",tapped.card().getId(),"tapped",tapped.tapped(),"stack_size",game.getStack().size()));
                if(event instanceof GameEventCardChangeZone moved) events.add(map("kind","zone","card_id",moved.card().getId(),"from",moved.from()==null?null:moved.from().zoneType().name(),"to",moved.to()==null?null:moved.to().zoneType().name(),"stack_size",game.getStack().size()));
                if(event instanceof GameEventCardSacrificed sacrificed) events.add(map("kind","sacrifice","card_id",sacrificed.card().getId(),"stack_size",game.getStack().size()));
                if(event instanceof GameEventManaPool mana) events.add(map("kind","mana","player",alias(game.getPlayer(mana.player().getId())),"mode",mana.mode().name(),"colors",mana.colors().toString(),"stack_size",game.getStack().size()));
                if(event instanceof GameEventSpellResolved resolved) events.add(map("kind","resolved","has_fizzled",resolved.hasFizzled(),"stack_size",game.getStack().size(),"description",resolved.stackDescription()));
            } catch(Throwable failure) {eventFailure=failure;}
        }
        Map<String,Object> finish(String id,String name,String departure,boolean stolenTarget,boolean tapped) {
            final List<Map<String,Object>> history=new ArrayList<>();
            for(SpellAbility ability:game.getStack().getAbilityActivatedThisTurn()) history.add(map("ability_id",ability.getId(),"host",identity(ability.getHostCard()),"controller",alias(ability.getActivatingPlayer()),"paid_mana",paidMana(ability)));
            return map("id",id,"source_card",name,"target_card","Hill Giant","comparison_scope","source".equals(departure)?"reference-only":"common",
                "intervention",departure,"snapshots",snapshots,"source_metadata",metadata,"ability_domain",abilityDomain,"callbacks",callbacks,"events",events,"actions",actions,
                "activation_history",history,"controller_class",actor.getController().getClass().getName(),"cost_visitor_class",StrictCosts.class.getName(),
                "counts",map("ability_selections",abilitySelections,"target_selections",targetSelections,"tap_decisions",tapDecisions,
                    "sacrifice_decisions",sacrificeDecisions,"mana_decisions",manaDecisions,"mana_payments",manaPayments));
        }
        List<String> characters(String value) { final List<String> values=new ArrayList<>(); for(char c:value.toCharArray()) values.add(String.valueOf(c)); return values; }
    }

    private static final class StrictCosts extends CostDecisionMakerBase {
        private final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script) { super(player,false,ability,ability.getHostCard()); this.script=script; }
        @Override public boolean paysRightAfterDecision() { return true; }
        @Override public PaymentDecision visit(CostTap cost) {
            Assert.assertSame(ability,script.selected); Assert.assertSame(player,script.payer); Assert.assertSame(source.getController(),player);
            Assert.assertTrue(cost.canPay(ability,player,false)); script.tapDecisions++; script.callbacks.add(map("kind","tap-decision","chooser",script.alias(player),"source",script.identity(source))); return PaymentDecision.number(0);
        }
        @Override public PaymentDecision visit(CostPartMana cost) {
            Assert.assertSame(ability,script.selected); Assert.assertFalse(cost.getManaCostFor(ability).isZero()); script.manaDecisions++; return PaymentDecision.number(0);
        }
        @Override public PaymentDecision visit(CostBehold cost) { throw new AssertionError("Unexpected cost: CostBehold"); }
        @Override public PaymentDecision visit(CostBeholdExile cost) { throw new AssertionError("Unexpected cost: CostBeholdExile"); }
        @Override public PaymentDecision visit(CostGainControl cost) { throw new AssertionError("Unexpected cost: CostGainControl"); }
        @Override public PaymentDecision visit(CostChooseColor cost) { throw new AssertionError("Unexpected cost: CostChooseColor"); }
        @Override public PaymentDecision visit(CostChooseCreatureType cost) { throw new AssertionError("Unexpected cost: CostChooseCreatureType"); }
        @Override public PaymentDecision visit(CostCollectEvidence cost) { throw new AssertionError("Unexpected cost: CostCollectEvidence"); }
        @Override public PaymentDecision visit(CostDiscard cost) { throw new AssertionError("Unexpected cost: CostDiscard"); }
        @Override public PaymentDecision visit(CostDamage cost) { throw new AssertionError("Unexpected cost: CostDamage"); }
        @Override public PaymentDecision visit(CostDraw cost) { throw new AssertionError("Unexpected cost: CostDraw"); }
        @Override public PaymentDecision visit(CostExile cost) { throw new AssertionError("Unexpected cost: CostExile"); }
        @Override public PaymentDecision visit(CostExileFromStack cost) { throw new AssertionError("Unexpected cost: CostExileFromStack"); }
        @Override public PaymentDecision visit(CostExiledMoveToGrave cost) { throw new AssertionError("Unexpected cost: CostExiledMoveToGrave"); }
        @Override public PaymentDecision visit(CostExert cost) { throw new AssertionError("Unexpected cost: CostExert"); }
        @Override public PaymentDecision visit(CostEnlist cost) { throw new AssertionError("Unexpected cost: CostEnlist"); }
        @Override public PaymentDecision visit(CostFlipCoin cost) { throw new AssertionError("Unexpected cost: CostFlipCoin"); }
        @Override public PaymentDecision visit(CostForage cost) { throw new AssertionError("Unexpected cost: CostForage"); }
        @Override public PaymentDecision visit(CostRollDice cost) { throw new AssertionError("Unexpected cost: CostRollDice"); }
        @Override public PaymentDecision visit(CostMill cost) { throw new AssertionError("Unexpected cost: CostMill"); }
        @Override public PaymentDecision visit(CostAddMana cost) { throw new AssertionError("Unexpected cost: CostAddMana"); }
        @Override public PaymentDecision visit(CostPayLife cost) { throw new AssertionError("Unexpected cost: CostPayLife"); }
        @Override public PaymentDecision visit(CostPayEnergy cost) { throw new AssertionError("Unexpected cost: CostPayEnergy"); }
        @Override public PaymentDecision visit(CostGainLife cost) { throw new AssertionError("Unexpected cost: CostGainLife"); }
        @Override public PaymentDecision visit(CostPromiseGift cost) { throw new AssertionError("Unexpected cost: CostPromiseGift"); }
        @Override public PaymentDecision visit(CostPutCardToLib cost) { throw new AssertionError("Unexpected cost: CostPutCardToLib"); }
        @Override public PaymentDecision visit(CostSacrifice cost) { Assert.assertSame(ability,script.selected); Assert.assertSame(player,script.payer); Assert.assertSame(source,script.source); Assert.assertTrue(cost.payCostFromSource()); Assert.assertEquals(cost.getAmount(),"1"); Assert.assertTrue(cost.canPay(ability,player,false)); script.sacrificeDecisions++; script.callbacks.add(map("kind","sacrifice-decision","chooser",script.alias(player),"source",script.identity(source))); return PaymentDecision.card(source); }
        @Override public PaymentDecision visit(CostReturn cost) { throw new AssertionError("Unexpected cost: CostReturn"); }
        @Override public PaymentDecision visit(CostReveal cost) { throw new AssertionError("Unexpected cost: CostReveal"); }
        @Override public PaymentDecision visit(CostRevealChosen cost) { throw new AssertionError("Unexpected cost: CostRevealChosen"); }
        @Override public PaymentDecision visit(CostRemoveAnyCounter cost) { throw new AssertionError("Unexpected cost: CostRemoveAnyCounter"); }
        @Override public PaymentDecision visit(CostRemoveCounter cost) { throw new AssertionError("Unexpected cost: CostRemoveCounter"); }
        @Override public PaymentDecision visit(CostPutCounter cost) { throw new AssertionError("Unexpected cost: CostPutCounter"); }
        @Override public PaymentDecision visit(CostPutCounterYou cost) { throw new AssertionError("Unexpected cost: CostPutCounterYou"); }
        @Override public PaymentDecision visit(CostUntapType cost) { throw new AssertionError("Unexpected cost: CostUntapType"); }
        @Override public PaymentDecision visit(CostUntap cost) { throw new AssertionError("Unexpected cost: CostUntap"); }
        @Override public PaymentDecision visit(CostUnattach cost) { throw new AssertionError("Unexpected cost: CostUnattach"); }
        @Override public PaymentDecision visit(CostTapType cost) { throw new AssertionError("Unexpected cost: CostTapType"); }
        @Override public PaymentDecision visit(CostPayShards cost) { throw new AssertionError("Unexpected cost: CostPayShards"); }
        @Override public PaymentDecision visit(CostBlight cost) { throw new AssertionError("Unexpected cost: CostBlight"); }
    }
    /** Pinned controller surface with only the explicit activation decisions above. */
    private static final class StrictController extends PlayerController {
        private final Script script;
        StrictController(Game game, Player player, Script script) {
            super(game, player, player.getLobbyPlayer());
            this.script = script;
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(getPlayer(), hostCard, abilities); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw new AssertionError("Unexpected controller operation: playSpellAbilityNoStack"); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { if (!activePlayerSAs.isEmpty()) throw new AssertionError("Unexpected triggers"); return java.util.Collections.emptyList(); }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { if (!activePlayerSAs.isEmpty()) throw new AssertionError("Unexpected triggers"); }
        @Override public boolean playTrigger(Card host, WrappedAbility wrapperAbility, boolean isMandatory) { throw new AssertionError("Unexpected controller operation: playTrigger"); }
        @Override public boolean playSaFromPlayEffect(SpellAbility tgtSA) { throw new AssertionError("Unexpected controller operation: playSaFromPlayEffect"); }
        @Override public List<PaperCard> sideboard(final Deck deck, GameType gameType, String message) { throw new AssertionError("Unexpected controller operation: sideboard"); }
        @Override public List<PaperCard> chooseCardsYouWonToAddToDeck(List<PaperCard> losses) { throw new AssertionError("Unexpected controller operation: chooseCardsYouWonToAddToDeck"); }
        @Override public Map<Card, Integer> assignCombatDamage(Card attacker, CardCollectionView blockers, CardCollectionView remaining, int damageDealt, GameEntity defender, boolean overrideOrder) { throw new AssertionError("Unexpected controller operation: assignCombatDamage"); }
        @Override public Map<GameEntity, Integer> divideShield(Card effectSource, Map<GameEntity, Integer> affected, int shieldAmount) { throw new AssertionError("Unexpected controller operation: divideShield"); }
        @Override public Map<Byte, Integer> specifyManaCombo(SpellAbility sa, ColorSet colorSet, int manaAmount, boolean different) { throw new AssertionError("Unexpected controller operation: specifyManaCombo"); }
        @Override public CardCollectionView choosePermanentsToSacrifice(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw new AssertionError("Unexpected controller operation: choosePermanentsToSacrifice"); }
        @Override public CardCollectionView choosePermanentsToDestroy(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw new AssertionError("Unexpected controller operation: choosePermanentsToDestroy"); }
        @Override public Integer announceRequirements(SpellAbility ability, int min, int max, String announce) { throw new AssertionError("Unexpected controller operation: announceRequirements"); }
        @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) { throw new AssertionError("Unexpected controller operation: chooseNewTargetsFor"); }
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { return script.target(getPlayer(),currentAbility); }
        @Override public Pair<SpellAbilityStackInstance, GameObject> chooseTarget(SpellAbility sa, List<Pair<SpellAbilityStackInstance, GameObject>> allTargets) { throw new AssertionError("Unexpected controller operation: chooseTarget"); }
        @Override public boolean helpPayForAssistSpell(ManaCostBeingPaid cost, SpellAbility sa, int max, int requested) { throw new AssertionError("Unexpected controller operation: helpPayForAssistSpell"); }
        @Override public Player choosePlayerToAssistPayment(FCollectionView<Player> optionList, SpellAbility sa, String title, int max) { throw new AssertionError("Unexpected controller operation: choosePlayerToAssistPayment"); }
        @Override public CardCollectionView chooseCardsForEffect(CardCollectionView sourceList, SpellAbility sa, String title, int min, int max, boolean isOptional, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseCardsForEffect"); }
        @Override public CardCollection chooseCardsForEffectMultiple(Map<String, CardCollection> validMap, SpellAbility sa, String title, boolean isOptional) { throw new AssertionError("Unexpected controller operation: chooseCardsForEffectMultiple"); }
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSingleEntityForEffect"); }
        @Override public <T extends GameEntity> List<T> chooseEntitiesForEffect(FCollectionView<T> optionList, int min, int max, DelayedReveal delayedReveal, SpellAbility sa, String title, Player relatedPlayer, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseEntitiesForEffect"); }
        @Override public List<SpellAbility> chooseSpellAbilitiesForEffect(List<SpellAbility> spells, SpellAbility sa, String title, int num, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSpellAbilitiesForEffect"); }
        @Override public SpellAbility chooseSingleSpellForEffect(List<SpellAbility> spells, SpellAbility sa, String title, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSingleSpellForEffect"); }
        @Override public boolean confirmAction(SpellAbility sa, PlayerActionConfirmMode mode, String message, List<String> options, Card cardToShow, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: confirmAction"); }
        @Override public boolean confirmBidAction(SpellAbility sa, PlayerActionConfirmMode bidlife, String string, int bid, Player winner) { throw new AssertionError("Unexpected controller operation: confirmBidAction"); }
        @Override public boolean confirmReplacementEffect(ReplacementEffect replacementEffect, SpellAbility effectSA, GameEntity affected, String question) { throw new AssertionError("Unexpected controller operation: confirmReplacementEffect"); }
        @Override public boolean confirmStaticApplication(Card hostCard, PlayerActionConfirmMode mode, String message, String logic) { throw new AssertionError("Unexpected controller operation: confirmStaticApplication"); }
        @Override public boolean confirmTrigger(WrappedAbility sa) { throw new AssertionError("Unexpected controller operation: confirmTrigger"); }
        @Override public List<Card> exertAttackers(List<Card> attackers) { throw new AssertionError("Unexpected controller operation: exertAttackers"); }
        @Override public List<Card> enlistAttackers(List<Card> attackers) { throw new AssertionError("Unexpected controller operation: enlistAttackers"); }
        @Override public void declareAttackers(Player attacker, Combat combat) { throw new AssertionError("Unexpected controller operation: declareAttackers"); }
        @Override public void declareBlockers(Player defender, Combat combat) { throw new AssertionError("Unexpected controller operation: declareBlockers"); }
        @Override public CardCollection orderBlockers(Card attacker, CardCollection blockers) { throw new AssertionError("Unexpected controller operation: orderBlockers"); }
        @Override public CardCollection orderBlocker(final Card attacker, final Card blocker, final CardCollection oldBlockers) { throw new AssertionError("Unexpected controller operation: orderBlocker"); }
        @Override public CardCollection orderAttackers(Card blocker, CardCollection attackers) { throw new AssertionError("Unexpected controller operation: orderAttackers"); }
        @Override public void reveal(CardCollectionView cards, ZoneType zone, Player owner, String messagePrefix, boolean addMsgSuffix) { /* notification only; no player decision */ }
        @Override public void reveal(List<CardView> cards, ZoneType zone, PlayerView owner, String messagePrefix, boolean addMsgSuffix) { /* notification only; no player decision */ }
        @Override public void notifyOfValue(SpellAbility saSource, GameObject realtedTarget, String value) { /* notification only; no player decision */ }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForScry(CardCollection topN) { throw new AssertionError("Unexpected controller operation: arrangeForScry"); }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForSurveil(CardCollection topN) { throw new AssertionError("Unexpected controller operation: arrangeForSurveil"); }
        @Override public boolean willPutCardOnTop(Card c) { throw new AssertionError("Unexpected controller operation: willPutCardOnTop"); }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destinationZone, SpellAbility source) { if (cards.size() > 1) throw new AssertionError("Unexpected zone ordering choice"); return cards; }
        @Override public CardCollectionView chooseCardsToDiscardFrom(Player playerDiscard, SpellAbility sa, CardCollection validCards, int min, int max, CardCollectionView visibleToChooser) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardFrom"); }
        @Override public CardCollectionView chooseCardsToDiscardUnlessType(int min, CardCollectionView hand, String[] unlessTypes, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardUnlessType"); }
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardToMaximumHandSize"); }
        @Override public CardCollectionView chooseCardsToDelve(int genericAmount, CardCollection grave) { throw new AssertionError("Unexpected controller operation: chooseCardsToDelve"); }
        @Override public Map<Card, ManaCostShard> chooseCardsForConvokeOrImprovise(SpellAbility sa, ManaCost manaCost, CardCollectionView untappedCards, boolean artifacts, boolean creatures, Integer maxReduction) { throw new AssertionError("Unexpected controller operation: chooseCardsForConvokeOrImprovise"); }
        @Override public List<Card> chooseCardsForSplice(SpellAbility sa, List<Card> cards) { throw new AssertionError("Unexpected controller operation: chooseCardsForSplice"); }
        @Override public CardCollectionView chooseCardsToRevealFromHand(int min, int max, CardCollectionView valid) { throw new AssertionError("Unexpected controller operation: chooseCardsToRevealFromHand"); }
        @Override public List<SpellAbility> chooseSaToActivateFromOpeningHand(List<SpellAbility> usableFromOpeningHand) { throw new AssertionError("Unexpected controller operation: chooseSaToActivateFromOpeningHand"); }
        @Override public Player chooseStartingPlayer(boolean isFirstGame) { throw new AssertionError("Unexpected controller operation: chooseStartingPlayer"); }
        @Override public PlayerZone chooseStartingHand(List<PlayerZone> zones) { throw new AssertionError("Unexpected controller operation: chooseStartingHand"); }
        @Override public Mana chooseManaFromPool(List<Mana> manaChoices) { throw new AssertionError("Unexpected controller operation: chooseManaFromPool"); }
        @Override public String chooseSomeType(String kindOfType, SpellAbility sa, Collection<String> validTypes, boolean isOptional) { throw new AssertionError("Unexpected controller operation: chooseSomeType"); }
        @Override public String chooseSector(Card assignee, String ai, List<String> sectors) { throw new AssertionError("Unexpected controller operation: chooseSector"); }
        @Override public Sticker chooseSticker(List<Sticker> options, Card target, SpellAbility sa, boolean isOptional) { throw new AssertionError("Unexpected controller operation: chooseSticker"); }
        @Override public int chooseStickerNamePosition(Sticker sticker, Card target) { throw new AssertionError("Unexpected controller operation: chooseStickerNamePosition"); }
        @Override public Card chooseCardToKeepStickers(CardCollectionView options) { throw new AssertionError("Unexpected controller operation: chooseCardToKeepStickers"); }
        @Override public List<Card> chooseContraptionsToCrank(List<Card> contraptions) { throw new AssertionError("Unexpected controller operation: chooseContraptionsToCrank"); }
        @Override public int chooseSprocket(Card assignee, List<Integer> sprockets) { throw new AssertionError("Unexpected controller operation: chooseSprocket"); }
        @Override public PlanarDice choosePDRollToIgnore(List<PlanarDice> rolls) { throw new AssertionError("Unexpected controller operation: choosePDRollToIgnore"); }
        @Override public Integer chooseRollToIgnore(List<Integer> rolls) { throw new AssertionError("Unexpected controller operation: chooseRollToIgnore"); }
        @Override public List<Integer> chooseDiceToReroll(List<Integer> rolls) { throw new AssertionError("Unexpected controller operation: chooseDiceToReroll"); }
        @Override public Integer chooseRollToModify(List<Integer> rolls) { throw new AssertionError("Unexpected controller operation: chooseRollToModify"); }
        @Override public RollDiceEffect.DieRollResult chooseRollToSwap(List<RollDiceEffect.DieRollResult> rolls) { throw new AssertionError("Unexpected controller operation: chooseRollToSwap"); }
        @Override public String chooseRollSwapValue(List<String> swapChoices, Integer currentResult, int power, int toughness) { throw new AssertionError("Unexpected controller operation: chooseRollSwapValue"); }
        @Override public Object vote(SpellAbility sa, String prompt, List<Object> options, ListMultimap<Object, Player> votes, Player forPlayer, boolean optional) { throw new AssertionError("Unexpected controller operation: vote"); }
        @Override public boolean mulliganKeepHand(Player player, int cardsToReturn) { throw new AssertionError("Unexpected controller operation: mulliganKeepHand"); }
        @Override public CardCollectionView tuckCardsViaMulligan(CardCollectionView hand, int cardsToReturn) { throw new AssertionError("Unexpected controller operation: tuckCardsViaMulligan"); }
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { throw new AssertionError("Unexpected controller operation: chooseSpellAbilityToPlay"); }
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { throw new AssertionError("Unexpected controller operation: playChosenSpellAbility"); }
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { throw new AssertionError("Unexpected controller operation: chooseModeForAbility"); }
        @Override public int chooseNumberForCostReduction(final SpellAbility sa, final int min, final int max) { throw new AssertionError("Unexpected controller operation: chooseNumberForCostReduction"); }
        @Override public int chooseNumberForKeywordCost(SpellAbility sa, Cost cost, KeywordInterface keyword, String prompt, int max) { throw new AssertionError("Unexpected controller operation: chooseNumberForKeywordCost"); }
        @Override public int chooseNumber(SpellAbility sa, String title, int min, int max) { throw new AssertionError("Unexpected controller operation: chooseNumber"); }
        @Override public int chooseNumber(SpellAbility sa, String title, List<Integer> values, Player relatedPlayer) { throw new AssertionError("Unexpected controller operation: chooseNumber"); }
        @Override public boolean chooseBinary(SpellAbility sa, String question, BinaryChoiceType kindOfChoice, Boolean defaultChoice) { throw new AssertionError("Unexpected controller operation: chooseBinary"); }
        @Override public boolean chooseFlipResult(SpellAbility sa, Player flipper, boolean call) { throw new AssertionError("Unexpected controller operation: chooseFlipResult"); }
        @Override public byte chooseColor(String message, SpellAbility sa, ColorSet colors) { throw new AssertionError("Unexpected controller operation: chooseColor"); }
        @Override public byte chooseColorAllowColorless(String message, Card c, ColorSet colors) { throw new AssertionError("Unexpected controller operation: chooseColorAllowColorless"); }
        @Override public ColorSet chooseColors(String message, SpellAbility sa, int min, int max, ColorSet options) { throw new AssertionError("Unexpected controller operation: chooseColors"); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, String message, Predicate<ICardFace> cpp, String name) { throw new AssertionError("Unexpected controller operation: chooseSingleCardFace"); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseSingleCardFace"); }
        @Override public CardState chooseSingleCardState(SpellAbility sa, List<CardState> states, String message, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSingleCardState"); }
        @Override public boolean chooseCardsPile(SpellAbility sa, CardCollectionView pile1, CardCollectionView pile2, String faceUp) { throw new AssertionError("Unexpected controller operation: chooseCardsPile"); }
        @Override public CounterType chooseCounterType(List<CounterType> options, SpellAbility sa, String prompt, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseCounterType"); }
        @Override public String chooseKeywordForPump(List<String> options, SpellAbility sa, String prompt, Card tgtCard) { throw new AssertionError("Unexpected controller operation: chooseKeywordForPump"); }
        @Override public boolean confirmPayment(CostPart costPart, String string, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: confirmPayment"); }
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { throw new AssertionError("Unexpected controller operation: chooseSingleReplacementEffect"); }
        @Override public StaticAbility chooseSingleStaticAbility(List<StaticAbility> possibleReplacers) { throw new AssertionError("Unexpected controller operation: chooseSingleStaticAbility"); }
        @Override public String chooseProtectionType(SpellAbility sa, List<String> choices) { throw new AssertionError("Unexpected controller operation: chooseProtectionType"); }
        @Override public void revealAnte(String message, Multimap<Player, PaperCard> removedAnteCards) { throw new AssertionError("Unexpected controller operation: revealAnte"); }
        @Override public void revealAISkipCards(String message, Map<Player, Map<DeckSection, List<? extends PaperCard>>> deckCards) { throw new AssertionError("Unexpected controller operation: revealAISkipCards"); }
        @Override public void revealUnsupported(Map<Player, List<PaperCard>> unsupported) { throw new AssertionError("Unexpected controller operation: revealUnsupported"); }
        @Override public List<OptionalCostValue> chooseOptionalCosts(SpellAbility choosen, List<OptionalCostValue> optionalCostValues) { throw new AssertionError("Unexpected controller operation: chooseOptionalCosts"); }
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { return script.order(getPlayer(),costs); }
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { throw new AssertionError("Unexpected controller operation: payCostToPreventEffect"); }
        @Override public boolean payCostDuringRoll(Cost cost, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: payCostDuringRoll"); }
        @Override public boolean payCombatCost(Card card, Cost cost, SpellAbility sa, String prompt) { throw new AssertionError("Unexpected controller operation: payCombatCost"); }
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.pay(getPlayer(),toPay,costPartMana,sa,prompt,matrix,effect); }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.apply(getPlayer(),toPay,ability,effect); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw new AssertionError("Unexpected controller operation: chooseCardsForCost"); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { Assert.assertEquals(player, script.payer); Assert.assertSame(ability, script.selected); Assert.assertFalse(effect); return new StrictCosts(player, ability, script); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw new AssertionError("Unexpected controller operation: chooseSingleCardForZoneChange"); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw new AssertionError("Unexpected controller operation: chooseCardsForZoneChange"); }
        @Override public void autoPassCancel() { /* notification only; no player decision */ }
        @Override public void awaitNextInput() { /* notification only; no player decision */ }
        @Override public void cancelAwaitNextInput() { /* notification only; no player decision */ }
    }
}
