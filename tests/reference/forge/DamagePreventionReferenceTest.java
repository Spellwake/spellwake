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
import forge.game.ability.AbilityKey;
import forge.game.ability.AbilityUtils;
import forge.game.ability.ApiType;
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

/** Bounded real-card prevention reference with explicit scripted controller choices.
 * Direct admission bypasses casting/payment. No cleanup or duration expiry runs.
 */
public class DamagePreventionReferenceTest extends AITest {
    private static final String COMMIT = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256 = "9403e214eeb28c5d113c03dda419ebc15b25354bb9037a8fb1de14702966f5da";

    @Test public void singleShieldReuse() throws IOException { run("single-shield-reuse"); }
    @Test public void mendingFirst() throws IOException { run("mending-first"); }
    @Test public void blessingFirst() throws IOException { run("blessing-first"); }
    @Test public void repeatedChoice() throws IOException { run("repeated-choice"); }

    private void run(String scenario) throws IOException {
        final Game game = initAndCreateGame();
        final Player actor = game.getPlayers().get(1);
        final Player defender = game.getPlayers().get(0);
        final Map<String, Card> cards = new LinkedHashMap<>();
        final Card warden = seed("Lumengrid Warden", "MRD", "41", defender, ZoneType.Battlefield);
        cards.put("warden", warden);
        cards.put("mending", seed("Mending Hands", "BOK", "15", actor, ZoneType.Hand));
        if (scenario.equals("single-shield-reuse")) {
            cards.put("bolt-one", seed("Lightning Bolt", "M11", "149", actor, ZoneType.Hand));
            cards.put("bolt-two", seed("Lightning Bolt", "M11", "149", actor, ZoneType.Hand));
        } else if (scenario.equals("repeated-choice")) {
            cards.put("blessing-a", seed("Shieldmate's Blessing", "ZEN", "35", actor, ZoneType.Hand));
            cards.put("blessing-b", seed("Shieldmate's Blessing", "ZEN", "35", actor, ZoneType.Hand));
            cards.put("impact", seed("Explosive Impact", "RTR", "94", actor, ZoneType.Hand));
        } else {
            Assert.assertTrue(scenario.equals("mending-first") || scenario.equals("blessing-first"));
            cards.put("blessing", seed("Shieldmate's Blessing", "ZEN", "35", actor, ZoneType.Hand));
            cards.put("bolt", seed("Lightning Bolt", "M11", "149", actor, ZoneType.Hand));
        }
        final Script script = new Script(game, actor, defender, cards);
        if (scenario.equals("single-shield-reuse")) {
            script.expect("bolt-one", "mending", 3, "mending");
            script.expect("bolt-two", "mending", 3, "mending");
        } else if (scenario.equals("repeated-choice")) {
            script.expect("impact", "blessing-a", 5, "blessing-a", "blessing-b", "mending");
            script.expect("impact", "blessing-b", 2, "blessing-b", "mending");
        } else {
            script.expect("bolt", scenario.equals("mending-first") ? "mending" : "blessing", 3, "blessing", "mending");
        }
        actor.addController(game.getNextTimestamp(), actor, new StrictController(game, actor, script), false);
        defender.addController(game.getNextTimestamp(), defender, new StrictController(game, defender, script), false);
        boolean completed = false;
        try {
            Assert.assertEquals(warden.getPaperCard().getRules().getPower(), "1");
            Assert.assertEquals(warden.getPaperCard().getRules().getToughness(), "3");
            Assert.assertTrue(warden.getPaperCard().getRules().getOracleText().isEmpty());
            game.getAction().checkStaticAbilities();
            game.getAction().checkStateEffects(true);
            script.observe("initial", 0, map());
            admitResolve(game, actor, cards.get("mending"), warden, ApiType.PreventDamage, "4");
            script.bindShield("mending", 4);
            script.observe("after-mending", 0, map("mending", 4));
            if (scenario.equals("single-shield-reuse")) {
                admitResolve(game, actor, cards.get("bolt-one"), warden, ApiType.DealDamage, "3");
                script.observe("after-bolt-one", 0, map("mending", 1));
                admitResolve(game, actor, cards.get("bolt-two"), warden, ApiType.DealDamage, "3");
                script.observe("after-bolt-two", 2, map("mending", 0));
            } else if (scenario.equals("repeated-choice")) {
                admitResolve(game, actor, cards.get("blessing-a"), warden, ApiType.PreventDamage, "3");
                script.bindShield("blessing-a", 3);
                script.observe("after-blessing-a", 0, map("mending", 4, "blessing-a", 3));
                admitResolve(game, actor, cards.get("blessing-b"), warden, ApiType.PreventDamage, "3");
                script.bindShield("blessing-b", 3);
                script.observe("after-blessing-b", 0, map("mending", 4, "blessing-a", 3, "blessing-b", 3));
                admitResolve(game, actor, cards.get("impact"), warden, ApiType.DealDamage, "5");
                script.observe("after-impact", 0, map("mending", 4, "blessing-a", 0, "blessing-b", 1));
            } else {
                admitResolve(game, actor, cards.get("blessing"), warden, ApiType.PreventDamage, "3");
                script.bindShield("blessing", 3);
                script.observe("after-blessing", 0, map("mending", 4, "blessing", 3));
                admitResolve(game, actor, cards.get("bolt"), warden, ApiType.DealDamage, "3");
                script.observe("after-bolt", 0, scenario.equals("mending-first")
                        ? map("mending", 1, "blessing", 3) : map("mending", 4, "blessing", 0));
            }
            script.assertConsumed();
            completed = true;
        } finally {
            final List<Map<String, Object>> metadata = new ArrayList<>();
            for (Map.Entry<String, Card> entry : cards.entrySet()) {
                final Card card = game.getCardState(entry.getValue());
                final IPaperCard paper = card.getPaperCard();
                metadata.add(map("alias", entry.getKey(), "name", card.getName(), "edition", paper.getEdition(),
                        "collector_number", paper.getCollectorNumber(), "oracle", paper.getRules().getOracleText(),
                        "types", paper.getRules().getType().toString(), "mana_cost", paper.getRules().getManaCost().toString(),
                        "defined_power", paper.getRules().getPower(), "defined_toughness", paper.getRules().getToughness(),
                        "owner", card.getOwner() == actor ? "A" : "B", "actual_class", card.getClass().getName()));
            }
            final Map<String, Object> trace = map("schema_version", 1, "scenario", scenario, "reference", "forge",
                    "commit", COMMIT, "strict_choices", true, "ai_policy_used", false, "mana_auto_payment", false,
                    "direct_stack_admission", true, "casting_scope", "direct-stack-admission-no-payment",
                    "observation_scope", "initial-and-settled-resolution-after-state-checks", "duration_expiration_tested", false,
                    "expected_sha256", EXPECTED_SHA256, "completed", completed, "card_metadata", metadata,
                    "observations", script.observations, "replacement_choices", script.choices,
                    "replacement_resolution_callbacks", script.resolutions, "shield_effect_observations", script.effectObservations);
            final String directory = System.getProperty("spellwake.evidence.dir");
            Assert.assertNotNull(directory, "explicit evidence destination required");
            Files.write(Paths.get(directory, scenario + ".json"),
                    (new GsonBuilder().serializeNulls().create().toJson(trace) + "\n").getBytes(StandardCharsets.UTF_8),
                    java.nio.file.StandardOpenOption.CREATE_NEW, java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario=" + scenario + " completed=" + completed + " checkpoints=" + script.observations.size());
        }
    }

    private Card seed(String name, String edition, String number, Player owner, ZoneType zone) {
        final IPaperCard paper = FModel.getMagicDb().getCommonCards().getCard(name, edition, number);
        Assert.assertNotNull(paper, "missing exact printing " + name);
        Assert.assertEquals(paper.getEdition(), edition);
        Assert.assertEquals(paper.getCollectorNumber(), number);
        final Card card = Card.fromPaperCard(paper, owner);
        card.setGameTimestamp(owner.getGame().getNextTimestamp());
        owner.getZone(zone).add(card);
        return card;
    }

    private void admitResolve(Game game, Player actor, Card original, Card target, ApiType api, String amount) {
        Assert.assertTrue(game.getStack().isEmpty());
        final Card source = game.getCardState(original);
        final Card recipient = game.getCardState(target);
        Assert.assertEquals(source.getZone().getZoneType(), ZoneType.Hand);
        Assert.assertEquals(source.getOwner(), actor);
        Assert.assertEquals(recipient.getZone().getZoneType(), ZoneType.Battlefield);
        Assert.assertEquals(source.getSpellAbilities().size(), 1);
        final SpellAbility ability = source.getSpellAbilities().get(0);
        Assert.assertEquals(ability.getApi(), api);
        Assert.assertEquals(ability.getParam(api == ApiType.PreventDamage ? "Amount" : "NumDmg"), amount);
        Assert.assertEquals(ability.getParam("ValidTgts"), "Any");
        ability.setActivatingPlayer(actor);
        Assert.assertTrue(ability.canTarget(recipient));
        ability.getTargets().add(recipient);
        game.getStack().addAndUnfreeze(ability);
        Assert.assertEquals(game.getStack().size(), 1);
        Assert.assertEquals(ability.getHostCard().getZone().getZoneType(), ZoneType.Stack);
        Assert.assertEquals(game.getStack().peekAbility().getTargets().getFirstTargetedCard().getId(), recipient.getId());
        game.getStack().resolveStack();
        game.getAction().checkStateEffects(true);
        Assert.assertFalse(game.isGameOver());
        Assert.assertTrue(game.getStack().isEmpty());
        Assert.assertEquals(game.getCardState(original).getZone().getZoneType(), ZoneType.Graveyard);
    }

    private static Map<String, Object> map(Object... pairs) {
        final Map<String, Object> values = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) values.put((String) pairs[i], pairs[i + 1]);
        return values;
    }

