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

import forge.game.player.PlaySpellAbility;
import com.google.common.eventbus.Subscribe;


import forge.game.phase.PhaseType;
import forge.game.player.RegisteredPlayer;



/** Explicit life and continuous-control observations through actual turn loops.
 * Trusted controller/correction interventions are labeled separately from player actions.
 */
public class SetupControlReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256="050785eda950be247c610d8b23b86a6ef35caef8cd61f0116f4c67e764c4df57";
    @Test public void seededPermanentsAtOwnTurnStarts() throws IOException { run("seeded",20,20); }
    @Test public void castCreatureKeepsAgeThroughOpponentTurn() throws IOException { run("cast",20,20); }
    @Test public void sameControllerPreservesAge() throws IOException { run("same",20,20); }
    @Test public void changedControllerWaitsForOwnTurn() throws IOException { run("change",20,20); }
    @Test public void awayAndBackDoesNotRestoreAge() throws IOException { run("away",20,20); }
    @Test public void explicitFortyLife() throws IOException { run("forty",40,40); }
    @Test public void explicitNonuniformLife() throws IOException { run("nonuniform",20,40); }
    @Test public void zeroLifeBeforeFirstChecks() throws IOException { run("zero",0,20); }
    @Test public void negativeLifeBeforeFirstChecks() throws IOException { run("negative",-1,40); }
    @Test public void bothZeroSimultaneousLoss() throws IOException { run("both-zero",0,0); }
    private Game freshGame(int lifeA,int lifeB) {
        final List<RegisteredPlayer> players=new ArrayList<>();
        final RegisteredPlayer b=new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-B",null));b.setStartingLife(lifeB);players.add(b);
        final RegisteredPlayer a=new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-A",null));a.setStartingLife(lifeA);players.add(a);
        final GameRules rules=new GameRules(GameType.Constructed);
        final Game game=new Game(players,rules,new Match(rules,players,"Setup/control reference"));
        game.setAge(GameStage.Play);return game;
    }
    private void run(String scenario,int lifeA,int lifeB) throws IOException {
        final Script script=new Script(freshGame(lifeA,lifeB),scenario);boolean completed=false;
        try {
            script.seed();script.game.subscribeToEvents(script);
            if(scenario.equals("seeded"))script.record("seeded-before-turn","construction");
            if(script.lifeOnly())script.record(scenario+"-before","construction");
            script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            if(script.terminal())script.record(scenario+"-after-untap","after-setupFirstTurn");
            for(int i=0;i<160;i++) {
                if(script.game.isGameOver())break;
                final int before=script.priorities.size();script.game.getPhaseHandler().mainLoopStep();
                script.loops.add(map("ordinal",i,"priority_callbacks",script.priorities.size()-before,"after",script.control()));
                if(script.eventFailure!=null)throw new AssertionError("Observer failure",script.eventFailure);
                if(script.done)break;
                Assert.assertTrue(i<159,"bounded loop exhausted");
            }
            if(script.terminal()){Assert.assertTrue(script.game.isGameOver());script.record(scenario+"-ended","after-ordinary-loop-checks");script.done=true;}
            else Assert.assertFalse(script.game.isGameOver());
            Assert.assertTrue(script.done);completed=true;
        } finally {
            final Map<String,Object> trace=map("schema_version",1,"scenario",scenario,"reference","forge","commit",COMMIT,"expected_sha256",EXPECTED_SHA256,
                "completed",completed,"strict_choices",true,"ai_policy_used",false,"priority_loop_executed",true,"dev_phase_advance_used",false,"direct_resolution_used",false,
                "setup","Explicit RegisteredPlayer starting life before Game construction; both lobby controllers replaced with strict non-AI controllers before setupFirstTurn(A,null). Exact ordinary Card.fromPaperCard seeds, startsGameInPlay false, four Island library cards per player; one disclosed floating GGCCCC for the cast case.",
                "api_boundary","Real PhaseHandler mainLoopStep for turn beginnings, paid spell resolution and SBA. Control rows use trusted setController plus immediate controllerChangeZoneCorrection on each leg, not a cast control spell or layer computation. No raw-setter-only equivalence, attack or whole-game qualification.",
                "registered_life",map("A",String.valueOf(lifeA),"B",String.valueOf(lifeB)),"rows",script.rows,"priorities",script.priorities,"loops",script.loops,"actions",script.actions,"events",script.events,"metadata",script.metadata);
            final String directory=System.getProperty("spellwake.evidence.dir");Assert.assertNotNull(directory);
            Files.write(Paths.get(directory,scenario+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario="+scenario+" completed="+completed+" rows="+script.rows.size()+" priorities="+script.priorities.size());
        }
    }
    private static Map<String,Object> map(Object... pairs){final Map<String,Object> out=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)out.put((String)pairs[i],pairs[i+1]);return out;}
    public static final class Script {
        final Game game;final Player actor,other;final String scenario;
        final List<Map<String,Object>> rows=new ArrayList<>(),priorities=new ArrayList<>(),loops=new ArrayList<>(),actions=new ArrayList<>(),events=new ArrayList<>();
        final Map<String,Object> metadata=new LinkedHashMap<>();final Map<String,Card> observed=new LinkedHashMap<>();
        final java.util.Set<String> recorded=new java.util.HashSet<>();
        Card spell,manaSource;SpellAbility selected;Player payer;String spend="GGCCCC";boolean cast,intervened,done;Throwable eventFailure;
        Script(Game game,String scenario){this.game=game;this.scenario=scenario;actor=game.getPlayers().get(1);other=game.getPlayers().get(0);for(Player p:game.getPlayers())p.addController(game.getNextTimestamp(),p,new StrictController(game,p,this),false);Assert.assertFalse(actor.getController().isAI());Assert.assertFalse(other.getController().isAI());}
        boolean terminal(){return scenario.equals("zero")||scenario.equals("negative")||scenario.equals("both-zero");}
        boolean lifeOnly(){return terminal()||scenario.equals("forty")||scenario.equals("nonuniform");}
        String alias(Player p){if(p==null)return null;if(p==actor)return "A";if(p==other)return "B";throw new AssertionError("Unknown player");}
        Card current(Card c){return game.getCardState(c);}
        Card seedCard(String id,String name,Player owner,ZoneType zone){
            final String number=name.equals("Craw Wurm")?"239":name.equals("Rod of Ruin")?"312":"335";
            final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name,"8ED",number);Assert.assertNotNull(paper);
            final Card c=Card.fromPaperCard(paper,owner);c.setGameTimestamp(game.getNextTimestamp());owner.getZone(zone).add(c);Assert.assertFalse(c.isStartsGameInPlay());
            metadata.put(id,map("name",name,"owner",alias(owner),"zone",zone.name(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText(),"card_id",c.getId(),"starts_game_in_play",c.isStartsGameInPlay()));return c;
        }
        void seed(){
            if(scenario.equals("seeded")){
                observed.put("wurmA",seedCard("wurmA","Craw Wurm",actor,ZoneType.Battlefield));observed.put("rodA",seedCard("rodA","Rod of Ruin",actor,ZoneType.Battlefield));
                observed.put("wurmB",seedCard("wurmB","Craw Wurm",other,ZoneType.Battlefield));observed.put("rodB",seedCard("rodB","Rod of Ruin",other,ZoneType.Battlefield));
            }else if(!lifeOnly()){spell=seedCard("wurmA","Craw Wurm",actor,scenario.equals("cast")?ZoneType.Hand:ZoneType.Battlefield);observed.put("wurmA",spell);}
            if(scenario.equals("cast"))manaSource=seedCard("mana-source","Island",actor,ZoneType.Battlefield);
            for(int i=0;i<4;i++){seedCard("A-library-"+i,"Island",actor,ZoneType.Library);seedCard("B-library-"+i,"Island",other,ZoneType.Library);}
        }
        Map<String,Object> control(){return map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"priority",alias(game.getPhaseHandler().getPriorityPlayer()),"stack_size",game.getStack().size());}
        String outcome(){if(!game.isGameOver())return null;if(game.getOutcome().isDraw())return "draw";return actor.hasWon()?"winner:A":other.hasWon()?"winner:B":"unknown";}
        Map<String,Object> physical(Card input){final Card c=current(input);final boolean bf=c.isInPlay();return map("owner",alias(c.getOwner()),"controller",bf?alias(c.getController()):null,"zone",c.getZone().getZoneType().name(),"continuous",bf?!c.isFirstTurnControlled():null);}
        void record(String id,String protocol){
            Assert.assertTrue(recorded.add(id),"duplicate row "+id);final Map<String,Object> cards=new LinkedHashMap<>(),facts=new LinkedHashMap<>();
            for(Map.Entry<String,Card> entry:observed.entrySet()){final Card c=current(entry.getValue());cards.put(entry.getKey(),physical(c));facts.put(entry.getKey(),map("card_id",c.getId(),"zone_owner",alias(c.getZone().getPlayer()),"first_turn_controlled",c.isInPlay()?c.isFirstTurnControlled():null,"has_sickness",c.isInPlay()?c.hasSickness():null,"is_sick",c.isInPlay()?c.isSick():null,"tapped",c.isInPlay()?c.isTapped():null));}
            final Map<String,Object> v=control();v.putAll(map("id",id,"life",map("A",String.valueOf(actor.getLife()),"B",String.valueOf(other.getLife())),"starting_life",map("A",String.valueOf(actor.getStartingLife()),"B",String.valueOf(other.getStartingLife())),"cards",cards,"card_facts",facts,"outcome",outcome(),"game_over",game.isGameOver(),"losses",map("A",actor.getOutcome()==null||actor.getOutcome().lossState==null?null:actor.getOutcome().lossState.name(),"B",other.getOutcome()==null||other.getOutcome().lossState==null?null:other.getOutcome().lossState.name()),"callback_count",priorities.size(),"protocol",protocol));rows.add(v);
        }
        void change(Player next,String id){final Card c=current(spell);final String before=alias(c.getController()),zoneBefore=alias(c.getZone().getPlayer());final int cardId=c.getId();c.setController(next,game.getNextTimestamp());game.getAction().controllerChangeZoneCorrection(c);actions.add(map("kind","trusted-controller-and-zone-correction","from",before,"to",alias(next),"zone_owner_before",zoneBefore,"zone_owner_after",alias(c.getZone().getPlayer()),"card_id",cardId,"same_card_id",current(spell).getId()==cardId,"correction_called",true,"callback_ordinal",priorities.size()-1));record(id,"trusted-registry-control-in-priority-callback");}
        List<SpellAbility> choose(Player p){
            Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertFalse(p.getController().isAI());final Map<String,Object> point=control();point.putAll(map("ordinal",priorities.size(),"chooser",alias(p)));priorities.add(point);
            final int turn=game.getPhaseHandler().getTurn();final PhaseType phase=game.getPhaseHandler().getPhase();
            if(phase==PhaseType.UPKEEP){
                if(scenario.equals("seeded")&&p==game.getPhaseHandler().getPlayerTurn()){
                    if(turn==1&&!recorded.contains("seeded-A-upkeep"))record("seeded-A-upkeep","actual-upkeep-callback");
                    if(turn==2&&!recorded.contains("seeded-B-upkeep")){record("seeded-B-upkeep","actual-upkeep-callback");done=true;}
                }else if((scenario.equals("forty")||scenario.equals("nonuniform"))&&turn==1&&p==actor&&!done){record(scenario+"-A-upkeep","actual-upkeep-callback");done=true;}
                else if((scenario.equals("same")||scenario.equals("change")||scenario.equals("away"))&&turn==1&&p==actor&&!intervened){
                    record(scenario+"-before","actual-upkeep-callback");intervened=true;
                    if(scenario.equals("same")){change(actor,"same-after");done=true;}
                    else if(scenario.equals("change"))change(other,"change-to-B");
                    else{change(other,"away-to-B");change(actor,"away-back-to-A");}
                }else if((scenario.equals("change")||scenario.equals("away")||scenario.equals("cast"))&&turn==2&&p==other&&!recorded.contains(scenario+"-B-upkeep")){
                    record(scenario+"-B-upkeep","actual-upkeep-callback");if(scenario.equals("change"))done=true;
                }else if((scenario.equals("away")||scenario.equals("cast"))&&turn==3&&p==actor&&!done){record(scenario+"-next-A-upkeep","actual-upkeep-callback");done=true;}
            }
            if(scenario.equals("cast")&&turn==1&&phase==PhaseType.MAIN1&&p==actor){
                if(!cast){record("cast-before","actual-main-callback-before-admission");selected=current(spell).getFirstSpellAbility();selected.setActivatingPlayer(actor);payer=actor;for(char c:spend.toCharArray())actor.getManaPool().addMana(new Mana(ManaAtom.fromName(String.valueOf(c)),manaSource,null,actor));actions.add(map("kind","trusted-floating-seed","actor","A","colors",spend,"callback_ordinal",priorities.size()-1));Assert.assertFalse(selected.usesTargeting());Assert.assertTrue(selected.canPlay());return java.util.Collections.singletonList(selected);}
                if(game.getStack().isEmpty()&&current(spell).isInPlay()&&!recorded.contains("cast-resolved"))record("cast-resolved","actual-main-callback-after-resolution");
            }
            return null;
        }
        boolean play(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertTrue(sa.canPlay());Assert.assertTrue(current(spell).isInZone(ZoneType.Hand));final int before=game.getStack().size();Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,sa));Assert.assertEquals(game.getStack().size(),before+1);Assert.assertTrue(current(spell).isInZone(ZoneType.Stack));cast=true;actions.add(map("kind","actual-paid-cast","actor","A","before_stack",before,"after_stack",game.getStack().size(),"target_count",game.getStack().peekAbility().getTargets().size(),"callback_ordinal",priorities.size()-1));return true;}
        SpellAbility select(Player p,Card host,List<SpellAbility> options){Assert.assertSame(p,payer);Assert.assertEquals(options.size(),1);Assert.assertSame(options.get(0),selected);return selected;}
        boolean target(Player p,SpellAbility sa){throw new AssertionError("Unexpected target decision");}
        List<CostPart> order(Player p,List<CostPart> costs){Assert.assertSame(p,payer);for(CostPart c:costs)Assert.assertTrue(c instanceof CostPartMana);return costs;}
        boolean pay(Player p,ManaCost toPay,CostPartMana cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);actions.add(map("kind","mana-payment","actor",alias(p),"cost",toPay.toString()));return PlaySpellAbility.payManaCost(p.getController(),toPay,cost,sa,p,prompt,matrix,false);}
        boolean apply(Player p,ManaCostBeingPaid toPay,SpellAbility sa,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);p.pushPaidForSA(sa);sa.setManaCostBeingPaid(toPay);try{for(char c:spend.toCharArray())Assert.assertTrue(p.getManaPool().tryPayCostWithColor(ManaAtom.fromName(String.valueOf(c)),sa,toPay,sa.getPayingMana()));Assert.assertTrue(toPay.isPaid());return true;}finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}}
        void attackers(Player p,Combat combat){Assert.assertSame(p,game.getPhaseHandler().getPlayerTurn());Assert.assertTrue(combat.getAttackers().isEmpty());actions.add(map("kind","declare-no-attackers","actor",alias(p),"turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase().name()));}
        void blockers(Player p,Combat combat){throw new AssertionError("Unexpected blockers without attackers");}
        @Subscribe public void onEvent(GameEvent event){if(eventFailure!=null)return;try{
            if(event instanceof GameEventPlayerPriority e)events.add(map("kind","priority-event","priority",alias(game.getPlayer(e.priority().getId())),"step",e.phase().name()));
            if(event instanceof GameEventSpellResolved e)events.add(map("kind","resolved","fizzled",e.hasFizzled(),"step",game.getPhaseHandler().getPhase().name()));
            if(event instanceof GameEventCardChangeZone e&&spell!=null&&e.card().getId()==spell.getId())events.add(map("kind","zone","card_id",spell.getId(),"from",e.from()==null?null:e.from().zoneType().name(),"to",e.to()==null?null:e.to().zoneType().name(),"callback_count",priorities.size()));
        }catch(Throwable e){eventFailure=e;}}
    }
    private static final class StrictCosts extends CostDecisionMakerBase {
        private final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script) { super(player,ability.isTrigger(),ability,ability.getHostCard()); this.script=script; }
        @Override public boolean paysRightAfterDecision() {return true;}
        @Override public PaymentDecision visit(CostTap cost) {throw new AssertionError("Unexpected tap cost");}
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
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw new AssertionError("Unexpected nested resolution"); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.size()<=1); return activePlayerSAs; }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.isEmpty(), "Unexpected trigger"); }
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
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { throw new AssertionError("Unexpected discard"); }
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
