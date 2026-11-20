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


import forge.game.player.RegisteredPlayer;


import forge.StaticData;
import forge.MulliganDefs;
import forge.util.MyRandom;
import java.util.Random;
import java.util.HashSet;
import java.util.Set;

/** Actual opening-hand service and bounded shuffle branches with every choice explicit. */
public class OpeningHandReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256="45bb3fecd999a2820fafab5bc47a48adb27dbb7a3fd521d69d118de0566676f7";
    @Test public void initialSeven() throws IOException { run("initial-seven"); }
    @Test public void londonRepeated() throws IOException { run("london-repeated"); }
    @Test public void multiplayerFirstFree() throws IOException { run("multiplayer-free"); }
    @Test public void keepStopsFurtherMulligans() throws IOException { run("keep-stops"); }
    @Test public void zeroHandLimit() throws IOException { run("zero-hand-limit"); }
    @Test public void shuffleControlledPermutationCoverage() throws IOException { run("shuffle-controlled"); }
    private Game freshGame(int count) {
        final List<RegisteredPlayer> players=new ArrayList<>();
        for(int i=0;i<count;i++)players.add(new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-"+(char)('A'+i),null)));
        final GameRules rules=new GameRules(GameType.Constructed);
        return new Game(players,rules,new Match(rules,players,"Opening hand reference"));
    }
    private void run(String scenario) throws IOException {
        final Script script=new Script(freshGame(scenario.equals("multiplayer-free")?3:2),scenario);boolean completed=false;
        final Random oldRandom=MyRandom.getRandom();
        final MulliganDefs.MulliganRule oldRule=StaticData.instance().getMulliganRule();
        final boolean oldFiltered=StaticData.instance().getFilteredHandsEnabled();
        try {
            StaticData.instance().setMulliganRule(MulliganDefs.MulliganRule.London);StaticData.instance().setFilteredHandsEnabled(false);
            script.seed();script.game.subscribeToEvents(script);script.observe("before");script.execute();script.validate();completed=true;
        } finally {
            MyRandom.setRandom(oldRandom);StaticData.instance().setMulliganRule(oldRule);StaticData.instance().setFilteredHandsEnabled(oldFiltered);
            final Map<String,Object> trace=map("schema_version",1,"scenario",scenario,"reference","forge","commit",COMMIT,"expected_sha256",EXPECTED_SHA256,
                "completed",completed,"strict_choices",true,"ai_policy_used",false,"actual_start_game_executed",!scenario.equals("shuffle-controlled"),
                "actual_priority_loop_executed",false,"start_hook_stop_used",!scenario.equals("shuffle-controlled"),"filtered_hands",false,
                "setup","Fresh Game, twelve prepared basic-land objects per seat (deliberately undersized), actual Player.shuffle before GameAction.startGame; strict controllers, explicit starting seat. Actual first-turn hook throws a sentinel before mainGameLoop.",
                "protocol_limit","MulliganService sequentially redraws each player before later declarations; CR simultaneous declaration protocol is not qualified. Zero-hand eighth prompt is retained as known source nonconformance.",
                "snapshots",script.snapshots,"choices",script.choices,"shuffles",script.shuffles,"events",script.events,"metadata",script.metadata,
                "random_words",script.randomWords,"shuffle_samples",script.samples,"stopped_at_first_turn",script.stopped,"event_failure",script.eventFailure==null?null:script.eventFailure.toString());
            final String directory=System.getProperty("spellwake.evidence.dir");Assert.assertNotNull(directory);
            Files.write(Paths.get(directory,scenario+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario="+scenario+" completed="+completed+" snapshots="+script.snapshots.size()+" choices="+script.choices.size());
        }
    }
    private static Map<String,Object> map(Object... pairs) {final Map<String,Object> values=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)values.put((String)pairs[i],pairs[i+1]);return values;}
    private static final class OpeningStop extends RuntimeException { private static final long serialVersionUID=1L; }
    /** Raw words are recorded below Forge's own CountingRandom adapter. */
    private static final class RecordedRandom extends Random {
        private static final long serialVersionUID=1L;
        private final Random delegate=new Random(0x5350454c4c57414bL);
        private final Script script;
        private final int[] controlled;
        private int index;
        RecordedRandom(Script script,int[] controlled) {super(0);this.script=script;this.controlled=controlled;}
        @Override public int nextInt() {
            if(controlled!=null&&index>=controlled.length)throw new AssertionError("Controlled shuffle consumed excess entropy");
            final int value=controlled==null?delegate.nextInt():controlled[index];index++;
            script.randomWords.add(map("ordinal",script.randomWords.size(),"raw_word",value));return value;
        }
        void exhausted(){if(controlled!=null)Assert.assertEquals(index,controlled.length);}
    }
    public static final class Script {
        final Game game;final Player actor;final String scenario;
        final Map<Integer,String> cardAliases=new LinkedHashMap<>();final Map<String,Card> cards=new LinkedHashMap<>();
        final Map<String,Object> metadata=new LinkedHashMap<>();final Map<String,Integer> keepCounts=new LinkedHashMap<>(),bottomCounts=new LinkedHashMap<>();
        final List<Map<String,Object>> snapshots=new ArrayList<>(),choices=new ArrayList<>(),shuffles=new ArrayList<>(),events=new ArrayList<>(),randomWords=new ArrayList<>(),samples=new ArrayList<>();
        Throwable eventFailure;boolean stopped;
        Script(Game game,String scenario){this.game=game;this.scenario=scenario;actor=game.getPlayers().get(0);
            for(Player p:game.getPlayers()){p.addController(game.getNextTimestamp(),p,new StrictController(game,p,this),false);Assert.assertFalse(p.getController().isAI());keepCounts.put(alias(p),0);bottomCounts.put(alias(p),0);}}
        String alias(Player p){return String.valueOf((char)('A'+p.getId()));}
        String cardAlias(Card c){final String id=cardAliases.get(c.getId());Assert.assertNotNull(id);return id;}
        Card current(String id){for(Player p:game.getRegisteredPlayers())for(ZoneType z:List.of(ZoneType.Library,ZoneType.Hand))for(Card c:p.getCardsIn(z))if(c.getId()==cards.get(id).getId())return c;throw new AssertionError("Missing canonical object "+id);}
        void seed(){
            for(Player p:game.getPlayers())for(int i=0;i<(scenario.equals("shuffle-controlled")?(p==actor?3:0):12);i++){
                final String id=alias(p)+String.format("%02d",i);final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard("Island");Assert.assertNotNull(paper);
                final Card card=Card.fromPaperCard(paper,p);card.setGameTimestamp(game.getNextTimestamp());p.getZone(ZoneType.Library).add(card);cards.put(id,card);cardAliases.put(card.getId(),id);
                metadata.put(id,map("name","Island","owner",alias(p),"physical_id",card.getId(),"initial_timestamp",card.getGameTimestamp(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber()));
            }
        }
        List<String> order(Player p,ZoneType zone){final List<String> ids=new ArrayList<>();for(Card c:p.getCardsIn(zone))ids.add(cardAlias(c));return ids;}
        Map<String,Object> state(){
            final Map<String,Object> players=new LinkedHashMap<>(),objects=new LinkedHashMap<>();
            for(Player p:game.getRegisteredPlayers())players.put(alias(p),map("library",order(p,ZoneType.Library),"hand",order(p,ZoneType.Hand),"lost",p.hasLost(),"drawn_this_turn",p.getNumDrawnThisTurn()));
            for(String id:cards.keySet()){final Card c=current(id);objects.put(id,map("physical_id",c.getId(),"timestamp",c.getGameTimestamp(),"zone",c.getZone().getZoneType().name(),"owner",alias(c.getOwner())));}
            return map("age",game.getAge().name(),"turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"stack_size",game.getStack().size(),"game_over",game.isGameOver(),"players",players,"objects",objects);
        }
        void observe(String label){snapshots.add(map("label",label,"state",state()));}
        int target(Player p){final String seat=alias(p);
            if(scenario.equals("london-repeated"))return seat.equals("A")?2:0;
            if(scenario.equals("multiplayer-free"))return seat.equals("A")?2:seat.equals("B")?1:0;
            if(scenario.equals("keep-stops"))return seat.equals("B")?2:0;
            if(scenario.equals("zero-hand-limit"))return seat.equals("A")?7:0;
            return 0;
        }
        Player startingPlayer(Player chooser,boolean firstGame){Assert.assertTrue(firstGame);choices.add(map("ordinal",choices.size(),"kind","starting-player","chooser",alias(chooser),"selected","A","state",state()));return actor;}
        boolean keep(Player p,Player first,int charged){
            Assert.assertSame(first,actor);Assert.assertEquals(game.getAge(),GameStage.Mulligan);final int seen=keepCounts.get(alias(p));final boolean keep=seen>=target(p);
            keepCounts.put(alias(p),seen+1);choices.add(map("ordinal",choices.size(),"kind","keep","player",alias(p),"charged",charged,"keep",keep,"state",state()));return keep;
        }
        CardCollectionView bottom(Player p,CardCollectionView hand,int count){
            Assert.assertEquals(hand.size(),7);Assert.assertEquals(game.getAge(),GameStage.Mulligan);final CardCollection selected=new CardCollection();
            for(int i=0;i<count;i++)selected.add(hand.get(hand.size()-1-i));
            final List<String> ids=new ArrayList<>();for(Card c:selected)ids.add(cardAlias(c));
            bottomCounts.put(alias(p),bottomCounts.get(alias(p))+1);choices.add(map("ordinal",choices.size(),"kind","bottom","player",alias(p),"count",count,"selected",ids,"state",state()));return selected;
        }
        void shuffled(Player p,CardCollectionView list){final List<String> ids=new ArrayList<>();for(Card c:list)ids.add(cardAlias(c));shuffles.add(map("ordinal",shuffles.size(),"player",alias(p),"randomized",ids,"state_before_commit",state(),"raw_word_count",randomWords.size(),"cheat_hook_modified",false));}
        @Subscribe public void onEvent(GameEvent event){if(eventFailure!=null)return;try{
            if(event instanceof GameEventShuffle)events.add(map("ordinal",events.size(),"kind","shuffle","state",state()));
            if(event instanceof GameEventMulligan)events.add(map("ordinal",events.size(),"kind","mulligan","state",state()));
            if(event instanceof GameEventCardChangeZone moved)events.add(map("ordinal",events.size(),"kind","zone","card",cardAliases.get(moved.card().getId()),"from",moved.from()==null?null:moved.from().zoneType().name(),"to",moved.to()==null?null:moved.to().zoneType().name(),"state",state()));
        }catch(Throwable failure){eventFailure=failure;}}
        void execute(){
            if(scenario.equals("shuffle-controlled")){shuffleSamples();return;}
            final RecordedRandom random=new RecordedRandom(this,null);MyRandom.setRandom(random);
            for(Player p:game.getPlayers())p.shuffle(null);observe("initial-shuffles-completed");
            try {game.getAction().startGame(null,()->{observe("first-turn-hook");stopped=true;throw new OpeningStop();});throw new AssertionError("Expected controlled opening stop");}
            catch(OpeningStop stop){Assert.assertTrue(stopped);}
        }
        void shuffleSamples(){
            final CardCollection original=new CardCollection(actor.getCardsIn(ZoneType.Library));
            for(int a=0;a<3;a++)for(int b=0;b<2;b++){
                actor.getZone(ZoneType.Library).setCards(original);final RecordedRandom random=new RecordedRandom(this,new int[]{a<<1,b<<31});MyRandom.setRandom(random);
                final Map<String,Object> before=state();actor.shuffle(null);random.exhausted();samples.add(map("kind","three-card","branch",List.of(a,b),"before",before,"after",state()));
            }
            for(int size=0;size<=1;size++){
                final CardCollection small=new CardCollection();if(size==1)small.add(original.get(0));actor.getZone(ZoneType.Library).setCards(small);
                final CardCollection held=new CardCollection();for(Card c:original)if(!small.contains(c))held.add(c);actor.getZone(ZoneType.Hand).setCards(held);
                final RecordedRandom random=new RecordedRandom(this,new int[]{});MyRandom.setRandom(random);final Map<String,Object> before=state();final int eventCount=events.size();
                actor.shuffle(null);random.exhausted();samples.add(map("kind","small","size",size,"before",before,"after",state(),"new_events",events.size()-eventCount));
            }
            observe("shuffle-samples-completed");
        }
        void validate(){
            if(eventFailure!=null)throw new AssertionError("Observer failure",eventFailure);
            if(scenario.equals("shuffle-controlled")){
                final Set<Object> permutations=new HashSet<>();for(Map<String,Object> s:samples)if(s.get("kind").equals("three-card"))permutations.add(((Map<?,?>)((Map<?,?>)s.get("after")).get("players")).get("A"));Assert.assertEquals(permutations.size(),6);return;
            }
            Assert.assertTrue(stopped);Assert.assertEquals(game.getAge(),GameStage.Play);
            for(Player p:game.getPlayers()){
                final int charged=Math.max(0,target(p)-(game.getPlayers().size()>2?1:0));Assert.assertEquals(p.getCardsIn(ZoneType.Hand).size(),7-charged);
                Assert.assertEquals(bottomCounts.get(alias(p)).intValue(),target(p));
                // Pinned Forge offers one extra keep prompt at zero; retain this source discrepancy.
                Assert.assertEquals(keepCounts.get(alias(p)).intValue(),target(p)+1);
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
        @Override public CardCollectionView cheatShuffle(CardCollectionView cards) { script.shuffled(getPlayer(), cards); return cards; }
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
        @Override public void declareAttackers(Player attacker, Combat combat) { throw new AssertionError("Unexpected combat before opening setup completed"); }
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
        @Override public Player chooseStartingPlayer(boolean isFirstGame) { return script.startingPlayer(getPlayer(), isFirstGame); }
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
        @Override public boolean mulliganKeepHand(Player player, int cardsToReturn) { return script.keep(getPlayer(), player, cardsToReturn); }
        @Override public CardCollectionView tuckCardsViaMulligan(CardCollectionView hand, int cardsToReturn) { return script.bottom(getPlayer(), hand, cardsToReturn); }
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { throw new AssertionError("Gameplay priority before controlled opening stop"); }
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