    private static final class Script {
        private final Game game;
        private final Player actor;
        private final Player defender;
        private final Map<String, Card> cards;
        private final Map<String, Card> shields = new LinkedHashMap<>();
        private final List<Map<String, Object>> expectedChoices = new ArrayList<>();
        private final List<Map<String, Object>> choices = new ArrayList<>();
        private final List<Map<String, Object>> resolutions = new ArrayList<>();
        private final List<Map<String, Object>> observations = new ArrayList<>();
        private final List<Map<String, Object>> effectObservations = new ArrayList<>();
        private Map<String, Object> pendingChoice;
        private String pendingShield;
        private int index;
        Script(Game game, Player actor, Player defender, Map<String, Card> cards) {
            this.game = game; this.actor = actor; this.defender = defender; this.cards = cards;
        }
        void expect(String source, String chosen, int pending, String... domain) {
            expectedChoices.add(map("source", source, "domain", java.util.Arrays.asList(domain), "chosen", chosen,
                    "damage_before", 0, "pending_damage", pending, "chooser", "B", "target", "warden",
                    "source_controller", "A", "stack_size", 1, "source_zone", "STACK"));
        }
        String alias(Card card) {
            for (Map.Entry<String, Card> entry : cards.entrySet()) {
                if (entry.getValue().getId() == card.getId()) return entry.getKey();
            }
            throw new AssertionError("unknown actual card identity " + card);
        }
        String shieldAlias(Card effect) {
            for (Map.Entry<String, Card> entry : shields.entrySet()) {
                if (entry.getValue().getId() == effect.getId()) return entry.getKey();
            }
            throw new AssertionError("unknown actual shield identity " + effect);
        }
        int storedAmount(Card effect) {
            final String value = effect.getSVar("ShieldAmount");
            Assert.assertTrue(value.matches("Number\\$[0-9]+"), "exact literal shield SVar " + value);
            return Integer.parseInt(value.substring(7));
        }
        void bindShield(String sourceAlias, int expectedAmount) {
            final List<Card> matches = new ArrayList<>();
            for (Card effect : game.getCardsIn(ZoneType.Command)) {
                if (effect.getEffectSource() != null && effect.getEffectSource().getId() == cards.get(sourceAlias).getId()) matches.add(effect);
            }
            Assert.assertEquals(matches.size(), 1, "one actual effect for each resolved source");
            final Card effect = matches.get(0);
            Assert.assertEquals(effect.getController(), actor);
            Assert.assertEquals(effect.getRememberedCount(), 1);
            Assert.assertTrue(effect.getRemembered().iterator().next() instanceof Card);
            Assert.assertEquals(((Card)effect.getRemembered().iterator().next()).getId(), cards.get("warden").getId());
            Assert.assertEquals(effect.getReplacementEffects().size(), 1);
            Assert.assertEquals(effect.getReplacementEffects().get(0).getParam("PreventionEffect"), "NextN");
            Assert.assertEquals(storedAmount(effect), expectedAmount);
            Assert.assertNull(shields.put(sourceAlias, effect));
        }
        ReplacementEffect choose(Player chooser, List<ReplacementEffect> candidates) {
            Assert.assertEquals(chooser, defender, "only affected creature controller chooses ordering");
            Assert.assertEquals(game.getCardState(cards.get("warden")).getController(), chooser);
            Assert.assertNull(pendingChoice, "previous replacement callback must finish first");
            Assert.assertTrue(index < expectedChoices.size(), "unexpected extra replacement choice");
            final Map<String, Object> expected = expectedChoices.get(index);
            final List<String> domain = new ArrayList<>();
            ReplacementEffect selected = null;
            for (ReplacementEffect candidate : candidates) {
                final Card effect = candidate.getHostCard();
                final String candidateAlias = shieldAlias(effect);
                Assert.assertEquals(effect.getZone().getZoneType(), ZoneType.Command);
                Assert.assertTrue(storedAmount(effect) > 0);
                Assert.assertEquals(candidate.getParam("PreventionEffect"), "NextN");
                Assert.assertEquals(candidate.getOverridingAbility().getApi(), ApiType.ReplaceDamage);
                Assert.assertTrue(candidate.matchesValidParam("ValidTarget", game.getCardState(cards.get("warden"))));
                Assert.assertFalse(domain.contains(candidateAlias));
                domain.add(candidateAlias);
                if (candidateAlias.equals(expected.get("chosen"))) selected = candidate;
            }
            java.util.Collections.sort(domain);
            final Card source = game.getStack().peekAbility().getHostCard();
            final Card target = game.getCardState(cards.get("warden"));
            pendingChoice = map("source", alias(source), "domain", domain, "chosen", selected == null ? null : shieldAlias(selected.getHostCard()),
                    "damage_before", target.getDamage(), "chooser", chooser == defender ? "B" : "A", "target", alias(target),
                    "source_controller", source.getController() == actor ? "A" : "B", "stack_size", game.getStack().size(),
                    "source_zone", source.getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT));
            Assert.assertEquals(domain, expected.get("domain"));
            Assert.assertEquals(alias(source), expected.get("source"));
            Assert.assertNotNull(selected, "scripted shield must be in actual domain");
            pendingShield = shieldAlias(selected.getHostCard());
            return selected;
        }
        void resolveReplacement(Player controller, SpellAbility ability, boolean mayChooseTargets) {
            Assert.assertEquals(controller, actor, "generated effect retains creating controller");
            Assert.assertNotNull(pendingChoice, "replacement execution must follow chosen shield");
            Assert.assertTrue(mayChooseTargets);
            Assert.assertTrue(ability.isReplacementAbility());
            Assert.assertEquals(ability.getApi(), ApiType.ReplaceDamage);
            Assert.assertEquals(ability.getParam("Amount"), "ShieldAmount");
            Assert.assertNull(ability.getSubAbility(), "no additional prevention consequences allowed");
            Assert.assertEquals(shieldAlias(ability.getHostCard()), pendingShield);
            final Card target = (Card)ability.getReplacingObject(AbilityKey.Target);
            final Card source = (Card)ability.getReplacingObject(AbilityKey.Source);
            Assert.assertEquals(alias(target), "warden");
            Assert.assertEquals(alias(source), pendingChoice.get("source"));
            final Integer damage = (Integer)ability.getReplacingObject(AbilityKey.DamageAmount);
            pendingChoice.put("pending_damage", damage);
            choices.add(new LinkedHashMap<>(pendingChoice));
            Assert.assertEquals(pendingChoice, expectedChoices.get(index));
            final String before = ability.getHostCard().getSVar("ShieldAmount");
            AbilityUtils.resolve(ability);
            final Card currentHost = game.getCardState(ability.getHostCard(), null);
            resolutions.add(map("source", alias(source), "shield", pendingShield, "controller", "A", "target", alias(target),
                    "pending_damage", damage, "stored_shield_before", before,
                    "retained_host_svar_after", ability.getHostCard().getSVar("ShieldAmount"),
                    "retained_host_zone_after", ability.getHostCard().getZone() == null ? null : ability.getHostCard().getZone().getZoneType().name(),
                    "current_host_present_after", currentHost != null,
                    "current_host_zone_after", currentHost == null || currentHost.getZone() == null ? null : currentHost.getZone().getZoneType().name(),
                    "marked_damage_after_callback", game.getCardState(cards.get("warden")).getDamage()));
            pendingChoice = null; pendingShield = null; ++index;
        }
        void observe(String checkpoint, int expectedDamage, Map<String, Object> expectedShield) {
            final Card target = game.getCardState(cards.get("warden"));
            final Map<String, Object> sourceZones = new LinkedHashMap<>();
            for (Map.Entry<String, Card> entry : cards.entrySet()) {
                if (!entry.getKey().equals("warden")) sourceZones.put(entry.getKey(), game.getCardState(entry.getValue()).getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT));
            }
            final Map<String, Object> remaining = new LinkedHashMap<>();
            final List<Map<String, Object>> rawEffects = new ArrayList<>();
            for (Map.Entry<String, Card> entry : shields.entrySet()) {
                final Card actual = game.getCardState(entry.getValue(), null);
                final boolean active = actual != null && actual.isInZone(ZoneType.Command);
                final Card readable = actual == null ? entry.getValue() : actual;
                remaining.put(entry.getKey(), active ? storedAmount(actual) : 0);
                rawEffects.add(map("source", entry.getKey(), "effect_id", entry.getValue().getId(), "live_in_game", actual != null,
                        "active", active, "actual_zone", actual == null || actual.getZone() == null ? null : actual.getZone().getZoneType().name(),
                        "stored_shield_amount", readable.getSVar("ShieldAmount"), "stored_value_from", actual == null ? "retained-original-reference" : "current-game-object"));
            }
            effectObservations.add(map("checkpoint", checkpoint, "effects", rawEffects));
            final Map<String, Object> point = map("checkpoint", checkpoint, "power", target.getNetPower(), "toughness", target.getNetToughness(),
                    "zone", target.getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT), "damage", target.getDamage(),
                    "owner", target.getOwner() == defender ? "B" : "A", "controller", target.getController() == defender ? "B" : "A",
                    "stack_size", game.getStack().size(), "source_zones", sourceZones, "shield_remaining", remaining);
            observations.add(point);
            Assert.assertEquals(target.getOwner(), defender);
            Assert.assertEquals(target.getController(), defender);
            Assert.assertEquals(target.getZone().getZoneType(), ZoneType.Battlefield);
            Assert.assertEquals(target.getNetPower(), 1);
            Assert.assertEquals(target.getNetToughness(), 3);
            Assert.assertEquals(target.getDamage(), expectedDamage, checkpoint);
            Assert.assertEquals(target.getNumAllCounters(), 0);
            Assert.assertTrue(game.getStack().isEmpty());
            Assert.assertEquals(remaining, expectedShield, checkpoint);
        }
        void assertConsumed() {
            Assert.assertNull(pendingChoice);
            Assert.assertEquals(index, expectedChoices.size(), "every scripted callback consumed exactly once");
            Assert.assertEquals(resolutions.size(), index);
        }
    }

    /** No AI policy is inherited; only exact replacement execution/order callbacks are permitted. */
    private static final class StrictController extends PlayerController {
        private final Script script;
        StrictController(Game game, Player player, Script script) {
            super(game, player, player.getLobbyPlayer()); this.script = script;
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { throw new AssertionError("Unexpected controller operation: getAbilityToPlay"); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { script.resolveReplacement(player, effectSA, mayChoseNewTargets); }
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
        @Override public CounterType chooseCounterType(List<CounterType> options, SpellAbility sa, String prompt, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseCounterType"); }
        @Override public String chooseKeywordForPump(List<String> options, SpellAbility sa, String prompt, Card tgtCard) { throw new AssertionError("Unexpected controller operation: chooseKeywordForPump"); }
        @Override public boolean confirmPayment(CostPart costPart, String string, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: confirmPayment"); }
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { return script.choose(player, possibleReplacers); }
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
