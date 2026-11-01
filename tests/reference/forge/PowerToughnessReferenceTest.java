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

/** Original Spellwake PT reference fixture. It uses real definitions, direct stack
 * admission, strict scripted targets and explicit post-resolution state checks.
 * Casting, payment, ordinary priority and turn-duration expiration are bypassed.
 */
public class PowerToughnessReferenceTest extends AITest {
    private static final String COMMIT = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256 = "d01c1aaf17dda41143c3f2e8fe98d2c88cf3f38df6e3986906ab9b8ede62072d";

    @Test public void setAfterBoost() throws IOException { run("set-after-boost"); }
    @Test public void switchThenAsymmetricBoost() throws IOException { run("switch-then-asymmetric-boost"); }
    @Test public void counterThenSwitch() throws IOException { run("counter-then-switch"); }

    private void run(String scenario) throws IOException {
        final Game game = initAndCreateGame();
        final Player actor = game.getPlayers().get(1);
        final Player opponent = game.getPlayers().get(0);
        final Map<String, Card> cards = new LinkedHashMap<>();
        final Card warden = seed("Lumengrid Warden", "MRD", "41", actor, ZoneType.Battlefield);
        cards.put("warden", warden);
        Assert.assertEquals(warden.getPaperCard().getRules().getPower(), "1");
        Assert.assertEquals(warden.getPaperCard().getRules().getToughness(), "3");
        Assert.assertTrue(warden.getPaperCard().getRules().getOracleText().isEmpty(), "vanilla creature required");
        if (scenario.equals("set-after-boost")) {
            cards.put("growth", seed("Giant Growth", "M11", "178", actor, ZoneType.Hand));
            cards.put("diminish", seed("Diminish", "M11", "52", actor, ZoneType.Hand));
        } else if (scenario.equals("switch-then-asymmetric-boost")) {
            cards.put("switch", seed("About Face", "ULG", "73", actor, ZoneType.Hand));
            cards.put("fists", seed("Fists of the Anvil", "MRD", "91", actor, ZoneType.Hand));
        } else if (scenario.equals("counter-then-switch")) {
            cards.put("counter", seed("Battlegrowth", "MRD", "113", actor, ZoneType.Hand));
            cards.put("switch", seed("About Face", "ULG", "73", actor, ZoneType.Hand));
        } else throw new AssertionError("Unknown scenario " + scenario);
        final StrictController actorController = new StrictController(game, actor,
                scenario.equals("counter-then-switch") ? 1 : 0, warden.getId());
        final StrictController opponentController = new StrictController(game, opponent, 0, warden.getId());
        actor.addController(game.getNextTimestamp(), actor, actorController, false);
        opponent.addController(game.getNextTimestamp(), opponent, opponentController, false);
        game.getAction().checkStaticAbilities();
        game.getAction().checkStateEffects(true);
        final List<Map<String, Object>> observations = new ArrayList<>();
        observe(game, cards, observations, "initial", 1, 3, 0);
        if (scenario.equals("set-after-boost")) {
            admitResolve(game, actor, cards.get("growth"), warden);
            observe(game, cards, observations, "after-growth", 4, 6, 0, "growth");
            admitResolve(game, actor, cards.get("diminish"), warden);
            observe(game, cards, observations, "after-diminish", 4, 4, 0, "growth", "diminish");
        } else if (scenario.equals("switch-then-asymmetric-boost")) {
            admitResolve(game, actor, cards.get("switch"), warden);
            observe(game, cards, observations, "after-switch", 3, 1, 0, "switch");
            admitResolve(game, actor, cards.get("fists"), warden);
            observe(game, cards, observations, "after-fists", 3, 5, 0, "switch", "fists");
        } else {
            admitResolve(game, actor, cards.get("counter"), warden);
            observe(game, cards, observations, "after-counter", 2, 4, 1, "counter");
            admitResolve(game, actor, cards.get("switch"), warden);
            observe(game, cards, observations, "after-switch", 4, 2, 1, "counter", "switch");
        }
        actorController.assertConsumed();
        opponentController.assertConsumed();
        final List<Map<String, Object>> metadata = new ArrayList<>();
        for (Map.Entry<String, Card> entry : cards.entrySet()) {
            final Card card = game.getCardState(entry.getValue());
            final IPaperCard paper = card.getPaperCard();
            metadata.add(map("alias", entry.getKey(), "name", card.getName(),
                    "edition", paper.getEdition(), "collector_number", paper.getCollectorNumber(),
                    "oracle", paper.getRules().getOracleText(), "types", paper.getRules().getType().toString(),
                    "mana_cost", paper.getRules().getManaCost().toString(),
                    "defined_power", paper.getRules().getPower(),
                    "defined_toughness", paper.getRules().getToughness(),
                    "actual_class", card.getClass().getName()));
        }
        final Map<String, Object> output = map("schema_version", 1, "scenario", scenario,
                "reference", "forge", "commit", COMMIT, "strict_choices", true,
                "mana_auto_payment", false, "direct_stack_admission", true,
                "casting_scope", "direct-stack-admission-no-payment",
                "observation_scope", "initial-and-settled-resolution-after-state-checks",
                "duration_expiration_tested", false, "expected_sha256", EXPECTED_SHA256,
                "singleton_counter_type_callbacks", actorController.usedCounterTypeCalls,
                "card_metadata", metadata, "observations", observations);
        final String directory = System.getProperty("spellwake.evidence.dir");
        Assert.assertNotNull(directory, "explicit evidence destination required");
        Files.write(Paths.get(directory, scenario + ".json"),
                (new GsonBuilder().serializeNulls().create().toJson(output) + "\n").getBytes(StandardCharsets.UTF_8),
                java.nio.file.StandardOpenOption.CREATE_NEW, java.nio.file.StandardOpenOption.WRITE);
        System.out.println("RESEARCH scenario=" + scenario + " checkpoints=" + observations.size());
    }

