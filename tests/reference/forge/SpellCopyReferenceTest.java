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
import java.util.Collections;
import java.util.LinkedHashMap;
import org.testng.Assert;
import org.testng.annotations.Test;
import forge.model.FModel;
import forge.item.IPaperCard;

/** Original strict fixture for one nonpermanent spell copy and independent stack lifetimes.
 * Physical spells use direct stack admission. Casting, payment and ordinary priority are bypassed.
 */
public class SpellCopyReferenceTest extends AITest {
    private static final String COMMIT = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";

    @Test public void copyRedirect() throws IOException { run("copy-redirect"); }
    @Test public void counterOriginal() throws IOException { run("counter-original"); }
    @Test public void counterCopy() throws IOException { run("counter-copy"); }
    @Test public void copyRetainMissingTarget() throws IOException { run("copy-retain-missing-target"); }
    @Test public void copyRetargetMissingTarget() throws IOException { run("copy-retarget-missing-target"); }
    @Test public void copyTargetDeparts() throws IOException { run("copy-target-departs"); }

    private void run(String scenario) throws IOException {
        final Game game = initAndCreateGame();
        final Player a = game.getPlayers().get(1);
        final Player b = game.getPlayers().get(0);
        final Map<String, Card> aliases = new LinkedHashMap<>();
        aliases.put("bolt", seed("Lightning Bolt", "M11", "149", a, ZoneType.Hand));
        aliases.put("twincast", seed("Twincast", "M10", "78", b, ZoneType.Hand));
        aliases.put("counter", seed("Counterspell", "7ED", "67", a, ZoneType.Hand));
        aliases.put("bounce", seed("Unsummon", "M11", "78", a, ZoneType.Hand));
        aliases.put("lion1", seed("Silvercoat Lion", "M11", "31", b, ZoneType.Battlefield));
        aliases.put("lion2", seed("Silvercoat Lion", "M11", "31", a, ZoneType.Battlefield));
        final boolean missing = scenario.endsWith("missing-target");
        final boolean retain = scenario.equals("copy-retain-missing-target") || scenario.equals("copy-target-departs");
        final StrictController controllerA = new StrictController(game, a, aliases.get("lion1"), null, false);
        final StrictController controllerB = new StrictController(game, b, aliases.get("lion1"), retain ? null : aliases.get("lion2"), true);
        a.addController(game.getNextTimestamp(), a, controllerA, false);
        b.addController(game.getNextTimestamp(), b, controllerB, false);
        game.getAction().checkStaticAbilities();
        final List<Map<String, Object>> observations = new ArrayList<>();
        final SpellAbility bolt = admit(game, aliases.get("bolt"), a, aliases.get("lion1"));
        observe(game, aliases, controllerB, observations, "bolt-on-stack", "BATTLEFIELD", "BATTLEFIELD", "bolt");
        admit(game, aliases.get("twincast"), b, bolt);
        observe(game, aliases, controllerB, observations, "twincast-on-stack", "BATTLEFIELD", "BATTLEFIELD", "bolt", "twincast");
        if (missing) {
            admit(game, aliases.get("bounce"), a, aliases.get("lion1"));
            observe(game, aliases, controllerB, observations, "bounce-on-stack", "BATTLEFIELD", "BATTLEFIELD", "bolt", "twincast", "bounce");
            resolve(game);
            observe(game, aliases, controllerB, observations, "after-bounce", "HAND", "BATTLEFIELD", "bolt", "twincast");
        }
        resolve(game);
        final String lion1Before = missing ? "HAND" : "BATTLEFIELD";
        observe(game, aliases, controllerB, observations, "after-copy", lion1Before, "BATTLEFIELD", "bolt", "bolt-copy");
        if (scenario.startsWith("counter-")) {
            final boolean counterCopy = scenario.equals("counter-copy");
            admit(game, aliases.get("counter"), a, counterCopy ? game.getStack().peekAbility() : bolt);
            observe(game, aliases, controllerB, observations, "counter-on-stack", "BATTLEFIELD", "BATTLEFIELD", "bolt", "bolt-copy", "counter");
            resolve(game);
            observe(game, aliases, controllerB, observations, "after-counter", "BATTLEFIELD", "BATTLEFIELD", counterCopy ? "bolt" : "bolt-copy");
            resolve(game);
            observe(game, aliases, controllerB, observations, "finished", counterCopy ? "GRAVEYARD" : "BATTLEFIELD", counterCopy ? "BATTLEFIELD" : "GRAVEYARD");
        } else {
            if (scenario.equals("copy-target-departs")) {
                admit(game, aliases.get("bounce"), a, aliases.get("lion1"));
                observe(game, aliases, controllerB, observations, "bounce-on-stack", "BATTLEFIELD", "BATTLEFIELD", "bolt", "bolt-copy", "bounce");
                resolve(game);
                observe(game, aliases, controllerB, observations, "after-bounce", "HAND", "BATTLEFIELD", "bolt", "bolt-copy");
            }
            resolve(game);
            final boolean departed = missing || scenario.equals("copy-target-departs");
            observe(game, aliases, controllerB, observations, "after-copy-resolution", departed ? "HAND" : "BATTLEFIELD", retain ? "BATTLEFIELD" : "GRAVEYARD", "bolt");
            resolve(game);
            observe(game, aliases, controllerB, observations, "finished", departed ? "HAND" : "GRAVEYARD", retain ? "BATTLEFIELD" : "GRAVEYARD");
        }
        Assert.assertTrue(game.getStack().isEmpty(), "no scripted stack entries may remain");
        controllerA.assertConsumed();
        controllerB.assertConsumed();
        Assert.assertFalse(game.getCardsInGame().contains(controllerB.copiedCard), "copy must be absent from all game zones after settlement");
        final Map<String, Object> output = map("schema_version", 2, "scenario", scenario,
                "reference", "forge", "commit", COMMIT, "strict_choices", true,
                "mana_auto_payment", false, "direct_stack_admission", true,
                "casting_scope", "direct-stack-admission-no-payment",
                "observation_scope", "settled-resolution-boundaries",
                "pre_sba_copy_exit_observed", false, "observations", observations);
        final String dir = System.getProperty("spellwake.evidence.dir");
        Assert.assertNotNull(dir, "explicit evidence destination required");
        Files.write(Paths.get(dir, scenario + ".json"), (new GsonBuilder().serializeNulls().create().toJson(output) + "\n").getBytes(StandardCharsets.UTF_8));
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

    private SpellAbility admit(Game game, Card card, Player actor, GameObject target) {
        final SpellAbility ability = game.getCardState(card).getSpellAbilities().get(0);
        ability.setActivatingPlayer(actor);
        Assert.assertTrue(ability.canTarget(target), "scripted target must be legal before admission");
        Assert.assertTrue(ability.getTargets().add(target));
        final int before = game.getStack().size();
        game.getStack().addAndUnfreeze(ability);
        Assert.assertEquals(game.getStack().size(), before + 1, "spell must actually be admitted");
        Assert.assertEquals(ability.getHostCard().getZone().getZoneType(), ZoneType.Stack);
        return game.getStack().peekAbility();
    }

    private void resolve(Game game) {
        Assert.assertFalse(game.getStack().isEmpty());
        game.getStack().resolveStack();
        game.getAction().checkStateEffects(true);
        Assert.assertFalse(game.isGameOver(), "unexpected end of seeded test game");
    }

    private void observe(Game game, Map<String, Card> aliases, StrictController controller,
            List<Map<String, Object>> observations, String checkpoint, String lion1Zone,
            String lion2Zone, String... expectedStack) {
        final List<SpellAbilityStackInstance> stack = new ArrayList<>();
        for (SpellAbilityStackInstance item : game.getStack()) stack.add(item);
        Collections.reverse(stack);
        final List<Map<String, Object>> entries = new ArrayList<>();
        final List<String> stackAliases = new ArrayList<>();
        for (SpellAbilityStackInstance item : stack) {
            final Card source = item.getSourceCard();
            final String stackAlias = alias(aliases, controller, source);
            final List<String> targets = new ArrayList<>();
            for (GameObject target : item.getTargetChoices()) {
                if (target instanceof Card) targets.add(alias(aliases, controller, (Card) target));
                else if (target instanceof SpellAbility) targets.add(alias(aliases, controller, ((SpellAbility) target).getHostCard()));
                else throw new AssertionError("Unknown target kind: " + target.getClass());
            }
            stackAliases.add(stackAlias);
            entries.add(map("alias", stackAlias, "kind", source.isCopiedSpell() ? "spell-copy" : "card-spell",
                    "owner", playerAlias(game, source.getOwner()), "controller", playerAlias(game, source.getController()), "targets", targets));
        }
        Assert.assertEquals(stackAliases, java.util.Arrays.asList(expectedStack));
        final List<Map<String, Object>> cards = new ArrayList<>();
        for (Map.Entry<String, Card> entry : aliases.entrySet()) {
            final Card card = game.getCardState(entry.getValue());
            Assert.assertFalse(card.isCopiedSpell(), "physical aliases cannot resolve to a copied host");
            final ZoneType zone = card.getZone().getZoneType();
            final boolean battlefield = zone == ZoneType.Battlefield;
            final String normalizedZone = zone.name().toUpperCase(java.util.Locale.ROOT);
            if (entry.getKey().equals("lion1")) Assert.assertEquals(normalizedZone, lion1Zone);
            if (entry.getKey().equals("lion2")) Assert.assertEquals(normalizedZone, lion2Zone);
            if (battlefield) {
                Assert.assertEquals(card.getNetPower(), 2);
                Assert.assertEquals(card.getNetToughness(), 2);
                Assert.assertEquals(card.getDamage(), 0);
            }
            cards.add(map("alias", entry.getKey(), "owner", playerAlias(game, card.getOwner()),
                    "zone", normalizedZone, "damage", battlefield ? card.getDamage() : null,
                    "power", battlefield ? card.getNetPower() : null,
                    "toughness", battlefield ? card.getNetToughness() : null,
                    "actual_controller", playerAlias(game, card.getController()),
                    "class", card.getClass().getName(), "set", card.getPaperCard().getEdition(),
                    "number", card.getPaperCard().getCollectorNumber()));
        }
        int copiedCards = 0;
        for (Card card : game.getCardsInGame()) {
            if (card.isCopiedSpell()) {
                Assert.assertSame(card, controller.copiedCard, "unexpected copied card");
                copiedCards++;
            }
        }
        Assert.assertEquals(copiedCards, stackAliases.contains("bolt-copy") ? 1 : 0, "copy zone presence must match its settled stack lifetime");
        observations.add(map("checkpoint", checkpoint, "stack_bottom_first", entries,
                "life", map("A", game.getPlayers().get(1).getLife(), "B", game.getPlayers().get(0).getLife()), "cards", cards));
    }

    private static String alias(Map<String, Card> aliases, StrictController controller, Card card) {
        if (controller.copiedCard != null && controller.copiedCard.getId() == card.getId()) return "bolt-copy";
        for (Map.Entry<String, Card> entry : aliases.entrySet()) {
            if (entry.getValue().getId() == card.getId()) return entry.getKey();
        }
        throw new AssertionError("Unexpected unaliased card: " + card.getName());
    }

    private static String playerAlias(Game game, Player player) {
        if (player == game.getPlayers().get(1)) return "A";
        if (player == game.getPlayers().get(0)) return "B";
        throw new AssertionError("Unexpected player: " + player);
    }

    private static Map<String, Object> map(Object... pairs) {
        final Map<String, Object> values = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) values.put((String) pairs[i], pairs[i + 1]);
        return values;
    }

