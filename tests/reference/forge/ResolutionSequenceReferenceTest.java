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

import com.google.common.eventbus.Subscribe;
import forge.card.CardRules;
import forge.card.CardRarity;
import forge.game.ability.AbilityFactory;
import forge.game.event.*;

/** Original authored primitive programs on a blank instant and a real vanilla Warden.
 * These are generic engine probes, not executable qualifications of real card text.
 * Factory SP/DB programs use one root target. Events are read only; real stack/SBA execute.
 */
public class ResolutionSequenceReferenceTest extends AITest {
    private static final String COMMIT = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256 = "bf3ab7147c15532883481b432893a15df7466f0900674e7fbe2fd2afd44d58e6";
    @Test public void zeroThenRecover() throws IOException { run("zero-then-recover"); }
    @Test public void lethalThenBounce() throws IOException { run("lethal-then-bounce"); }
    @Test public void shieldThenDamage() throws IOException { run("shield-then-damage"); }
    @Test public void sequentialDamageOrdering() throws IOException { run("sequential-damage-ordering"); }
    @Test public void bounceThenDamage() throws IOException { run("bounce-then-damage"); }

    private void run(String scenario) throws IOException {
        final Game game = initAndCreateGame();
        final Player actor = game.getPlayers().get(1);
        final Player defender = game.getPlayers().get(0);
        final Map<String, Card> cards = new LinkedHashMap<>();
        cards.put("warden", seed("Lumengrid Warden", "MRD", "41", defender, ZoneType.Battlefield));
        final CardRules rules = new CardRules.Reader().readCard(java.util.Arrays.asList(
                "Name:Spellwake Authored Sequence Probe", "ManaCost:1", "Types:Instant", "Oracle:"));
        final Card source = Card.fromPaperCard(new PaperCard(rules, "???", CardRarity.Common), actor);
        source.setGameTimestamp(game.getNextTimestamp());
        actor.getZone(ZoneType.Hand).add(source);
        cards.put("probe", source);
        final List<String> program = program(scenario);
        source.setSVar("Second", program.get(1));
        final SpellAbility ability = AbilityFactory.getAbility(program.get(0), source);
        Assert.assertTrue(ability.isSpell());
        Assert.assertNotNull(ability.getSubAbility());
        Assert.assertNull(ability.getSubAbility().getSubAbility());
        Assert.assertFalse(ability.getSubAbility().usesTargeting(), "only root captures one target");
        final Script script = new Script(game, actor, defender, cards, scenario);
        actor.addController(game.getNextTimestamp(), actor, new StrictController(game, actor, script), false);
        defender.addController(game.getNextTimestamp(), defender, new StrictController(game, defender, script), false);
        boolean completed = false;
        try {
            final Card warden = cards.get("warden");
            Assert.assertEquals(warden.getPaperCard().getRules().getPower(), "1");
            Assert.assertEquals(warden.getPaperCard().getRules().getToughness(), "3");
            Assert.assertTrue(warden.getPaperCard().getRules().getOracleText().isEmpty());
            Assert.assertTrue(source.getPaperCard().getRules().getOracleText().isEmpty());
            game.getAction().checkStaticAbilities();
            game.getAction().checkStateEffects(true);
            if (scenario.equals("sequential-damage-ordering")) {
                cards.put("blessing", seed("Shieldmate's Blessing", "ZEN", "35", actor, ZoneType.Hand));
                cards.put("mending", seed("Mending Hands", "BOK", "15", actor, ZoneType.Hand));
                resolveSetup(game, actor, cards.get("blessing"), warden);
                resolveSetup(game, actor, cards.get("mending"), warden);
                script.discoverShields();
                Assert.assertEquals(script.remaining(), map("blessing", 3, "mending", 4));
            }
            script.observe("initial");
            ability.setActivatingPlayer(actor);
            Assert.assertTrue(ability.canTarget(warden));
            ability.getTargets().add(warden);
            Assert.assertEquals(ability.getTargets().size(), 1);
            game.getStack().addAndUnfreeze(ability);
            Assert.assertEquals(game.getStack().size(), 1);
            Assert.assertEquals(game.getCardState(source).getZone().getZoneType(), ZoneType.Stack);
            Assert.assertEquals(game.getStack().peekAbility().getTargets().getFirstTargetedCard().getId(), warden.getId());
            game.subscribeToEvents(script);
            script.capture = true;
            game.getStack().resolveStack();
            script.capture = false;
            script.observe("before-sba");
            game.getAction().checkStateEffects(true);
            script.observe("settled");
            script.assertConsumed();
            Assert.assertFalse(game.isGameOver());
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
                        "owner", card.getOwner() == actor ? "A" : "B", "actual_class", card.getClass().getName(),
                        "authored_blank_probe", entry.getKey().equals("probe")));
            }
            final Map<String, Object> trace = map("schema_version", 1, "scenario", scenario, "reference", "forge", "commit", COMMIT,
                    "strict_choices", true, "ai_policy_used", false, "mana_auto_payment", false, "direct_stack_admission", true,
                    "qualification", "original-authored-primitive-program-probes-not-real-card-qualification",
                    "casting_scope", "direct-stack-admission-no-payment", "duration_expiration_tested", false,
                    "observation_scope", "read-only-events-plus-before-sba-and-settled", "expected_sha256", EXPECTED_SHA256,
                    "completed", completed, "program", program, "card_metadata", metadata, "observations", script.observations,
                    "events", script.events, "replacement_choices", script.choices, "replacement_callbacks", script.callbacks,
                    "raw_shields", script.rawShields, "event_failure", script.eventFailure == null ? null : script.eventFailure.toString());
            final String directory = System.getProperty("spellwake.evidence.dir");
            Assert.assertNotNull(directory, "explicit evidence destination required");
            Files.write(Paths.get(directory, scenario + ".json"),
                    (new GsonBuilder().serializeNulls().create().toJson(trace) + "\n").getBytes(StandardCharsets.UTF_8),
                    java.nio.file.StandardOpenOption.CREATE_NEW, java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario=" + scenario + " completed=" + completed + " checkpoints=" + script.observations.size());
        }
    }

    private static List<String> program(String name) {
        if (name.equals("zero-then-recover")) return java.util.Arrays.asList(
                "SP$ Animate | ValidTgts$ Creature | Power$ 1 | Toughness$ 0 | SubAbility$ Second",
                "DB$ Pump | Defined$ ParentTarget | NumAtt$ 0 | NumDef$ 2");
        if (name.equals("lethal-then-bounce")) return java.util.Arrays.asList(
                "SP$ DealDamage | ValidTgts$ Creature | NumDmg$ 3 | SubAbility$ Second",
                "DB$ ChangeZone | Defined$ ParentTarget | Origin$ Battlefield | Destination$ Hand");
        if (name.equals("shield-then-damage")) return java.util.Arrays.asList(
                "SP$ PreventDamage | ValidTgts$ Creature | Amount$ 4 | SubAbility$ Second",
                "DB$ DealDamage | Defined$ ParentTarget | NumDmg$ 3");
        if (name.equals("sequential-damage-ordering")) return java.util.Arrays.asList(
                "SP$ DealDamage | ValidTgts$ Creature | NumDmg$ 5 | SubAbility$ Second",
                "DB$ DealDamage | Defined$ ParentTarget | NumDmg$ 3");
        if (name.equals("bounce-then-damage")) return java.util.Arrays.asList(
                "SP$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand | SubAbility$ Second",
                "DB$ DealDamage | Defined$ ParentTarget | NumDmg$ 3");
        throw new AssertionError("Unknown scenario " + name);
    }
    private Card seed(String name, String edition, String number, Player owner, ZoneType zone) {
        final IPaperCard paper = FModel.getMagicDb().getCommonCards().getCard(name, edition, number);
        Assert.assertNotNull(paper, "exact real printing required " + name);
        Assert.assertEquals(paper.getEdition(), edition); Assert.assertEquals(paper.getCollectorNumber(), number);
        final Card card = Card.fromPaperCard(paper, owner);
        card.setGameTimestamp(owner.getGame().getNextTimestamp()); owner.getZone(zone).add(card); return card;
    }
    private void resolveSetup(Game game, Player actor, Card source, Card target) {
        Assert.assertTrue(game.getStack().isEmpty()); Assert.assertEquals(source.getSpellAbilities().size(), 1);
        final SpellAbility ability = source.getSpellAbilities().get(0);
        Assert.assertEquals(ability.getApi(), ApiType.PreventDamage); Assert.assertNull(ability.getSubAbility());
        ability.setActivatingPlayer(actor); Assert.assertTrue(ability.canTarget(target)); ability.getTargets().add(target);
        game.getStack().addAndUnfreeze(ability); game.getStack().resolveStack(); game.getAction().checkStateEffects(true);
        Assert.assertTrue(game.getStack().isEmpty()); Assert.assertTrue(game.getCardState(source).isInZone(ZoneType.Graveyard));
    }
    private static Map<String, Object> map(Object... pairs) {
        final Map<String, Object> values = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) values.put((String)pairs[i], pairs[i + 1]); return values;
    }

    /** Read-only event subscriber and strict replacement executor. No game state is assigned. */
    public static final class Script {
        private final Game game; private final Player actor; private final Player defender;
        private final Map<String, Card> cards; private final String scenario;
        private final Map<String, Card> shields = new LinkedHashMap<>();
        private final List<Map<String, Object>> observations = new ArrayList<>();
        private final List<Map<String, Object>> events = new ArrayList<>();
        private final List<Map<String, Object>> choices = new ArrayList<>();
        private final List<Map<String, Object>> callbacks = new ArrayList<>();
        private final List<Map<String, Object>> rawShields = new ArrayList<>();
        private Map<String, Object> pending; private int index; private boolean capture; private Throwable eventFailure;
        Script(Game game, Player actor, Player defender, Map<String, Card> cards, String scenario) {
            this.game=game; this.actor=actor; this.defender=defender; this.cards=cards; this.scenario=scenario;
        }
        String alias(Card card) {
            for (Map.Entry<String, Card> entry : cards.entrySet()) if (entry.getValue().getId()==card.getId()) return entry.getKey();
            throw new AssertionError("Unknown physical card " + card);
        }
        void discoverShields() {
            for (Card effect : game.getCardsIn(ZoneType.Command)) {
                if (effect.getEffectSource() != null && effect.hasSVar("ShieldAmount")) {
                    final String creator=alias(effect.getEffectSource());
                    final Card previous=shields.putIfAbsent(creator,effect);
                    Assert.assertTrue(previous==null || previous.getId()==effect.getId(), "one shield per explicit source");
                    Assert.assertEquals(effect.getReplacementEffects().size(),1);
                    Assert.assertEquals(effect.getRememberedCount(),1);
                    Assert.assertEquals(((Card)effect.getRemembered().iterator().next()).getId(),cards.get("warden").getId());
                }
            }
        }
        int amount(Card effect) {
            final String text=effect.getSVar("ShieldAmount"); Assert.assertTrue(text.matches("Number\\$[0-9]+"),text);
            return Integer.parseInt(text.substring(7));
        }
        String shieldAlias(Card effect) {
            discoverShields();
            for (Map.Entry<String,Card> entry:shields.entrySet()) if(entry.getValue().getId()==effect.getId()) return entry.getKey();
            throw new AssertionError("Unknown actual generated shield " + effect);
        }
        Map<String,Object> remaining() {
            discoverShields(); final Map<String,Object> result=new LinkedHashMap<>();
            for(Map.Entry<String,Card> entry:shields.entrySet()) {
                final Card live=game.getCardState(entry.getValue(),null);
                result.put(entry.getKey(),live!=null && live.isInZone(ZoneType.Command) ? amount(live) : 0);
            }
            return result;
        }
        Map<String,Object> state() {
            final Card target=game.getCardState(cards.get("warden")); final Card source=game.getCardState(cards.get("probe"));
            final boolean battlefield=target.isInZone(ZoneType.Battlefield);
            return map("zone",target.getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT),
                    "power",battlefield?target.getNetPower():null,"toughness",battlefield?target.getNetToughness():null,
                    "damage",battlefield?target.getDamage():null,"stack_size",game.getStack().size(),
                    "source_zone",source.getZone().getZoneType().name().toUpperCase(java.util.Locale.ROOT),
                    "resolving",game.getStack().isResolving(),"shield_remaining",remaining());
        }
        void observe(String checkpoint) {
            final Map<String,Object> point=map("checkpoint",checkpoint);point.putAll(state());observations.add(point);
            final List<Map<String,Object>> effects=new ArrayList<>();
            for(Map.Entry<String,Card> entry:shields.entrySet()) {
                final Card live=game.getCardState(entry.getValue(),null);final Card readable=live==null?entry.getValue():live;
                effects.add(map("source",entry.getKey(),"effect_id",entry.getValue().getId(),"live",live!=null,
                        "zone",live==null || live.getZone()==null?null:live.getZone().getZoneType().name(),
                        "stored_amount",readable.getSVar("ShieldAmount")));
            }
            rawShields.add(map("checkpoint",checkpoint,"effects",effects));
        }
        @Subscribe public void onEvent(GameEvent event) {
            if(!capture || eventFailure!=null) return;
            try {
                String kind=null;
                if(event instanceof GameEventCardStatsChanged e && e.cards().stream().anyMatch(c->c.getId()==cards.get("warden").getId())) kind="card-stats";
                if(event instanceof GameEventCardDamaged e && e.card().getId()==cards.get("warden").getId()) kind="card-damaged";
                if(event instanceof GameEventCardChangeZone e && e.card().getId()==cards.get("warden").getId()) kind="card-zone";
                if(event instanceof GameEventSpellResolved) kind="spell-resolved";
                if(kind!=null) {
                    final Map<String,Object> point=map("kind",kind);point.putAll(state());
                    point.put("raw_power",game.getCardState(cards.get("warden")).getNetPower());
                    point.put("raw_toughness",game.getCardState(cards.get("warden")).getNetToughness());
                    point.put("raw_damage",game.getCardState(cards.get("warden")).getDamage());events.add(point);
                }
            } catch(Throwable failure) { eventFailure=failure; }
        }
        ReplacementEffect choose(Player chooser,List<ReplacementEffect> candidates) {
            Assert.assertEquals(chooser,defender);Assert.assertNull(pending);
            final String chosen=scenario.equals("shield-then-damage")?"probe":index==0?"blessing":"mending";
            final List<String> domain=new ArrayList<>();ReplacementEffect result=null;
            for(ReplacementEffect effect:candidates) {
                final String name=shieldAlias(effect.getHostCard());domain.add(name);
                Assert.assertEquals(effect.getParam("PreventionEffect"),"NextN");
                if(name.equals(chosen)) result=effect;
            }
            java.util.Collections.sort(domain); Assert.assertNotNull(result,"scripted candidate required");
            Assert.assertEquals(game.getStack().size(),1);Assert.assertTrue(game.getStack().isResolving());
            Assert.assertTrue(game.getCardState(cards.get("probe")).isInZone(ZoneType.Stack));
            Assert.assertEquals(game.getStack().peekAbility().getHostCard().getId(),cards.get("probe").getId());
            pending=map("domain",domain,"chosen",chosen,"damage_before",game.getCardState(cards.get("warden")).getDamage());
            return result;
        }
        void resolveReplacement(Player controller,SpellAbility ability,boolean mayChooseTargets) {
            Assert.assertEquals(controller,actor);Assert.assertTrue(mayChooseTargets);Assert.assertNotNull(pending);
            Assert.assertTrue(ability.isReplacementAbility());Assert.assertEquals(ability.getApi(),ApiType.ReplaceDamage);
            Assert.assertNull(ability.getSubAbility());Assert.assertEquals(ability.getParam("Amount"),"ShieldAmount");
            Assert.assertEquals(shieldAlias(ability.getHostCard()),pending.get("chosen"));
            Assert.assertEquals(alias((Card)ability.getReplacingObject(AbilityKey.Target)),"warden");
            Assert.assertEquals(alias((Card)ability.getReplacingObject(AbilityKey.Source)),"probe");
            pending.put("pending_damage",ability.getReplacingObject(AbilityKey.DamageAmount));
            choices.add(new LinkedHashMap<>(pending));final Map<String,Object> before=state();
            AbilityUtils.resolve(ability);
            callbacks.add(map("choice_index",index,"chooser","B","executor","A","source_id",cards.get("probe").getId(),
                    "source","probe","target","warden","before",before,"after",state()));
            pending=null;++index;
        }
        void assertConsumed() {
            Assert.assertNull(eventFailure,"event subscriber failures must not be swallowed"); Assert.assertNull(pending);
            final int count=scenario.equals("shield-then-damage")?1:scenario.equals("sequential-damage-ordering")?3:0;
            Assert.assertEquals(index,count);Assert.assertEquals(callbacks.size(),count);
            Assert.assertTrue(game.getStack().isEmpty());Assert.assertTrue(game.getCardState(cards.get("probe")).isInZone(ZoneType.Graveyard));
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