    private Card seed(String name, String edition, String number, Player player, ZoneType zone) {
        final IPaperCard paper = FModel.getMagicDb().getCommonCards().getCard(name, edition, number);
        Assert.assertNotNull(paper, "missing exact printing: " + name + " " + edition + " " + number);
        Assert.assertEquals(paper.getEdition(), edition);
        Assert.assertEquals(paper.getCollectorNumber(), number);
        final Card card = Card.fromPaperCard(paper, player);
        card.setGameTimestamp(player.getGame().getNextTimestamp());
        player.getZone(zone).add(card);
        return card;
    }

    private void admitResolve(Game game, Player actor, Card original, Card target) {
        Assert.assertTrue(game.getStack().isEmpty());
        final Card card = game.getCardState(original);
        final Card recipient = game.getCardState(target);
        Assert.assertEquals(card.getZone().getZoneType(), ZoneType.Hand);
        Assert.assertEquals(recipient.getZone().getZoneType(), ZoneType.Battlefield);
        Assert.assertEquals(card.getSpellAbilities().size(), 1, "exactly one printed spell program required");
        final SpellAbility ability = card.getSpellAbilities().get(0);
        ability.setActivatingPlayer(actor);
        Assert.assertTrue(ability.canTarget(recipient), "explicit scripted target must be legal");
        ability.getTargets().add(recipient);
        game.getStack().addAndUnfreeze(ability);
        Assert.assertEquals(game.getStack().size(), 1);
        Assert.assertEquals(ability.getHostCard().getZone().getZoneType(), ZoneType.Stack);
        Assert.assertEquals(game.getStack().peekAbility().getTargets().size(), 1);
        Assert.assertEquals(game.getStack().peekAbility().getTargets().getFirstTargetedCard().getId(), recipient.getId());
        game.getStack().resolveStack();
        game.getAction().checkStateEffects(true);
        Assert.assertFalse(game.isGameOver(), "unexpected seeded game outcome");
        Assert.assertTrue(game.getStack().isEmpty());
        Assert.assertEquals(game.getCardState(original).getZone().getZoneType(), ZoneType.Graveyard);
    }

