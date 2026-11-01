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

/** Original fixed tap-only noncreature Land activation probes.
 * Successful actions traverse PlaySpellAbility, actual CostTap payment and the
 * MagicStack immediate-mana branch. Setup is already on the battlefield; ETB
 * replacements/triggers, land play and the production priority loop are excluded.
 */
public class ManaAbilityReferenceTest extends AITest {
    private static final String COMMIT = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256 = "4dd50f4dd642fb20211a9e0c048f8666785cb78fd30f2a6ef97781492112e0c7";
    private static final String COLORS = "WUBRGC";
    @Test public void basicTypes() throws IOException { run("basic-types"); }
    @Test public void fixedAlternatives() throws IOException { run("fixed-alternatives"); }
    @Test public void fixedMultipleOutput() throws IOException { run("fixed-multiple-output"); }
    @Test public void sequentialSources() throws IOException { run("sequential-sources"); }
    @Test public void repeatTappedRejected() throws IOException { run("repeat-tapped-rejected"); }
    @Test public void controllerReceivesMana() throws IOException { run("controller-receives-mana"); }

    private void run(String scenario) throws IOException {
        final List<Map<String, Object>> probes = new ArrayList<>();
        boolean completed = false;
        try {
            if (scenario.equals("basic-types")) {
                final String[] names = {"Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"};
                for (int i=0; i<COLORS.length(); i++) probes.add(single("basic-" + Character.toLowerCase(COLORS.charAt(i)), names[i], String.valueOf(COLORS.charAt(i)), false));
            } else if (scenario.equals("fixed-alternatives")) {
                probes.add(single("tundra-white", "Tundra", "W", false));
                probes.add(single("tundra-blue", "Tundra", "U", false));
            } else if (scenario.equals("fixed-multiple-output")) {
                probes.add(single("guildless-colorless-two", "Guildless Commons", "CC", false));
                probes.add(single("boros-red-white", "Boros Garrison", "RW", false));
            } else if (scenario.equals("sequential-sources")) {
                final Script script = setup();
                script.add("mountain", "Mountain", false); script.add("wastes", "Wastes", false); script.seedPools();
                script.observe("initial", null);
                final Map<String, Object> first = script.activate("mountain", "R"); script.observe("mountain-activated", first.get("accepted"));
                final Map<String, Object> second = script.activate("wastes", "C"); script.observe("wastes-activated", second.get("accepted"));
                probes.add(script.finish("mountain-wastes", java.util.Arrays.asList("R", "C"), java.util.Arrays.asList(first.get("produced"), second.get("produced"))));
            } else if (scenario.equals("repeat-tapped-rejected")) {
                final Script script = setup(); script.add("source", "Mountain", false); script.seedPools(); script.observe("initial", null);
                final Map<String, Object> first = script.activate("source", "R"); script.observe("first-activation", first.get("accepted"));
                final Map<String, Object> second = script.activate("source", "R"); script.observe("rejected-repeat", second.get("accepted"));
                probes.add(script.finish("mountain-repeat", java.util.Arrays.asList("R", "R"), java.util.Arrays.asList(first.get("produced"), second.get("produced"))));
            } else if (scenario.equals("controller-receives-mana")) probes.add(single("stolen-mountain", "Mountain", "R", true));
            else throw new AssertionError("Unknown scenario " + scenario);
            completed = true;
        } finally {
            final Map<String, Object> trace = map("schema_version", 1, "scenario", scenario, "reference", "forge", "commit", COMMIT,
                    "expected_sha256", EXPECTED_SHA256, "completed", completed, "strict_choices", true, "ai_policy_used", false,
                    "activation_path", "PlaySpellAbility/CostPayment/CostTap/MagicStack-mana-resolution", "mana_abilities_activated", true,
                    "produced_pool_injection", false, "direct_effect_resolution", false, "casting_executed", false,
                    "land_play_or_etb_executed", false, "priority_loop_executed", false, "reference_rejection", "real-canPlay-preflight",
                    "qualification", "fixed-tap-noncreature-land-ability-isolated-activation", "probes", probes);
            final String directory = System.getProperty("spellwake.evidence.dir"); Assert.assertNotNull(directory);
            Files.write(Paths.get(directory, scenario + ".json"), (new GsonBuilder().serializeNulls().create().toJson(trace) + "\n").getBytes(StandardCharsets.UTF_8),
                    java.nio.file.StandardOpenOption.CREATE_NEW, java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario=" + scenario + " completed=" + completed + " probes=" + probes.size());
        }
    }
    private Script setup() {
        final Game game = initAndCreateGame(); final Script script = new Script(game);
        for (Player player : game.getPlayers()) {
            player.addController(game.getNextTimestamp(), player, new StrictController(game, player, script), false);
            Assert.assertEquals(player.getController().getClass(), StrictController.class);
        }
        return script;
    }
    private Map<String, Object> single(String id, String name, String ability, boolean stolen) {
        final Script script = setup(); script.add("source", name, stolen); script.seedPools(); script.observe("initial", null);
        final Map<String, Object> result = script.activate("source", ability); script.observe("activated", result.get("accepted"));
        return script.finish(id, ability, result.get("produced"));
    }
    private static byte color(char color) { return ManaAtom.fromName(String.valueOf(color)); }
    private static Map<String, Object> pool(ManaPool pool) {
        final Map<String, Object> result = new LinkedHashMap<>();
        for (char color : COLORS.toCharArray()) result.put(String.valueOf(color), pool.getAmountOfColor(color(color)));
        return result;
    }
    private static Map<String, Object> produced(SpellAbility ability) {
        final Map<String, Object> result = new LinkedHashMap<>();
        for (char c : COLORS.toCharArray()) {
            int count=0;
            for (Mana mana : ability.getManaPart().getLastManaProduced()) if (mana.getColor() == color(c)) count++;
            result.put(String.valueOf(c), count);
        }
        return result;
    }
    private static Map<String, Object> map(Object... pairs) {
        final Map<String, Object> result = new LinkedHashMap<>();
        for (int i=0; i<pairs.length; i+=2) result.put((String)pairs[i], pairs[i+1]); return result;
    }
    /** Real source identities and actual pool/state/event reads; no expected file input. */
    public static final class Script {
        final Game game; final Player actor; final Player other;
        final Map<String, Card> sources = new LinkedHashMap<>(); final Map<String, Object> metadata = new LinkedHashMap<>();
        final List<Map<String, Object>> snapshots = new ArrayList<>(); final List<Map<String, Object>> actions = new ArrayList<>();
        final List<Map<String, Object>> events = new ArrayList<>();
        SpellAbility selected; int tapDecisions; int zeroDecisions; int zeroPayments; int abilitySelections; Throwable eventFailure;
        Script(Game game) { this.game=game; actor=game.getPlayers().get(1); other=game.getPlayers().get(0); }
        String alias(Player player) { if(player==actor) return "A"; if(player==other) return "B"; throw new AssertionError("Unknown player"); }
        Card add(String alias, String name, boolean stolen) {
            final IPaperCard paper = FModel.getMagicDb().getCommonCards().getCard(name); Assert.assertNotNull(paper);
            final Card card = Card.fromPaperCard(paper, stolen ? other : actor);
            card.setGameTimestamp(game.getNextTimestamp()); if(stolen) card.setController(actor,game.getNextTimestamp());
            actor.getZone(ZoneType.Battlefield).add(card); Assert.assertTrue(card.isLand()); Assert.assertFalse(card.isCreature()); Assert.assertFalse(card.isTapped());
            sources.put(alias,card);
            metadata.put(alias,map("name",paper.getName(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),
                    "oracle",paper.getRules().getOracleText(),"types",paper.getRules().getType().toString(),"actual_class",card.getClass().getName()));
            return card;
        }
        void seedPools() {
            final Card seed = sources.values().iterator().next();
            Assert.assertTrue(actor.getManaPool().isEmpty()); Assert.assertTrue(other.getManaPool().isEmpty());
            // Preexisting floating setup only. All action production is executed by Forge.
            for(char c : "BBC".toCharArray()) actor.getManaPool().addMana(new Mana(color(c),seed,null,actor));
            other.getManaPool().addMana(new Mana(color('W'),seed,null,other));
            game.subscribeToEvents(this);
        }
        Map<String, Object> sourceStates() {
            final Map<String, Object> result=new LinkedHashMap<>();
            for(Map.Entry<String,Card> entry:sources.entrySet()) {
                final Card card=entry.getValue(); Assert.assertSame(game.getCardState(card),card);
                result.put(entry.getKey(),map("card",card.getName(),"zone",card.getZone().getZoneType().name(),"owner",alias(card.getOwner()),"controller",alias(card.getController()),"tapped",card.isTapped()));
            }
            return result;
        }
        Map<String, Object> identities() {
            final Map<String, Object> result=new LinkedHashMap<>();
            for(Map.Entry<String,Card> entry:sources.entrySet()) result.put(entry.getKey(),map("id",entry.getValue().getId(),"timestamp",entry.getValue().getGameTimestamp()));
            return result;
        }
        void observe(String label,Object accepted) {
            snapshots.add(map("label",label,"sources",sourceStates(),"pools",map("A",pool(actor.getManaPool()),"B",pool(other.getManaPool())),
                    "stack_size",game.getStack().size(),"accepted",accepted,"source_identities",identities(),"priority_player",alias(game.getPhaseHandler().getPriorityPlayer())));
        }
        SpellAbility select(Card host,List<SpellAbility> abilities) {
            Assert.assertSame(host,selected.getHostCard()); Assert.assertEquals(abilities.size(),1); Assert.assertSame(abilities.get(0),selected); abilitySelections++; return selected;
        }
        Map<String, Object> activate(String sourceAlias,String signature) {
            final Card source=sources.get(sourceAlias); selected=null;
            final List<Map<String,Object>> domain=new ArrayList<>();
            for(SpellAbility candidate:source.getManaAbilities()) {
                candidate.setActivatingPlayer(actor);
                final String fixed=candidate.getManaPart().getOrigProduced().replace(" ","");
                final int amount=Integer.parseInt(candidate.getParamOrDefault("Amount","1"));
                final String expanded=fixed.repeat(amount);
                domain.add(map("produced",fixed,"amount",amount,"signature",expanded,"mana_ability",candidate.isManaAbility(),"cost",candidate.getPayCosts().toString()));
                if(expanded.equals(signature)) { Assert.assertNull(selected,"ambiguous fixed ability"); selected=candidate; }
            }
            Assert.assertNotNull(selected,"selected actual fixed ability missing"); Assert.assertTrue(selected.isManaAbility()); Assert.assertTrue(selected.isActivatedAbility());
            Assert.assertFalse(selected.usesTargeting()); Assert.assertFalse(selected.isSpell()); Assert.assertSame(source.getController(),actor);
            int taps=0;
            for(CostPart cost:selected.getPayCosts().getCostParts()) {
                if(cost instanceof CostTap) taps++;
                else { Assert.assertTrue(cost instanceof CostPartMana); Assert.assertTrue(((CostPartMana)cost).getManaCostFor(selected).isZero()); }
            }
            Assert.assertEquals(taps,1);
            final int beforeTap=tapDecisions; final int beforeZero=zeroDecisions; final int beforePayment=zeroPayments;
            final int beforeSelect=abilitySelections; final int beforeEvents=events.size();
            final Map<String,Object> beforePool=pool(actor.getManaPool());
            final boolean canPlay=selected.canPlay();
            // Activated abilities need the actual canPlay legality gate before PlaySpellAbility.
            final boolean accepted=canPlay && PlaySpellAbility.playSpellAbility(actor.getController(),actor,selected);
            final Map<String,Object> added;
            if(accepted) {
                added=produced(selected); Assert.assertEquals(tapDecisions-beforeTap,1); Assert.assertTrue(source.isTapped());
                Assert.assertEquals(abilitySelections-beforeSelect,1); Assert.assertTrue(events.size()>beforeEvents);
            } else {
                added=new LinkedHashMap<>(); final Map<String,Object> afterPool=pool(actor.getManaPool());
                for(char c:COLORS.toCharArray()) added.put(String.valueOf(c),(Integer)afterPool.get(String.valueOf(c))-(Integer)beforePool.get(String.valueOf(c)));
                Assert.assertFalse(canPlay); Assert.assertEquals(tapDecisions,beforeTap); Assert.assertEquals(events.size(),beforeEvents);
            }
            if(eventFailure!=null) throw new AssertionError("Event observation failed",eventFailure);
            final Map<String,Object> action=map("source",sourceAlias,"source_id",source.getId(),"signature",signature,"ability_domain",domain,"can_play",canPlay,
                    "accepted",accepted,"produced",added,"tap_decisions",tapDecisions-beforeTap,"zero_decisions",zeroDecisions-beforeZero,"zero_payments",zeroPayments-beforePayment,
                    "ability_selections",abilitySelections-beforeSelect,"event_start",beforeEvents,"event_end",events.size(),
                    "actual_mana_activation_count",game.getStack().getAbilityActivatedThisTurn().size());
            actions.add(action); return action;
        }
        @Subscribe public void onEvent(GameEvent event) {
            if(eventFailure!=null) return;
            try {
                if(event instanceof GameEventCardTapped tapped) events.add(map("kind","tap","source_id",tapped.card().getId(),"tapped",tapped.tapped(),"stack_size",game.getStack().size(),"pool_A",pool(actor.getManaPool())));
                if(event instanceof GameEventManaPool mana) events.add(map("kind","mana","player_id",mana.player().getId(),"mode",mana.mode().name(),"colors",mana.colors().toString(),"stack_size",game.getStack().size(),"pool_A",pool(actor.getManaPool()),"pool_B",pool(other.getManaPool())));
            } catch(Throwable failure) {eventFailure=failure;}
        }
        Map<String,Object> finish(String id,Object selection,Object production) {
            final Map<String,Object> cards=new LinkedHashMap<>(); for(Map.Entry<String,Card> entry:sources.entrySet()) cards.put(entry.getKey(),entry.getValue().getName());
            return map("id",id,"source_cards",cards,"selected_ability",selection,"produced",production,"snapshots",snapshots,
                    "source_metadata",metadata,"actions",actions,"events",events,"controller_class",actor.getController().getClass().getName(),
                    "cost_visitor_class",StrictCosts.class.getName(),"actor_id",actor.getId(),"other_id",other.getId());
        }
    }
    /** Only exact fixed tap and automatic zero-mana decisions are authorized. */
    private static final class StrictCosts extends CostDecisionMakerBase {
        private final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script) { super(player,false,ability,ability.getHostCard()); this.script=script; }
        @Override public boolean paysRightAfterDecision() { return true; }
        @Override public PaymentDecision visit(CostTap cost) {
            Assert.assertSame(ability,script.selected); Assert.assertSame(player,script.actor); Assert.assertSame(source.getController(),player);
            Assert.assertTrue(cost.canPay(ability,player,false)); script.tapDecisions++; return PaymentDecision.number(0);
        }
        @Override public PaymentDecision visit(CostPartMana cost) {
            Assert.assertTrue(cost.getManaCostFor(ability).isZero()); script.zeroDecisions++; return PaymentDecision.number(0);
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
        @Override public PaymentDecision visit(CostSacrifice cost) { throw new AssertionError("Unexpected cost: CostSacrifice"); }
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
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(hostCard, abilities); }
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
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { Assert.assertSame(sa, script.selected); Assert.assertFalse(effect); Assert.assertTrue(toPay.isZero()); script.zeroPayments++; return true; }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { throw new AssertionError("Unexpected controller operation: applyManaToCost"); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw new AssertionError("Unexpected controller operation: chooseCardsForCost"); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { Assert.assertEquals(player, script.actor); Assert.assertSame(ability, script.selected); Assert.assertFalse(effect); return new StrictCosts(player, ability, script); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw new AssertionError("Unexpected controller operation: chooseSingleCardForZoneChange"); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw new AssertionError("Unexpected controller operation: chooseCardsForZoneChange"); }
        @Override public void autoPassCancel() { /* notification only; no player decision */ }
        @Override public void awaitNextInput() { /* notification only; no player decision */ }
        @Override public void cancelAwaitNextInput() { /* notification only; no player decision */ }
    }
}