    /** No AI choice implementation is inherited. Every unplanned decision fails the fixture. */
    private static final class StrictController extends PlayerController {
        private final Card inheritedTarget;
        private final Card replacementTarget;
        private final boolean expectsCopy;
        private SpellAbility copiedAbility;
        private Card copiedCard;
        private int copiesPlayed;
        private int targetChoices;

        StrictController(Game game, Player player, Card inherited, Card replacement, boolean expects) {
            super(game, player, player.getLobbyPlayer());
            inheritedTarget = inherited;
            replacementTarget = replacement;
            expectsCopy = expects;
        }

        void assertConsumed() {
            Assert.assertEquals(copiesPlayed, expectsCopy ? 1 : 0, "scripted copy action must be consumed exactly once");
            Assert.assertEquals(targetChoices, expectsCopy ? 1 : 0, "scripted target choice must be consumed exactly once");
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { throw new AssertionError("Unexpected controller operation: getAbilityToPlay"); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw new AssertionError("Unexpected controller operation: playSpellAbilityNoStack"); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { if (!activePlayerSAs.isEmpty()) throw new AssertionError("Unexpected triggers"); return java.util.Collections.emptyList(); }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) {
            if (activePlayerSAs.isEmpty()) return;
            Assert.assertTrue(expectsCopy, "unexpected copy controller");
            Assert.assertEquals(copiesPlayed++, 0, "unexpected repeated copy action");
            Assert.assertEquals(activePlayerSAs.size(), 1, "only one copied Bolt is scripted");
            final SpellAbility next = activePlayerSAs.get(0);
            Assert.assertTrue(next.isCopied() && next.isSpell() && next.getHostCard().isCopiedSpell());
            Assert.assertEquals(next.getHostCard().getName(), "Lightning Bolt");
            Assert.assertEquals(next.getActivatingPlayer(), getPlayer());
            Assert.assertEquals(next.getHostCard().getOwner(), getPlayer());
            Assert.assertTrue(next.isMayChooseNewTargets());
            Assert.assertEquals(next.getTargets().size(), 1);
            Assert.assertEquals(next.getTargets().getFirstTargetedCard().getId(), inheritedTarget.getId());
            copiedAbility = next;
            // This is the non-choice copied-spell path from PlayerControllerHuman.
            if (!next.getHostCard().isInZone(ZoneType.Stack)) {
                next.setHostCard(getGame().getAction().moveToStack(next.getHostCard(), next));
            } else {
                getGame().getStackZone().add(next.getHostCard());
            }
            copiedCard = next.getHostCard();
            Assert.assertTrue(next.setupNewTargets(getPlayer()));
            getGame().getStack().add(next);
        }
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
        @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) {
            Assert.assertSame(ability, copiedAbility, "only the created copy may request targets");
            Assert.assertEquals(targetChoices++, 0, "unexpected repeated target decision");
            Assert.assertNull(filter);
            Assert.assertTrue(optional);
            if (replacementTarget == null) return null;
            final Card target = getGame().getCardState(replacementTarget);
            Assert.assertTrue(ability.canTarget(target), "new target must be legal");
            ability.clearTargets();
            Assert.assertTrue(ability.getTargets().add(target));
            return ability.getTargets();
        }
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
