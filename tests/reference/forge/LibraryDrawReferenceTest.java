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


import com.google.common.eventbus.Subscribe;


import forge.game.phase.PhaseType;
import forge.game.player.RegisteredPlayer;


/** Strict ordered-library/draw primitives and real scheduled draw-entry probes. */
public class LibraryDrawReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256="e398d09c0e2c7f2c3a30265783eca5e9f0dcbb46ac95b171a9c8c28ba425a21c";
    @Test public void orderedSingleDraw() throws IOException { run("ordered-single"); }
    @Test public void orderedMultipleDraw() throws IOException { run("ordered-multiple"); }
    @Test public void lastCardSafe() throws IOException { run("last-card-safe"); }
    @Test public void emptyDrawDeferredLoss() throws IOException { run("empty-draw-deferred"); }
    @Test public void partialBatchDeferredLoss() throws IOException { run("partial-batch-deferred"); }
    @Test public void firstDrawTwoPlayer() throws IOException { run("first-draw-two-player"); }
    @Test public void firstDrawMultiplayer() throws IOException { run("first-draw-multiplayer"); }
    @Test public void libraryPositioning() throws IOException { run("library-positioning"); }
    private Game freshGame(int count) {
        final List<RegisteredPlayer> players=new ArrayList<>();
        for(int i=0;i<count;i++)players.add(new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-"+(char)('A'+i),null)));
        final GameRules rules=new GameRules(GameType.Constructed);
        final Game game=new Game(players,rules,new Match(rules,players,"Library draw reference"));
        game.setAge(GameStage.Play);return game;
    }
    private void run(String scenario) throws IOException {
        final Script script=new Script(freshGame(scenario.equals("first-draw-multiplayer")?3:2),scenario);boolean completed=false;
        try {script.seed();script.game.subscribeToEvents(script);script.observe("before");script.execute();script.validate();completed=true;}
        finally {
            final Map<String,Object> trace=map("schema_version",1,"scenario",scenario,"reference","forge","commit",COMMIT,
                "expected_sha256",EXPECTED_SHA256,"completed",completed,"strict_choices",true,"ai_policy_used",false,
                "priority_loop_executed",scenario.startsWith("first-draw-"),"dev_phase_advance_used",false,
                "setup","Fresh Game in Play age, prepared explicit top-to-bottom ordered libraries, strict controllers replacing lobby controllers before any operation. No opening-hand setup, shuffle or mulligan.",
                "primitive_scope","Direct draw cases call actual Player.drawCards, then actual GameAction.checkStateEffects at separately observed boundaries. Position insertions call GameAction; exact same-library reorder calls Zone.reorder, not an effect.",
                "snapshots",script.snapshots,"zone_events",script.events,"priorities",script.priorities,"phase_events",script.phases,
                "actions",script.actions,"loops",script.loops,"metadata",script.metadata,"returned_draw_order",script.returned);
            final String directory=System.getProperty("spellwake.evidence.dir");Assert.assertNotNull(directory);
            Files.write(Paths.get(directory,scenario+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),
                java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario="+scenario+" completed="+completed+" snapshots="+script.snapshots.size());
        }
    }
    private static Map<String,Object> map(Object... pairs) {final Map<String,Object> values=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)values.put((String)pairs[i],pairs[i+1]);return values;}
    public static final class Script {
        final Game game;final Player actor,other;final String scenario;
        final Map<Integer,String> cardAliases=new LinkedHashMap<>();final Map<String,Card> cards=new LinkedHashMap<>();
        final Map<String,Object> metadata=new LinkedHashMap<>();final List<String> returned=new ArrayList<>();
        final List<Map<String,Object>> snapshots=new ArrayList<>(),events=new ArrayList<>(),priorities=new ArrayList<>(),phases=new ArrayList<>(),actions=new ArrayList<>(),loops=new ArrayList<>();
        Throwable eventFailure;
        Script(Game game,String scenario){this.game=game;this.scenario=scenario;actor=game.getPlayers().get(0);other=game.getPlayers().get(1);
            for(Player p:game.getPlayers()){p.addController(game.getNextTimestamp(),p,new StrictController(game,p,this),false);Assert.assertFalse(p.getController().isAI());}}
        String alias(Player p){return String.valueOf((char)('A'+p.getId()));}
        String cardAlias(Card c){final String id=cardAliases.get(c.getId());Assert.assertNotNull(id);return id;}
        Card current(String id){
            final int physicalId=cards.get(id).getId();
            for(Player p:game.getRegisteredPlayers())for(ZoneType zone:List.of(ZoneType.Library,ZoneType.Hand))for(Card c:p.getCardsIn(zone))if(c.getId()==physicalId)return c;
            throw new AssertionError("Missing canonical zone occupant: "+id);
        }
        Card seedCard(String id,String name,Player owner,ZoneType zone){
            final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name);Assert.assertNotNull(paper);
            final Card card=Card.fromPaperCard(paper,owner);card.setGameTimestamp(game.getNextTimestamp());owner.getZone(zone).add(card);
            cards.put(id,card);cardAliases.put(card.getId(),id);metadata.put(id,map("name",name,"owner",alias(owner),"initial_zone",zone.name(),"physical_id",card.getId(),"initial_timestamp",card.getGameTimestamp(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber()));return card;
        }
        void seed(){
            if(scenario.startsWith("first-draw-")){
                seedCard("a","Island",actor,ZoneType.Library);seedCard("b","Mountain",actor,ZoneType.Library);
                seedCard("c","Forest",other,ZoneType.Library);seedCard("d","Plains",other,ZoneType.Library);
                if(game.getPlayers().size()==3){seedCard("e","Swamp",game.getPlayers().get(2),ZoneType.Library);seedCard("f","Island",game.getPlayers().get(2),ZoneType.Library);}
            }else{
                if(!scenario.equals("empty-draw-deferred"))seedCard("a","Island",actor,ZoneType.Library);
                if(scenario.equals("ordered-single")||scenario.equals("ordered-multiple")||scenario.equals("library-positioning")){
                    seedCard("b","Mountain",actor,ZoneType.Library);seedCard("c","Forest",actor,ZoneType.Library);}
                if(scenario.equals("library-positioning")){seedCard("d","Plains",actor,ZoneType.Hand);seedCard("e","Swamp",actor,ZoneType.Hand);}
                seedCard("z","Island",other,ZoneType.Library);
            }
        }
        List<String> order(Player p,ZoneType zone){final List<String> values=new ArrayList<>();for(Card c:p.getCardsIn(zone))values.add(cardAlias(c));return values;}
        Map<String,Object> state(){final Map<String,Object> players=new LinkedHashMap<>();
            for(Player p:game.getRegisteredPlayers())players.put(alias(p),map("library",order(p,ZoneType.Library),"hand",order(p,ZoneType.Hand),"lost",p.hasLost(),"won",p.hasWon(),"loss_reason",p.getOutcome()==null||p.getOutcome().lossState==null?null:p.getOutcome().lossState.name(),"drawn_this_turn",p.getNumDrawnThisTurn()));
            final Map<String,Object> objects=new LinkedHashMap<>(),lookupObjects=new LinkedHashMap<>();for(String id:cards.keySet()){Card c=current(id);objects.put(id,map("physical_id",c.getId(),"timestamp",c.getGameTimestamp(),"zone",c.getZone().getZoneType().name()));Card lookup=game.getCardState(cards.get(id));lookupObjects.put(id,map("physical_id",lookup.getId(),"timestamp",lookup.getGameTimestamp(),"zone",lookup.getZone().getZoneType().name()));}
            return map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",game.getPhaseHandler().getPlayerTurn()==null?null:alias(game.getPhaseHandler().getPlayerTurn()),"game_over",game.isGameOver(),"stack_size",game.getStack().size(),"players",players,"objects",objects,"lookup_objects",lookupObjects);
        }
        void observe(String label){snapshots.add(map("label",label,"state",state()));}
        void execute(){
            if(scenario.startsWith("first-draw-")){scheduled();return;}
            if(scenario.equals("library-positioning")){
                final long d=current("d").getGameTimestamp(),e=current("e").getGameTimestamp();
                game.getAction().moveToLibrary(current("d"),null);observe("top-inserted");Assert.assertNotEquals(current("d").getGameTimestamp(),d);
                game.getAction().moveToBottomOfLibrary(current("e"),null);observe("bottom-inserted");Assert.assertNotEquals(current("e").getGameTimestamp(),e);
                final Card c=current("c");final long stamp=c.getGameTimestamp();
                actor.getZone(ZoneType.Library).reorder(c,0);observe("same-library-top");Assert.assertSame(current("c"),c);Assert.assertEquals(c.getGameTimestamp(),stamp);
                actor.getZone(ZoneType.Library).reorder(c,actor.getCardsIn(ZoneType.Library).size()-1);observe("same-library-bottom");Assert.assertSame(current("c"),c);Assert.assertEquals(c.getGameTimestamp(),stamp);return;
            }
            final int count=scenario.equals("ordered-multiple")?2:scenario.equals("partial-batch-deferred")?3:1;
            actions.add(map("kind","actual-drawCards-call","count",count));
            for(Card c:actor.drawCards(count))returned.add(cardAlias(c));observe("after-draw-before-sba");
            Assert.assertFalse(actor.hasLost());Assert.assertFalse(game.isGameOver());
            game.getAction().checkStateEffects(true);observe("after-sba");
        }
        void scheduled(){
            game.getPhaseHandler().setupFirstTurn(actor,null);observe("first-untap-entered");
            for(int i=0;i<120;i++){
                Assert.assertFalse(game.isGameOver());final Map<String,Object> before=state();
                game.getPhaseHandler().mainLoopStep();loops.add(map("ordinal",i,"before",before,"after",state()));
                if(eventFailure!=null)throw new AssertionError("Event observer failure",eventFailure);
                if(scenario.equals("first-draw-two-player")&&game.getPhaseHandler().getTurn()==1&&game.getPhaseHandler().getPhase()==PhaseType.MAIN1&&priorities.stream().anyMatch(p->p.get("turn").equals(1)&&p.get("step").equals("MAIN1"))){
                    if(snapshots.stream().noneMatch(p->p.get("label").equals("A-first-main-priority")))observe("A-first-main-priority");}
                final int expectedTurn=scenario.equals("first-draw-two-player")?2:1;
                if(priorities.stream().anyMatch(p->p.get("turn").equals(expectedTurn)&&p.get("step").equals("DRAW"))){observe("draw-priority");return;}
            }throw new AssertionError("Scheduler bound exhausted");
        }
        List<SpellAbility> choose(Player p){priorities.add(map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase().name(),"player",alias(p),"state",state()));return null;}
        @Subscribe public void onEvent(GameEvent event){if(eventFailure!=null)return;try{
            if(event instanceof GameEventCardChangeZone moved)events.add(map("ordinal",events.size(),"card",cardAliases.get(moved.card().getId()),"physical_id",moved.card().getId(),"from",moved.from()==null?null:moved.from().zoneType().name(),"to",moved.to()==null?null:moved.to().zoneType().name(),"state",state()));
            if(event instanceof GameEventTurnPhase phase)phases.add(map("turn",game.getPhaseHandler().getTurn(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"step",phase.phase().name()));
        }catch(Throwable failure){eventFailure=failure;}}
        void validate(){if(eventFailure!=null)throw new AssertionError("Event observer failure",eventFailure);
            switch(scenario){
                case "ordered-single":Assert.assertEquals(returned,List.of("a"));Assert.assertEquals(order(actor,ZoneType.Library),List.of("b","c"));break;
                case "ordered-multiple":Assert.assertEquals(returned,List.of("a","b"));Assert.assertEquals(order(actor,ZoneType.Library),List.of("c"));break;
                case "last-card-safe":Assert.assertEquals(returned,List.of("a"));Assert.assertFalse(actor.hasLost());Assert.assertFalse(game.isGameOver());break;
                case "empty-draw-deferred":Assert.assertTrue(returned.isEmpty());Assert.assertTrue(actor.hasLost());Assert.assertEquals(actor.getOutcome().lossState.name(),"Milled");Assert.assertTrue(game.isGameOver());Assert.assertTrue(other.hasWon());break;
                case "partial-batch-deferred":Assert.assertEquals(returned,List.of("a"));Assert.assertTrue(actor.hasLost());Assert.assertEquals(actor.getOutcome().lossState.name(),"Milled");Assert.assertTrue(game.isGameOver());Assert.assertTrue(other.hasWon());break;
                case "first-draw-two-player":Assert.assertTrue(order(actor,ZoneType.Hand).isEmpty());Assert.assertEquals(order(actor,ZoneType.Library),List.of("a","b"));Assert.assertEquals(order(other,ZoneType.Hand),List.of("c"));Assert.assertEquals(order(other,ZoneType.Library),List.of("d"));Assert.assertTrue(priorities.stream().noneMatch(p->p.get("turn").equals(1)&&p.get("step").equals("DRAW")));break;
                case "first-draw-multiplayer":Assert.assertEquals(order(actor,ZoneType.Hand),List.of("a"));Assert.assertEquals(order(actor,ZoneType.Library),List.of("b"));break;
                case "library-positioning":Assert.assertEquals(order(actor,ZoneType.Library),List.of("d","a","b","e","c"));break;
                default:throw new AssertionError("Unknown scenario");
            }
        }
    }
    /** Pinned controller surface. Every choice is explicit and all other decisions throw. */
    private static final class StrictController extends PlayerController {
        private final Script script;
        StrictController(Game game, Player player, Script script) {
            super(game, player, player.getLobbyPlayer());
            this.script = script;
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { throw new AssertionError("Unexpected ability choice"); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw new AssertionError("Unexpected no-stack effect"); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.isEmpty()); return activePlayerSAs; }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.isEmpty()); }
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
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { throw new AssertionError("Unexpected target choice"); }
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
        @Override public void declareAttackers(Player attacker, Combat combat) { Assert.assertTrue(combat.getAttackers().isEmpty()); script.actions.add(map("kind","declare-no-attackers","player",script.alias(getPlayer()))); }
        @Override public void declareBlockers(Player defender, Combat combat) { throw new AssertionError("Unexpected blocker choice"); }
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
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { throw new AssertionError("Unexpected discard choice"); }
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
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { throw new AssertionError("Unexpected chosen spell"); }
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
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { throw new AssertionError("Unexpected cost ordering"); }
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { throw new AssertionError("Unexpected controller operation: payCostToPreventEffect"); }
        @Override public boolean payCostDuringRoll(Cost cost, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: payCostDuringRoll"); }
        @Override public boolean payCombatCost(Card card, Cost cost, SpellAbility sa, String prompt) { throw new AssertionError("Unexpected controller operation: payCombatCost"); }
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { throw new AssertionError("Unexpected mana cost"); }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { throw new AssertionError("Unexpected mana application"); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw new AssertionError("Unexpected controller operation: chooseCardsForCost"); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { throw new AssertionError("Unexpected cost decision maker"); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw new AssertionError("Unexpected controller operation: chooseSingleCardForZoneChange"); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw new AssertionError("Unexpected controller operation: chooseCardsForZoneChange"); }
        @Override public void autoPassCancel() { /* notification only; no player decision */ }
        @Override public void awaitNextInput() { /* notification only; no player decision */ }
        @Override public void cancelAwaitNextInput() { /* notification only; no player decision */ }
    }
}
