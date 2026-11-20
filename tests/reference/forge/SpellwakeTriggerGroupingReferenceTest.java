package forge.ai;

import com.google.common.eventbus.Subscribe;
import com.google.gson.GsonBuilder;
import forge.LobbyPlayer;
import forge.card.*;
import forge.card.mana.*;
import forge.deck.Deck;
import forge.game.*;
import forge.game.ability.*;
import forge.game.card.*;
import forge.game.cost.*;
import forge.game.event.GameEventTokenCreated;
import forge.game.mana.*;
import forge.game.phase.PhaseType;
import forge.game.player.*;
import forge.game.spellability.*;
import forge.game.trigger.*;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import forge.util.ITriggerEvent;
import java.nio.file.*;
import java.util.*;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Normal paid grouped-trigger resolution. Synthetic seeds, injected funding and narrow
 * observer projections are explicit; this does not certify full cards/runtime.
 */
public class SpellwakeTriggerGroupingReferenceTest extends AITest {
    @Test public void normalPaidTogetherEntry() throws Exception { run("entry"); }
    @Test public void normalPaidCreatedOnce() throws Exception { run("created"); }
    @Test public void normalPaidCountedDraw() throws Exception { run("count"); }
    @Test public void normalPaidSeparateBatches() throws Exception { run("separate"); }
    private void run(String scenario) throws Exception {
        Script s=new Script(scenario); boolean passed=false;
        try {
            s.seed(); s.capture("prepared"); s.game.getPhaseHandler().setupFirstTurn(s.actor,null);
            for(int n=0;n<180&&!s.done;n++) { Assert.assertFalse(s.game.isGameOver()); s.game.getPhaseHandler().mainLoopStep(); }
            Assert.assertTrue(s.done,"normal priority loop did not settle"); Assert.assertNull(s.observerFailure,"subscriber failure must fail the scenario"); Assert.assertEquals(s.casts,1); Assert.assertEquals(s.payments,1); Assert.assertEquals(s.triggerZeroPayments,s.bodies); passed=true;
        } finally {
            Map<String,Object> result=new LinkedHashMap<>(); result.put("scenario",scenario);result.put("assertions_passed",passed);
            result.put("casts",s.casts);result.put("payments",s.payments);result.put("trigger_bodies",s.bodies);result.put("trigger_zero_payments",s.triggerZeroPayments);result.put("body_zero_payments",s.bodyZeroPayments);result.put("token_game_events",s.birthEvents);
            result.put("observer_failure",s.observerFailure==null?null:s.observerFailure.toString());result.put("rows",s.rows);result.put("placements",s.placements);result.put("source_related_pairs",s.sourceRelatedPairs);result.put("scope","normal paid grouped synthetic resolution; actual triggering Cards/Amount and mandatory body callbacks; prepared libraries/funding and reused runtime/resources; no participant callback/full-card/numerical ID/performance parity");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir"))).resolve(scenario+".json"),new GsonBuilder().serializeNulls().create().toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    private static final class Script {
        final String scenario; final Game game; final Player actor,other;
        final List<Map<String,Object>> rows=new ArrayList<>(),placements=new ArrayList<>();
        final Set<String> sourceRelatedPairs=new LinkedHashSet<>();
        final Set<SpellAbility> allowedBodies=Collections.newSetFromMap(new IdentityHashMap<>());
        Throwable observerFailure; Card original,observer; SpellAbility selected,admittingTrigger,resolvingBody; int casts,payments,bodies,birthEvents,triggerZeroPayments,bodyZeroPayments; boolean done;
        Script(String scenario) {
            this.scenario=scenario;
            List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(players,rules,new Match(rules,players,"Spellwake grouped trigger comparison"));game.setAge(GameStage.Play);
            actor=game.getPlayers().get(1);other=game.getPlayers().get(0);
            for(Player player:game.getPlayers()) { ((Controller)player.getController()).script=this;Assert.assertFalse(player.getController().isAI()); }
            game.subscribeToEvents(new Events(this));
        }
        Card seed(Player player,ZoneType zone,String name,String types,String cost,String... extra) {
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:"+cost,"Types:"+types,"Oracle:"));lines.addAll(List.of(extra));
            Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);card.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(card);return card;
        }
        ApiType bodyApi(){return scenario.equals("count")?ApiType.Draw:ApiType.GainLife;}
        String[] watcher() {
            String trigger=scenario.equals("created")?"T:Mode$ TokenCreatedOnce | ValidToken$ Creature.YouOwn":"T:Mode$ ChangesZoneAll | Origin$ Any | Destination$ Battlefield | ValidCards$ Creature.YouCtrl";
            trigger+=" | TriggerZones$ Battlefield | Execute$ BatchBody | TriggerDescription$ Whenever one or more qualifying creatures enter or are created, execute the mandatory body.";
            return scenario.equals("count")?new String[]{trigger,"SVar:BatchBody:DB$ Draw | Defined$ You | NumCards$ X","SVar:X:TriggerCount$Amount"}:new String[]{trigger,"SVar:BatchBody:DB$ GainLife | Defined$ You | LifeAmount$ 1"};
        }
        SpellAbility expectedAbility(){return resolvingBody!=null?resolvingBody:admittingTrigger!=null?admittingTrigger:selected;}
        void seed() {
            for(Player p:game.getPlayers()) for(int n=0;n<24;n++) seed(p,ZoneType.Library,"Library "+n,"Artifact","0");
            String ability="A:SP$ Token | Cost$ G | TokenAmount$ 2 | TokenScript$ g_1_1_saproling | TokenOwner$ You";
            List<String> extra=new ArrayList<>();
            if(scenario.equals("separate")){ability+=" | SubAbility$ Second";extra.add("SVar:Second:DB$ Token | TokenAmount$ 2 | TokenScript$ g_1_1_saproling | TokenOwner$ You");}
            extra.add(ability); original=seed(actor,ZoneType.Hand,"Producer","Instant","G",extra.toArray(String[]::new));
            observer=seed(actor,ZoneType.Battlefield,"Observer","Artifact","0",watcher());
        }
        List<Card> tokens() {List<Card> values=new ArrayList<>();for(Card c:game.getCardsIn(ZoneType.Battlefield))if(c.isToken())values.add(c);return values;}
        Map<String,Object> card(Card c) {Map<String,Object> v=new LinkedHashMap<>();v.put("id",c.getId());v.put("owner",c.getOwner().getName());v.put("controller",c.getController().getName());v.put("token",c.isToken());v.put("name",c.getName());v.put("timestamp",c.getGameTimestamp());v.put("types",c.getType().toString());v.put("power",c.getNetPower());v.put("toughness",c.getNetToughness());return v;}
        void capture(String point) {Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);row.put("stack",game.getStack().size());row.put("tokens",tokens().stream().map(this::card).toList());row.put("life_actor",actor.getLife());row.put("trigger_bodies",bodies);row.put("hand_actor",actor.getCardsIn(ZoneType.Hand).size());row.put("library_actor",actor.getCardsIn(ZoneType.Library).size());rows.add(row);}
        void tokenEvent() throws Exception {
            birthEvents++;capture("token-effect-event-after-table-publication");
            var field=TriggerHandler.class.getDeclaredField("waitingTriggers");field.setAccessible(true);Object value=field.get(game.getTriggerHandler());Assert.assertTrue(value instanceof List<?>);
            List<Map<String,Object>> waiting=new ArrayList<>();int individual=0;
            for(Object item:(List<?>)value) {Assert.assertTrue(item instanceof TriggerWaiting);TriggerWaiting w=(TriggerWaiting)item;Map<String,Object> row=new LinkedHashMap<>();row.put("mode",w.getMode().name());row.put("origin",String.valueOf(w.getParams().get(AbilityKey.Origin)));row.put("destination",String.valueOf(w.getParams().get(AbilityKey.Destination)));Object c=w.getParams().get(AbilityKey.Card);if(c instanceof Card card)row.put("card",card(card));waiting.add(row);if(w.getMode()==TriggerType.ChangesZone&&"Battlefield".equals(row.get("destination")))individual++;}
            rows.add(Map.of("point","actual-waiting-queue-before-normal-drain","records",waiting,"individual_entries",individual));
            Assert.assertEquals(individual,tokens().size());
        }
        List<SpellAbility> choose(Player player) {
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            if(casts==0&&player==actor&&game.getStack().isEmpty()) {selected=game.getCardState(original).getFirstSpellAbility();selected.setActivatingPlayer(player);Assert.assertTrue(selected.canPlay());player.getManaPool().addMana(new Mana(MagicColor.GREEN,selected.getHostCard(),null,player));return List.of(selected);}
            if(casts==1&&game.getStack().isEmpty()) {
                int expectedBodies=scenario.equals("separate")?2:1;
                Assert.assertEquals(tokens().size(),scenario.equals("separate")?4:2);Assert.assertEquals(bodies,expectedBodies);Assert.assertEquals(actor.getLife(),scenario.equals("count")?20:20+expectedBodies);
                Set<Integer> ids=new HashSet<>();for(Card c:tokens()){Assert.assertTrue(ids.add(c.getId()));Assert.assertSame(c.getOwner(),actor);Assert.assertSame(c.getController(),actor);Assert.assertEquals(c.getNetPower(),1);Assert.assertEquals(c.getNetToughness(),1);}
                Set<String> expectedPairs=new HashSet<>();for(Card related:tokens())expectedPairs.add(observer.getId()+":"+related.getId());Assert.assertEquals(sourceRelatedPairs,expectedPairs);Assert.assertEquals(birthEvents,expectedBodies);
                Map<String,Object> paid=rows.stream().filter(r->"paid-cast".equals(r.get("point"))).findFirst().orElseThrow();
                int draws=scenario.equals("count")?2:0;Assert.assertEquals(actor.getCardsIn(ZoneType.Hand).size(),((Number)paid.get("hand_actor")).intValue()+draws);Assert.assertEquals(actor.getCardsIn(ZoneType.Library).size(),((Number)paid.get("library_actor")).intValue()-draws);
                Assert.assertEquals(placements.size(),expectedBodies);for(Map<String,Object> placement:placements)Assert.assertEquals(((List<?>)placement.get("members")).size(),2);
                capture("settled-after-normal-trigger-drain-and-resolution");done=true;
            }
            return null;
        }
        boolean play(Player player,SpellAbility ability) {Assert.assertSame(ability,selected);Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));Assert.assertEquals(player.getManaPool().getAmountOfColor(MagicColor.GREEN),0);casts++;capture("paid-cast");return true;}
        void place(Player player,List<SpellAbility> offered) {
            if(offered.isEmpty())return;Assert.assertTrue(birthEvents>0);
            for(SpellAbility ability:offered) {
                Assert.assertTrue(ability instanceof WrappedAbility);WrappedAbility w=(WrappedAbility)ability;
                Assert.assertFalse(w.isOptionalTrigger());Assert.assertFalse(w.usesTargeting());Assert.assertSame(w.getActivatingPlayer(),player);
                Assert.assertEquals(w.getTrigger().getMode(),scenario.equals("created")?TriggerType.TokenCreatedOnce:TriggerType.ChangesZoneAll);Assert.assertEquals(w.getWrappedAbility().getApi(),bodyApi());
                allowedBodies.add(w.getWrappedAbility());Object captured=w.getTriggeringObject(AbilityKey.Cards);Assert.assertTrue(captured instanceof Iterable<?>);
                List<Map<String,Object>> members=new ArrayList<>();Set<Integer> ids=new HashSet<>();
                for(Object item:(Iterable<?>)captured){Assert.assertTrue(item instanceof Card);Card related=(Card)item;Assert.assertTrue(related.isToken());Assert.assertTrue(ids.add(related.getId()));Assert.assertTrue(sourceRelatedPairs.add(w.getHostCard().getId()+":"+related.getId()));members.add(card(related));}
                Assert.assertEquals(members.size(),2);Object amount=w.getTriggeringObject(AbilityKey.Amount);if(!scenario.equals("created"))Assert.assertEquals(amount,2);else Assert.assertNull(amount);
                Map<String,Object> placement=new LinkedHashMap<>();placement.put("source",card(w.getHostCard()));placement.put("controller",player.getName());placement.put("mode",w.getTrigger().getMode().name());placement.put("members",members);placement.put("amount",amount);placement.put("trigger",w.getTrigger().getId());placement.put("stack_before",game.getStack().size());placements.add(placement);
                admittingTrigger=w;try{Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,w));}finally{admittingTrigger=null;}
            }
            capture("normal-trigger-cohort-admitted");
        }
    }
    public static final class Events {final Script script;Events(Script script){this.script=script;}@Subscribe public void token(GameEventTokenCreated event) {try{script.tokenEvent();}catch(Throwable error){script.observerFailure=error;}}}
    private static final class Lobby extends LobbyPlayer implements IGameEntitiesFactory {
        Lobby(String name){super(name);}
        @Override public Player createIngamePlayer(Game game,int id){Player player=new Player(getName(),game,id);player.setFirstController(new Controller(game,player,this));return player;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw new AssertionError("Unexpected control");}
        @Override public void hear(LobbyPlayer from,String message){throw new AssertionError("Unexpected message");}
    }
    private static final class Controller extends StrictRejectingController {
        Script script;Controller(Game game,Player player,LobbyPlayer lobby){super(game,player,lobby);}
        @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected callback "+method);}
        @Override public List<SpellAbility> chooseSpellAbilityToPlay(){return script.choose(getPlayer());}
        @Override public boolean playChosenSpellAbility(SpellAbility ability){return script.play(getPlayer(),ability);}
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);SpellAbility expected=script.expectedAbility();Assert.assertSame(abilities.get(0),expected);return expected;}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){script.place(getPlayer(),abilities);}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void playSpellAbilityNoStack(SpellAbility ability,boolean setup) {Assert.assertFalse(setup);Assert.assertTrue(script.allowedBodies.remove(ability));Assert.assertEquals(ability.getApi(),script.bodyApi());script.bodies++;script.resolvingBody=ability;try{Assert.assertTrue(PlaySpellAbility.playSpellAbilityNoStack(this,getPlayer(),ability,true));}finally{script.resolvingBody=null;}script.capture("mandatory-body-return");}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.expectedAbility());Assert.assertEquals(effect,ability.isTrigger());if(ability!=script.selected){Assert.assertTrue(cost.isZero());if(script.resolvingBody==ability)script.bodyZeroPayments++;else script.triggerZeroPayments++;}return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,effect);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.expectedAbility());Assert.assertEquals(effect,ability.isTrigger());Assert.assertSame(ability,script.selected);script.payments++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(MagicColor.GREEN,ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(ability,script.expectedAbility());Assert.assertEquals(effect,ability.isTrigger());return new StrictRejectingController.Costs(player,ability,effect){@Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}@Override public boolean paysRightAfterDecision(){return true;}@Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}};}
    }
}
