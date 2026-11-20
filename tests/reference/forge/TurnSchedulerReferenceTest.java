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


import forge.game.phase.PhaseType;
import forge.game.player.RegisteredPlayer;

/** Strict actual turn-loop probes. Trusted fixture seeds are disclosed in each trace. */
public class TurnSchedulerReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256="6c1aa6149c5da27ee4b97fc2a23e6c67fa1e7364332c385d948b4510a0ea2e4e";
    private static final String COLORS="WUBRGC";
    @Test public void normalTurnOrder() throws IOException { run("turn-order"); }
    @Test public void boundaryMana() throws IOException { run("boundary-mana"); }
    @Test public void stackResolution() throws IOException { run("stack-resolution"); }
    @Test public void repeatedCleanup() throws IOException { run("cleanup-repeat"); }
    @Test public void combatOrder() throws IOException { run("combat-order"); }

    private Game freshGame() {
        final List<RegisteredPlayer> players=new ArrayList<>();
        players.add(new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-B",null)));
        players.add(new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-A",null)));
        final GameRules rules=new GameRules(GameType.Constructed);
        final Game game=new Game(players,rules,new Match(rules,players,"Turn reference"));
        game.setAge(GameStage.Play); return game;
    }
    private void run(String scenario) throws IOException {
        final Script script=new Script(freshGame(),scenario); boolean completed=false;
        try {
            script.seed(); script.game.subscribeToEvents(script);
            script.observe("before-first-turn");
            script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            script.observe("first-untap-entered");
            for(int i=0;i<160;i++) {
                Assert.assertFalse(script.game.isGameOver());
                final Map<String,Object> before=script.state();
                final PhaseType step=script.game.getPhaseHandler().getPhase();
                final int callbacks=script.priorities.size();
                script.game.getPhaseHandler().mainLoopStep();
                final Map<String,Object> after=script.state();
                script.loops.add(map("ordinal",i,"before",before,"after",after,"priority_callbacks",script.priorities.size()-callbacks));
                if(script.eventFailure!=null) throw new AssertionError("Event observer failed",script.eventFailure);
                if(scenario.equals("boundary-mana") && step==PhaseType.UPKEEP && script.game.getPhaseHandler().getPhase()==step && script.priorities.size()>callbacks) script.observe("upkeep-after-one-pass");
                if(scenario.equals("boundary-mana") && step==PhaseType.UPKEEP && script.game.getPhaseHandler().getPhase()!=step) script.observe("after-upkeep");
                if(scenario.equals("boundary-mana") && step==PhaseType.MAIN1 && script.game.getPhaseHandler().getPhase()==step && script.priorities.size()>callbacks) script.observe("main-after-one-pass");
                if(scenario.equals("boundary-mana") && step==PhaseType.MAIN1 && script.game.getPhaseHandler().getPhase()!=step) script.observe("after-main");
                if(scenario.equals("stack-resolution") && script.cast && script.game.getStack().isEmpty() && !script.resolutionObserved) { script.resolutionObserved=true;script.observe("after-resolution"); }
                if(scenario.equals("stack-resolution") && step==PhaseType.UPKEEP && script.game.getPhaseHandler().getPhase()!=step) {script.observe("after-step");break;}
                if(scenario.equals("boundary-mana") && script.game.getPhaseHandler().getPhase()==PhaseType.COMBAT_BEGIN) break;
                if(!scenario.equals("turn-order") && !scenario.equals("boundary-mana") && !scenario.equals("stack-resolution") && script.game.getPhaseHandler().getTurn()==2) {script.observe("next-turn");break;}
                if(scenario.equals("turn-order") && script.game.getPhaseHandler().getTurn()==3) {script.observe("third-turn");break;}
                Assert.assertTrue(i<159,"Loop bound exhausted");
            }
            script.validate(); completed=true;
        } finally {
            final Map<String,Object> trace=map("schema_version",1,"scenario",scenario,"reference","forge","commit",COMMIT,
                "expected_sha256",EXPECTED_SHA256,"completed",completed,"strict_choices",true,"ai_policy_used",false,
                "priority_loop_executed",true,"dev_phase_advance_used",false,"direct_resolution_used",false,
                "setup","Fresh two-player Game; strict controllers replace initial lobby controllers before setupFirstTurn(A,null). Cards, libraries, starting taps/damage and floating mana are trusted fixture seeds.",
                "raw_phase_event_caveat","Forge emits phase events even for skipped internal markers; priority callbacks and actual work provide semantic evidence.",
                "phases",script.phases,"priorities",script.priorities,"actions",script.actions,"events",script.events,
                "loops",script.loops,"snapshots",script.snapshots,"metadata",script.metadata,
                "discard_choices",script.discards,"trigger_admissions",script.triggers,"actual_paid_casts",script.cast?1:0);
            final String directory=System.getProperty("spellwake.evidence.dir");Assert.assertNotNull(directory);
            Files.write(Paths.get(directory,scenario+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),
                java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario="+scenario+" completed="+completed+" priorities="+script.priorities.size());
        }
    }
    private static byte color(char c) {return ManaAtom.fromName(String.valueOf(c));}
    private static Map<String,Object> pool(ManaPool pool) {final Map<String,Object> values=new LinkedHashMap<>();for(char c:COLORS.toCharArray()) values.put(String.valueOf(c),pool.getAmountOfColor(color(c)));return values;}
    private static Map<String,Object> map(Object... pairs) {final Map<String,Object> result=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)result.put((String)pairs[i],pairs[i+1]);return result;}
    public static final class Script {
        final Game game;final Player actor;final Player other;final String scenario;
        final List<Map<String,Object>> phases=new ArrayList<>(),priorities=new ArrayList<>(),actions=new ArrayList<>(),events=new ArrayList<>(),loops=new ArrayList<>(),snapshots=new ArrayList<>();
        final Map<String,Object> metadata=new LinkedHashMap<>();
        Card source,otherSource,target,response,megrim;SpellAbility selected;Player payer;
        boolean cast,resolutionObserved;int discards,triggers;Throwable eventFailure;
        Script(Game game,String scenario) {this.game=game;this.scenario=scenario;actor=game.getPlayers().get(1);other=game.getPlayers().get(0);
            for(Player player:game.getPlayers())player.addController(game.getNextTimestamp(),player,new StrictController(game,player,this),false);
            Assert.assertFalse(actor.getController().isAI());Assert.assertFalse(other.getController().isAI());}
        String alias(Player p) {if(p==actor)return "A";if(p==other)return "B";throw new AssertionError("Unknown player");}
        Card current(Card c) {return game.getCardState(c);}
        Card seedCard(String id,String name,Player owner,ZoneType zone) {
            final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name);Assert.assertNotNull(paper);
            final Card card=Card.fromPaperCard(paper,owner);card.setGameTimestamp(game.getNextTimestamp());owner.getZone(zone).add(card);
            metadata.put(id,map("name",name,"owner",alias(owner),"zone",zone.name(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText()));return card;
        }
        void seed() {
            source=seedCard("source","Island",actor,ZoneType.Battlefield);otherSource=seedCard("other-source","Island",other,ZoneType.Battlefield);
            source.setTapped(true);otherSource.setTapped(true);target=seedCard("target","Hill Giant",actor,ZoneType.Battlefield);target.setDamage(1);
            for(int i=0;i<5;i++){seedCard("A-library-"+i,"Island",actor,ZoneType.Library);seedCard("B-library-"+i,"Island",other,ZoneType.Library);}
            if(scenario.equals("boundary-mana")){floating(actor,'W',1);floating(other,'C',1);}
            if(scenario.equals("stack-resolution"))response=seedCard("response","Unsummon",actor,ZoneType.Hand);
            if(scenario.equals("cleanup-repeat")){megrim=seedCard("megrim","Megrim",other,ZoneType.Battlefield);for(int i=0;i<8;i++)seedCard("discard-"+i,"Island",actor,ZoneType.Hand);}
        }
        void floating(Player player,char type,int count){for(int i=0;i<count;i++)player.getManaPool().addMana(new Mana(color(type),source,null,player));actions.add(map("kind","trusted-floating-seed","player",alias(player),"color",String.valueOf(type),"count",count,"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name()));}
        Map<String,Object> state() {return map("turn",game.getPhaseHandler().getTurn(),"active",game.getPhaseHandler().getPlayerTurn()==null?null:alias(game.getPhaseHandler().getPlayerTurn()),
            "step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"priority",game.getPhaseHandler().getPriorityPlayer()==null?null:alias(game.getPhaseHandler().getPriorityPlayer()),
            "stack_size",game.getStack().size(),"pools",map("A",pool(actor.getManaPool()),"B",pool(other.getManaPool())),"A_tapped",current(source).isTapped(),"B_tapped",current(otherSource).isTapped(),
            "target_zone",current(target).getZone().getZoneType().name(),"target_damage",current(target).getDamage(),"response_zone",response==null?null:current(response).getZone().getZoneType().name(),
            "A_hand",actor.getCardsIn(ZoneType.Hand).size(),"B_hand",other.getCardsIn(ZoneType.Hand).size(),"A_library",actor.getCardsIn(ZoneType.Library).size(),"B_library",other.getCardsIn(ZoneType.Library).size(),
            "A_graveyard",actor.getCardsIn(ZoneType.Graveyard).size(),"A_life",actor.getLife(),"B_life",other.getLife());}
        void observe(String label){final Map<String,Object> value=state();value.put("label",label);snapshots.add(value);}
        List<SpellAbility> choose(Player player) {
            final PhaseType step=game.getPhaseHandler().getPhase();Assert.assertNotEquals(step,PhaseType.UNTAP);
            if(scenario.equals("boundary-mana") && player==actor && game.getPhaseHandler().getTurn()==1 && (step==PhaseType.UPKEEP||step==PhaseType.MAIN1)) {
                if(step==PhaseType.UPKEEP)observe("first-upkeep");floating(actor,'W',2);floating(other,'C',3);observe(step==PhaseType.UPKEEP?"upkeep-seeded":"main-seeded");
            }
            if(scenario.equals("stack-resolution") && player==actor && !cast){floating(actor,'U',2);floating(other,'C',3);}
            final Map<String,Object> point=state();point.put("ordinal",priorities.size());point.put("chooser",alias(player));priorities.add(point);
            if(game.getPhaseHandler().getTurn()==1 && step==PhaseType.UPKEEP && player==actor && priorities.size()==1)observe("A-first-upkeep");
            if(game.getPhaseHandler().getTurn()==2 && step==PhaseType.UPKEEP && player==other)observe("B-first-upkeep");
            if(game.getPhaseHandler().getTurn()==2 && step==PhaseType.DRAW && player==other)observe("B-first-draw");
            if(scenario.equals("stack-resolution") && player==actor && !cast){selected=current(response).getFirstSpellAbility();payer=actor;selected.setActivatingPlayer(actor);Assert.assertTrue(selected.canPlay());return java.util.Collections.singletonList(selected);}
            return null;
        }
        boolean play(Player player,SpellAbility ability){Assert.assertSame(player,actor);Assert.assertSame(ability,selected);Assert.assertFalse(cast);Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));cast=true;observe("after-cast");actions.add(map("kind","actual-paid-cast","actor",alias(player),"stack_size",game.getStack().size()));return true;}
        SpellAbility select(Player player,Card host,List<SpellAbility> options){Assert.assertSame(player,payer);Assert.assertEquals(options.size(),1);Assert.assertSame(options.get(0),selected);return selected;}
        boolean target(Player player,SpellAbility ability){Assert.assertSame(player,actor);Assert.assertSame(ability,selected);Assert.assertTrue(ability.canTarget(current(target)));ability.getTargets().add(current(target));return true;}
        List<CostPart> order(Player player,List<CostPart> costs){Assert.assertSame(player,payer);for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        boolean pay(Player player,ManaCost toPay,CostPartMana cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(player,payer);Assert.assertSame(ability,selected);if(ability.isTrigger()){Assert.assertEquals(scenario,"cleanup-repeat");Assert.assertSame(player,other);Assert.assertTrue(toPay.isZero());Assert.assertTrue(effect);}else{Assert.assertSame(player,actor);Assert.assertFalse(effect);}return PlaySpellAbility.payManaCost(player.getController(),toPay,cost,ability,player,prompt,matrix,effect);}
        boolean apply(Player player,ManaCostBeingPaid toPay,SpellAbility ability,boolean effect){Assert.assertSame(player,actor);Assert.assertSame(ability,selected);Assert.assertFalse(effect);player.pushPaidForSA(ability);ability.setManaCostBeingPaid(toPay);try{Assert.assertTrue(player.getManaPool().tryPayCostWithColor(color('U'),ability,toPay,ability.getPayingMana()));Assert.assertTrue(toPay.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);player.popPaidForSA();}}
        void nestedTrigger(Player player,SpellAbility ability,boolean mayChooseNewTargets){Assert.assertEquals(scenario,"cleanup-repeat");Assert.assertSame(player,other);Assert.assertFalse(mayChooseNewTargets);Assert.assertEquals(game.getPhaseHandler().getPhase(),PhaseType.CLEANUP);Assert.assertEquals(game.getStack().size(),1);Assert.assertEquals(ability.getHostCard().getName(),"Megrim");Assert.assertFalse(ability.usesTargeting());Assert.assertEquals(ability.getApi(),forge.game.ability.ApiType.DealDamage);selected=ability;payer=other;actions.add(map("kind","actual-nested-trigger-resolution-callback","stack_size",game.getStack().size(),"step",game.getPhaseHandler().getPhase().name()));Assert.assertTrue(PlaySpellAbility.playSpellAbilityNoStack(player.getController(),player,ability,true));}
        void trigger(Player player,List<SpellAbility> abilities){Assert.assertEquals(scenario,"cleanup-repeat");Assert.assertSame(player,other);Assert.assertEquals(abilities.size(),1);selected=abilities.get(0);payer=other;Assert.assertEquals(selected.getHostCard().getName(),"Megrim");Assert.assertTrue(selected.isTrigger());Assert.assertFalse(selected.usesTargeting());Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,selected));triggers++;actions.add(map("kind","actual-trigger-admission","step",game.getPhaseHandler().getPhase().name(),"stack_size",game.getStack().size()));}
        CardCollectionView discard(Player player,int count){Assert.assertEquals(scenario,"cleanup-repeat");Assert.assertSame(player,actor);Assert.assertEquals(count,1);Assert.assertEquals(player.getCardsIn(ZoneType.Hand).size(),8);discards++;final CardCollection result=new CardCollection();result.add(player.getCardsIn(ZoneType.Hand).get(0));actions.add(map("kind","maximum-hand-discard-choice","card_id",result.get(0).getId(),"step",game.getPhaseHandler().getPhase().name()));return result;}
        void attackers(Player player,Combat combat){Assert.assertSame(player,actor);if(scenario.equals("combat-order")){Assert.assertTrue(combat.getAttackers().isEmpty());combat.addAttacker(current(target),other);actions.add(map("kind","declare-one-attacker","step",game.getPhaseHandler().getPhase().name()));}else actions.add(map("kind","declare-no-attackers","step",game.getPhaseHandler().getPhase().name()));}
        void blockers(Player player,Combat combat){Assert.assertEquals(scenario,"combat-order");Assert.assertSame(player,other);Assert.assertEquals(combat.getAttackers().size(),1);actions.add(map("kind","declare-no-blockers","step",game.getPhaseHandler().getPhase().name()));}
        @Subscribe public void onEvent(GameEvent event){if(eventFailure!=null)return;try{
            if(event instanceof GameEventTurnPhase phase)phases.add(map("turn",game.getPhaseHandler().getTurn(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"step",phase.phase().name(),"phase_marker",phase.phaseDesc()));
            if(event instanceof GameEventPlayerPriority priority)events.add(map("kind","priority-event","step",priority.phase().name(),"priority",alias(game.getPlayer(priority.priority().getId()))));
            if(event instanceof GameEventSpellResolved resolved)events.add(map("kind","resolved","step",game.getPhaseHandler().getPhase().name(),"fizzled",resolved.hasFizzled()));
            if(event instanceof GameEventCardChangeZone moved)events.add(map("kind","zone","card_id",moved.card().getId(),"from",moved.from()==null?null:moved.from().zoneType().name(),"to",moved.to()==null?null:moved.to().zoneType().name()));
        }catch(Throwable failure){eventFailure=failure;}}
        void validate(){Assert.assertTrue(priorities.size()>0);for(Map<String,Object> p:priorities)Assert.assertNotEquals(p.get("step"),"UNTAP");
            if(scenario.equals("cleanup-repeat")){Assert.assertEquals(discards,1);Assert.assertEquals(triggers,1);Assert.assertEquals(actor.getLife(),18);}
            else for(Map<String,Object> p:priorities)Assert.assertNotEquals(p.get("step"),"CLEANUP");
            if(scenario.equals("stack-resolution")){Assert.assertTrue(cast);Assert.assertTrue(resolutionObserved);Assert.assertEquals(current(target).getZone().getZoneType(),ZoneType.Hand);}
            if(scenario.equals("combat-order"))Assert.assertEquals(other.getLife(),17);
        }
    }
    private static final class StrictCosts extends CostDecisionMakerBase {
        StrictCosts(Player player,SpellAbility ability,Script script) { super(player,ability.isTrigger(),ability,ability.getHostCard()); }
        @Override public boolean paysRightAfterDecision() {return true;}
        @Override public PaymentDecision visit(CostTap cost) {throw new AssertionError("Unexpected cost: CostTap");}
        @Override public PaymentDecision visit(CostPartMana cost) {return PaymentDecision.number(0);}
        @Override public PaymentDecision visit(CostBehold cost) {throw new AssertionError("Unexpected cost: CostBehold");}
        @Override public PaymentDecision visit(CostBeholdExile cost) {throw new AssertionError("Unexpected cost: CostBeholdExile");}
        @Override public PaymentDecision visit(CostGainControl cost) {throw new AssertionError("Unexpected cost: CostGainControl");}
        @Override public PaymentDecision visit(CostChooseColor cost) {throw new AssertionError("Unexpected cost: CostChooseColor");}
        @Override public PaymentDecision visit(CostChooseCreatureType cost) {throw new AssertionError("Unexpected cost: CostChooseCreatureType");}
        @Override public PaymentDecision visit(CostCollectEvidence cost) {throw new AssertionError("Unexpected cost: CostCollectEvidence");}
        @Override public PaymentDecision visit(CostDiscard cost) {throw new AssertionError("Unexpected cost: CostDiscard");}
        @Override public PaymentDecision visit(CostDamage cost) {throw new AssertionError("Unexpected cost: CostDamage");}
        @Override public PaymentDecision visit(CostDraw cost) {throw new AssertionError("Unexpected cost: CostDraw");}
        @Override public PaymentDecision visit(CostExile cost) {throw new AssertionError("Unexpected cost: CostExile");}
        @Override public PaymentDecision visit(CostExileFromStack cost) {throw new AssertionError("Unexpected cost: CostExileFromStack");}
        @Override public PaymentDecision visit(CostExiledMoveToGrave cost) {throw new AssertionError("Unexpected cost: CostExiledMoveToGrave");}
        @Override public PaymentDecision visit(CostExert cost) {throw new AssertionError("Unexpected cost: CostExert");}
        @Override public PaymentDecision visit(CostEnlist cost) {throw new AssertionError("Unexpected cost: CostEnlist");}
        @Override public PaymentDecision visit(CostFlipCoin cost) {throw new AssertionError("Unexpected cost: CostFlipCoin");}
        @Override public PaymentDecision visit(CostForage cost) {throw new AssertionError("Unexpected cost: CostForage");}
        @Override public PaymentDecision visit(CostRollDice cost) {throw new AssertionError("Unexpected cost: CostRollDice");}
        @Override public PaymentDecision visit(CostMill cost) {throw new AssertionError("Unexpected cost: CostMill");}
        @Override public PaymentDecision visit(CostAddMana cost) {throw new AssertionError("Unexpected cost: CostAddMana");}
        @Override public PaymentDecision visit(CostPayLife cost) {throw new AssertionError("Unexpected cost: CostPayLife");}
        @Override public PaymentDecision visit(CostPayEnergy cost) {throw new AssertionError("Unexpected cost: CostPayEnergy");}
        @Override public PaymentDecision visit(CostGainLife cost) {throw new AssertionError("Unexpected cost: CostGainLife");}
        @Override public PaymentDecision visit(CostPromiseGift cost) {throw new AssertionError("Unexpected cost: CostPromiseGift");}
        @Override public PaymentDecision visit(CostPutCardToLib cost) {throw new AssertionError("Unexpected cost: CostPutCardToLib");}
        @Override public PaymentDecision visit(CostSacrifice cost) {throw new AssertionError("Unexpected cost: CostSacrifice");}
        @Override public PaymentDecision visit(CostReturn cost) {throw new AssertionError("Unexpected cost: CostReturn");}
        @Override public PaymentDecision visit(CostReveal cost) {throw new AssertionError("Unexpected cost: CostReveal");}
        @Override public PaymentDecision visit(CostRevealChosen cost) {throw new AssertionError("Unexpected cost: CostRevealChosen");}
        @Override public PaymentDecision visit(CostRemoveAnyCounter cost) {throw new AssertionError("Unexpected cost: CostRemoveAnyCounter");}
        @Override public PaymentDecision visit(CostRemoveCounter cost) {throw new AssertionError("Unexpected cost: CostRemoveCounter");}
        @Override public PaymentDecision visit(CostPutCounter cost) {throw new AssertionError("Unexpected cost: CostPutCounter");}
        @Override public PaymentDecision visit(CostPutCounterYou cost) {throw new AssertionError("Unexpected cost: CostPutCounterYou");}
        @Override public PaymentDecision visit(CostUntapType cost) {throw new AssertionError("Unexpected cost: CostUntapType");}
        @Override public PaymentDecision visit(CostUntap cost) {throw new AssertionError("Unexpected cost: CostUntap");}
        @Override public PaymentDecision visit(CostUnattach cost) {throw new AssertionError("Unexpected cost: CostUnattach");}
        @Override public PaymentDecision visit(CostTapType cost) {throw new AssertionError("Unexpected cost: CostTapType");}
        @Override public PaymentDecision visit(CostPayShards cost) {throw new AssertionError("Unexpected cost: CostPayShards");}
        @Override public PaymentDecision visit(CostBlight cost) {throw new AssertionError("Unexpected cost: CostBlight");}
    }
    /** Pinned controller surface. Every choice is explicit and all other decisions throw. */
    private static final class StrictController extends PlayerController {
        private final Script script;
        StrictController(Game game, Player player, Script script) {
            super(game, player, player.getLobbyPlayer());
            this.script = script;
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(getPlayer(), hostCard, abilities); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { script.nestedTrigger(getPlayer(),effectSA,mayChoseNewTargets); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.size()<=1); return activePlayerSAs; }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { if (!activePlayerSAs.isEmpty()) script.trigger(getPlayer(),activePlayerSAs); }
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
        @Override public void declareAttackers(Player attacker, Combat combat) { script.attackers(getPlayer(),combat); }
        @Override public void declareBlockers(Player defender, Combat combat) { script.blockers(getPlayer(),combat); }
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
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { return script.discard(getPlayer(),numDiscard); }
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
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { return script.choose(getPlayer()); }
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { return script.play(getPlayer(),sa); }
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
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { Assert.assertEquals(player, script.payer); Assert.assertSame(ability, script.selected); Assert.assertEquals(effect,ability.isTrigger()); return new StrictCosts(player, ability, script); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw new AssertionError("Unexpected controller operation: chooseSingleCardForZoneChange"); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw new AssertionError("Unexpected controller operation: chooseCardsForZoneChange"); }
        @Override public void autoPassCancel() { /* notification only; no player decision */ }
        @Override public void awaitNextInput() { /* notification only; no player decision */ }
        @Override public void cancelAwaitNextInput() { /* notification only; no player decision */ }
    }
}