    private void observe(Game game, Map<String, Card> cards, List<Map<String, Object>> observations,
            String checkpoint, int expectedPower, int expectedToughness, int expectedPlus, String... exited) {
        final Card creature = game.getCardState(cards.get("warden"));
        Assert.assertEquals(creature.getZone().getZoneType(), ZoneType.Battlefield);
        Assert.assertEquals(creature.getNetPower(), expectedPower, checkpoint + " power");
        Assert.assertEquals(creature.getNetToughness(), expectedToughness, checkpoint + " toughness");
        Assert.assertEquals(creature.getCounters(CounterEnumType.P1P1), expectedPlus);
        Assert.assertEquals(creature.getCounters(CounterEnumType.M1M1), 0);
        Assert.assertEquals(creature.getNumAllCounters(), expectedPlus, "no unexpected counter kind");
        Assert.assertEquals(creature.getDamage(), 0);
        Assert.assertTrue(game.getStack().isEmpty());
        final List<String> exitedAliases = java.util.Arrays.asList(exited);
        final Map<String, Object> zones = new LinkedHashMap<>();
        final Map<String, Object> exits = new LinkedHashMap<>();
        for (Map.Entry<String, Card> entry : cards.entrySet()) {
            if (entry.getKey().equals("warden")) continue;
            final Card source = game.getCardState(entry.getValue());
            final ZoneType expected = exitedAliases.contains(entry.getKey()) ? ZoneType.Graveyard : ZoneType.Hand;
            Assert.assertEquals(source.getZone().getZoneType(), expected);
            zones.put(entry.getKey(), source.getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT));
            exits.put(entry.getKey(), source.getZone().getZoneType() == ZoneType.Graveyard);
        }
        observations.add(map("checkpoint", checkpoint, "power", creature.getNetPower(),
                "toughness", creature.getNetToughness(),
                "plus_one_counters", creature.getCounters(CounterEnumType.P1P1),
                "minus_one_counters", creature.getCounters(CounterEnumType.M1M1),
                "zone", creature.getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT),
                "damage", creature.getDamage(), "stack_size", game.getStack().size(),
                "source_zones", zones, "source_exit", exits));
    }

    private static Map<String, Object> map(Object... pairs) {
        final Map<String, Object> values = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) values.put((String) pairs[i], pairs[i + 1]);
        return values;
    }

    /** No AI choice implementation is inherited. Every unplanned decision fails the fixture. */
    private static final class StrictController extends PlayerController {
        private int remainingCounterTypeCalls;
        private int usedCounterTypeCalls;
        private final int expectedCounterTargetId;
        StrictController(Game game, Player player, int counterTypeCalls, int counterTargetId) {
            super(game, player, player.getLobbyPlayer());
            remainingCounterTypeCalls = counterTypeCalls;
            expectedCounterTargetId = counterTargetId;
        }
        void assertConsumed() {
            Assert.assertEquals(remainingCounterTypeCalls, 0, "scripted singleton callback must be consumed");
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { throw new AssertionError("Unexpected controller operation: getAbilityToPlay"); }
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
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { throw new AssertionError("Unexpected controller operation: chooseTargetsFor"); }
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
        @Override public CounterType chooseCounterType(List<CounterType> options, SpellAbility sa,
                String prompt, Map<String, Object> params) {
            Assert.assertEquals(remainingCounterTypeCalls, 1, "unexpected or repeated counter callback");
            Assert.assertEquals(sa.getHostCard().getName(), "Battlegrowth");
            Assert.assertEquals(sa.getParam("CounterType"), "P1P1");
            Assert.assertEquals(sa.getParam("CounterNum"), "1");
            Assert.assertEquals(options.size(), 1, "only a forced singleton is accepted");
            Assert.assertEquals(options.get(0), CounterEnumType.P1P1);
            Assert.assertEquals(sa.getTargets().size(), 1);
            Assert.assertEquals(sa.getTargets().getFirstTargetedCard().getId(), expectedCounterTargetId);
            --remainingCounterTypeCalls;
            ++usedCounterTypeCalls;
            return options.get(0);
        }
        @Override public String chooseKeywordForPump(List<String> options, SpellAbility sa, String prompt, Card tgtCard) { throw new AssertionError("Unexpected controller operation: chooseKeywordForPump"); }
        @Override public boolean confirmPayment(CostPart costPart, String string, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: confirmPayment"); }
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { throw new AssertionError("Unexpected controller operation: chooseSingleReplacementEffect"); }
        @Override public StaticAbility chooseSingleStaticAbility(List<StaticAbility> possibleReplacers) { throw new AssertionError("Unexpected controller operation: chooseSingleStaticAbility"); }
        @Override public String chooseProtectionType(SpellAbility sa, List<String> choices) { throw new AssertionError("Unexpected controller operation: chooseProtectionType"); }
        @Override public void revealAnte(String message, Multimap<Player, PaperCard> removedAnteCards) { throw new AssertionError("Unexpected controller operation: revealAnte"); }
        @Override public void revealAISkipCards(String message, Map<Player, Map<DeckSection, List<? extends PaperCard>>> deckCards) { throw new AssertionError("Unexpected controller operation: revealAISkipCards"); }
        @Override public void revealUnsupported(Map<Player, List<PaperCard>> unsupported) { throw new AssertionError("Unexpected controller operation: revealUnsupported"); }
        @Override public List<OptionalCostValue> chooseOptionalCosts(SpellAbility choosen, List<OptionalCostValue> optionalCostValues) { throw new AssertionError("Unexpected controller operation: chooseOptionalCosts"); }
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { throw new AssertionError("Unexpected controller operation: orderCosts"); }
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { throw new AssertionError("Unexpected controller operation: payCostToPreventEffect"); }
        @Override public boolean payCostDuringRoll(Cost cost, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: payCostDuringRoll"); }
        @Override public boolean payCombatCost(Card card, Cost cost, SpellAbility sa, String prompt) { throw new AssertionError("Unexpected controller operation: payCombatCost"); }
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { throw new AssertionError("Unexpected controller operation: payManaCost"); }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { throw new AssertionError("Unexpected controller operation: applyManaToCost"); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw new AssertionError("Unexpected controller operation: chooseCardsForCost"); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { throw new AssertionError("Unexpected controller operation: getCostDecisionMaker"); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw new AssertionError("Unexpected controller operation: chooseSingleCardForZoneChange"); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw new AssertionError("Unexpected controller operation: chooseCardsForZoneChange"); }
        @Override public void autoPassCancel() { /* notification only; no player decision */ }
        @Override public void awaitNextInput() { /* notification only; no player decision */ }
        @Override public void cancelAwaitNextInput() { /* notification only; no player decision */ }
    }
}
