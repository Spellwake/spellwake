package forge.ai;

import forge.LobbyPlayer;
import forge.card.CardRarity;
import forge.card.CardRules;
import forge.card.mana.ManaAtom;
import forge.card.mana.ManaCost;
import forge.deck.Deck;
import forge.game.*;
import forge.game.card.*;
import forge.game.cost.*;
import forge.game.mana.*;
import forge.game.phase.PhaseType;
import forge.game.player.*;
import forge.game.spellability.*;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import forge.util.ITriggerEvent;
import forge.util.collect.FCollectionView;
import com.google.gson.GsonBuilder;
import com.google.common.eventbus.Subscribe;
import forge.game.event.GameEventCardStatsChanged;
import org.testng.Assert;
import org.testng.annotations.Test;
import java.nio.file.*;
import java.util.*;

/** Actual ordinary paid announcement/priority/resolution after an explicit prepared
 * inventory and setupFirstTurn. Synthetic Programs, no registered-deck opening,
 * no dev phase selection, no direct effect resolver, no reference AI decisions.
 */
public class SpellwakeSupertypeLifecycleTest extends AITest {
    @Test public void normalLegendEntryOffersThePrescribedKeep() throws Exception {run("legend-entry");}
    @Test public void normalWorldEntryKeepsTheNewest() throws Exception {run("world-entry");}
    @Test public void normalWorldGainUsesTheActualChangeCut() throws Exception {run("world-gain");}
    @Test public void normalWorldLossRegainRenewsTenure() throws Exception {run("world-regain");}
    @Test public void normalCopyAcquiresWorldAtItsOwnCut() throws Exception {run("world-copy");}
    @Test public void normalPaidLegendaryInstantUsesTheCurrentKeeper() throws Exception {run("legendary-instant");}
    @Test public void normalPaidLegendarySorceryUsesTheCurrentKeeper() throws Exception {run("legendary-sorcery");}
    private void run(String scenario) throws Exception {
        Script script=new Script(scenario);boolean completed=false;
        try {
            script.seed();script.game.subscribeToEvents(script);script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            for(int i=0;i<60&&!script.done;i++) {
                Assert.assertFalse(script.game.isGameOver());script.game.getPhaseHandler().mainLoopStep();
                if(script.observationFailure!=null)throw new AssertionError("Observer failed",script.observationFailure);
                Assert.assertTrue(i<59,"bounded priority loop exhausted");
            }
            Assert.assertTrue(script.done);Assert.assertEquals(script.castCount,script.sequence.size());
            Assert.assertTrue(script.priorityCalls>0);Assert.assertEquals(script.game.getPhaseHandler().getTurn(),1);
            completed=true;
        } finally {
            script.capture("final");
            Map<String,Object> output=new LinkedHashMap<>();output.put("scenario",scenario);output.put("completed",completed);
            output.put("setup","Synthetic hands/libraries, strict non-AI controllers, actual setupFirstTurn; no registered opening");
            output.put("ordinary_paid_wrapper",true);output.put("direct_resolution",false);output.put("dev_phase_selection",false);
            output.put("cast_count",script.castCount);output.put("cost_callbacks",script.costCallbacks);output.put("mana_callbacks",script.manaCallbacks);
            output.put("priority_callbacks",script.priorityCalls);output.put("prescribed_legend_choices",script.legendChoices);output.put("rows",script.rows);
            Path directory=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
            Files.writeString(directory.resolve(scenario+".json"),new GsonBuilder().serializeNulls().create().toJson(output)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    private static final class Lobby extends LobbyPlayer implements IGameEntitiesFactory {
        Lobby(String name){super(name);}
        @Override public Player createIngamePlayer(Game game,int id){Player p=new Player(getName(),game,id);p.setFirstController(new Controller(game,p,this));return p;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw new AssertionError("Unexpected mind slave");}
        @Override public void hear(LobbyPlayer from,String message){throw new AssertionError("Unexpected lobby message");}
    }
    private static final class Script {
        final String scenario;final Game game;final Player actor,other;
        final Map<String,Card> cards=new LinkedHashMap<>();final List<String> sequence=new ArrayList<>();
        final Map<String,String> targets=new LinkedHashMap<>();final List<Map<String,Object>> rows=new ArrayList<>();
        int castCount,validated,priorityCalls,costCallbacks,manaCallbacks,legendChoices;boolean done;
        SpellAbility selected;String selectedAlias;Throwable observationFailure;
        @Subscribe public void stats(GameEventCardStatsChanged event){if(observationFailure!=null)return;try{capture("stats-event:"+selectedAlias);}catch(Throwable failure){observationFailure=failure;}}
        Script(String scenario){
            this.scenario=scenario;List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(players,rules,new Match(rules,players,"Spellwake normal supertype probes"));
            game.setAge(GameStage.Play);actor=game.getPlayers().get(1);other=game.getPlayers().get(0);
            for(Player p:game.getPlayers()){((Controller)p.getController()).script=this;Assert.assertFalse(p.getController().isAI());}
        }
        Card seed(String alias,String name,String types,String... text){
            List<String> lines=new ArrayList<>(List.of("Name:"+name,"ManaCost:0","Types:"+types,"PT:2/3","Oracle:"));lines.addAll(List.of(text));
            Card c=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),actor);
            c.setGameTimestamp(game.getNextTimestamp());actor.getZone(ZoneType.Hand).add(c);Assert.assertNull(cards.put(alias,c));return c;
        }
        Card current(String alias){return game.getCardState(cards.get(alias));}
        void zone(String alias,ZoneType zone){Assert.assertEquals(current(alias).getZone().getZoneType(),zone,alias);}
        void spell(String alias,String definition,String target){seed(alias,"Spellwake "+alias,"Instant","A:"+definition);if(target!=null)targets.put(alias,target);}
        void seed(){
            for(Player p:game.getPlayers())for(int i=0;i<8;i++){
                Card c=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of("Name:Spellwake Library "+i,"ManaCost:0","Types:Artifact","Oracle:")),"???",CardRarity.Common),p);
                c.setGameTimestamp(game.getNextTimestamp());p.getZone(ZoneType.Library).add(c);
            }
            switch(scenario){
                case "legend-entry": seed("first","Spellwake Legend","Legendary Artifact");seed("second","Spellwake Legend","Legendary Artifact");sequence.addAll(List.of("first","second"));break;
                case "world-entry": seed("first","Spellwake First World","World Artifact");seed("second","Spellwake Second World","World Artifact");sequence.addAll(List.of("first","second"));break;
                case "world-gain":
                    seed("receiver","Spellwake Receiver","Artifact");seed("world","Spellwake World","World Artifact");
                    spell("gain","SP$ Animate | Cost$ 0 | ValidTgts$ Artifact | Types$ World | Duration$ Permanent","receiver");sequence.addAll(List.of("receiver","world","gain"));break;
                case "world-regain":
                    seed("receiver","Spellwake Receiver","World Artifact");seed("world","Spellwake World","World Artifact");
                    spell("loss","SP$ Animate | Cost$ 0 | ValidTgts$ Artifact | RemoveTypes$ World | Duration$ Permanent","receiver");
                    spell("gain","SP$ Animate | Cost$ 0 | ValidTgts$ Artifact | Types$ World | Duration$ Permanent","receiver");sequence.addAll(List.of("receiver","loss","world","gain"));break;
                case "world-copy":
                    seed("world","Spellwake World","World Artifact");seed("receiver","Spellwake Receiver","Artifact","A:AB$ Clone | Cost$ 0 | ValidTgts$ Artifact | Duration$ Permanent");
                    targets.put("activate","world");sequence.addAll(List.of("world","receiver","activate"));break;
                case "legendary-instant":case "legendary-sorcery":
                    seed("keeper","Spellwake Keeper","Legendary Creature Elf");seed("paid","Spellwake Paid",scenario.equals("legendary-instant")?"Legendary Instant":"Legendary Sorcery","A:SP$ GainLife | Cost$ G | LifeAmount$ 3");sequence.addAll(List.of("keeper","paid"));break;
                default:throw new AssertionError("Unknown scenario");
            }
        }
        void capture(String checkpoint){
            Map<String,Object> row=new LinkedHashMap<>();row.put("after",checkpoint);row.put("turn",game.getPhaseHandler().getTurn());row.put("phase",game.getPhaseHandler().getPhase().name());row.put("stack",game.getStack().size());
            Map<String,Object> values=new LinkedHashMap<>();for(String alias:cards.keySet())values.put(alias,Map.of("zone",current(alias).getZone().getZoneType().name(),"world_age",current(alias).getWorldTimestamp(),"game_timestamp",current(alias).getGameTimestamp(),"legendary",current(alias).getType().isLegendary(),"world",current(alias).getType().hasSupertype(forge.card.CardType.Supertype.World)));row.put("cards",values);row.put("life",actor.getLife());row.put("green",actor.getManaPool().getAmountOfColor(ManaAtom.fromName("G")));row.put("assertions_passed",false);rows.add(row);
        }
        void observe(){
            String previous=sequence.get(validated);capture(previous);zone(previous.equals("activate")?"receiver":previous,
                previous.equals("gain")||previous.equals("loss")||previous.equals("paid")?ZoneType.Graveyard:ZoneType.Battlefield);
            if(scenario.equals("legend-entry")&&previous.equals("second")){zone("first",ZoneType.Graveyard);Assert.assertEquals(legendChoices,1);}
            if(scenario.equals("world-entry")&&previous.equals("second")){zone("first",ZoneType.Graveyard);Assert.assertTrue(current("second").getWorldTimestamp()>=0);}
            if((scenario.equals("world-gain")||scenario.equals("world-regain"))&&previous.equals("gain")){
                zone("receiver",ZoneType.Battlefield);zone("world",ZoneType.Graveyard);Assert.assertTrue(current("receiver").getType().hasSupertype(forge.card.CardType.Supertype.World));Assert.assertTrue(current("receiver").getWorldTimestamp()>=0);
            }
            if(scenario.equals("world-regain")&&previous.equals("loss")){zone("receiver",ZoneType.Battlefield);Assert.assertFalse(current("receiver").getType().hasSupertype(forge.card.CardType.Supertype.World));}
            if(scenario.equals("world-copy")&&previous.equals("activate")){zone("world",ZoneType.Graveyard);zone("receiver",ZoneType.Battlefield);Assert.assertTrue(current("receiver").getWorldTimestamp()>=0);}
            if(previous.equals("paid")){Assert.assertEquals(actor.getLife(),23);Assert.assertEquals(actor.getManaPool().getAmountOfColor(ManaAtom.fromName("G")),0);Assert.assertEquals(manaCallbacks,1);}
            rows.get(rows.size()-1).put("assertions_passed",true);validated++;
        }
        List<SpellAbility> choose(Player p){
            priorityCalls++;Assert.assertSame(p,game.getPhaseHandler().getPriorityPlayer());
            if(game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1||p!=actor)return null;
            if(!game.getStack().isEmpty())return null;
            if(validated<castCount)observe();
            if(castCount==sequence.size()){done=true;return null;}
            selectedAlias=sequence.get(castCount);Card host=current(selectedAlias.equals("activate")?"receiver":selectedAlias);
            selected=selectedAlias.equals("activate")?host.getNonManaAbilities().stream().filter(SpellAbility::isActivatedAbility).findFirst().orElseThrow():host.getFirstSpellAbility();selected.setActivatingPlayer(actor);
            if(selectedAlias.equals("keeper")){SpellAbility blocked=current("paid").getFirstSpellAbility();blocked.setActivatingPlayer(actor);Assert.assertFalse(blocked.canPlay());}
            if(selectedAlias.equals("paid")){Assert.assertTrue(selected.canPlay());actor.getManaPool().addMana(new Mana(ManaAtom.fromName("G"),current("keeper"),null,actor));}
            Assert.assertTrue(selected.canPlay());return List.of(selected);
        }
        boolean play(Player p,SpellAbility ability){
            Assert.assertSame(p,actor);Assert.assertSame(ability,selected);Assert.assertTrue(ability.canPlay());Assert.assertTrue(game.getStack().isEmpty());
            Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,ability));Assert.assertEquals(game.getStack().size(),1);
            if(!selectedAlias.equals("activate"))zone(selectedAlias,ZoneType.Stack);castCount++;return true;
        }
    }
    private static final class Controller extends StrictRejectingController {
        Script script;
        Controller(Game game,Player p,LobbyPlayer lobby){super(game,p,lobby);}
        @Override protected AssertionError reject(String method,Object... args){return new AssertionError("Unexpected callback "+method);}
        @Override public List<SpellAbility> chooseSpellAbilityToPlay(){return script.choose(getPlayer());}
        @Override public boolean playChosenSpellAbility(SpellAbility ability){return script.play(getPlayer(),ability);}
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.selected);return script.selected;}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());}
        @Override public boolean chooseTargetsFor(SpellAbility ability){Assert.assertSame(ability,script.selected);Card target=script.current(Objects.requireNonNull(script.targets.get(script.selectedAlias)));Assert.assertTrue(ability.canTarget(target));ability.getTargets().add(target);Assert.assertTrue(ability.isTargetNumberValid());return true;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player payer,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(payer,script.actor);Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.costCallbacks++;return new ManaCosts(payer,ability);}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,false);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);Assert.assertEquals(script.selectedAlias,"paid");script.manaCallbacks++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(ManaAtom.fromName("G"),ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}}
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> offered,DelayedReveal reveal,SpellAbility ability,String title,boolean optional,Player related,Map<String,Object> params){Assert.assertEquals(script.scenario,"legend-entry");Assert.assertEquals(offered.size(),2);Set<Integer> ids=new TreeSet<>();for(T entity:offered)ids.add(entity.getId());Assert.assertEquals(ids,Set.of(script.current("first").getId(),script.current("second").getId()));script.legendChoices++;for(T entity:offered)if(entity.getId()==script.current("second").getId())return entity;throw reject("missing prescribed keep");}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
    }
    /** Only mana costs are admitted; every other visitor leaf is fatal. */
    private static final class ManaCosts extends CostDecisionMakerBase {
        ManaCosts(Player player,SpellAbility ability){super(player,false,ability,ability.getHostCard());}
        @Override public boolean paysRightAfterDecision(){return true;}
        @Override public PaymentDecision visit(CostBehold cost) {throw new AssertionError("Unexpected cost CostBehold");}
        @Override public PaymentDecision visit(CostBeholdExile cost) {throw new AssertionError("Unexpected cost CostBeholdExile");}
        @Override public PaymentDecision visit(CostGainControl cost) {throw new AssertionError("Unexpected cost CostGainControl");}
        @Override public PaymentDecision visit(CostChooseColor cost) {throw new AssertionError("Unexpected cost CostChooseColor");}
        @Override public PaymentDecision visit(CostChooseCreatureType cost) {throw new AssertionError("Unexpected cost CostChooseCreatureType");}
        @Override public PaymentDecision visit(CostCollectEvidence cost) {throw new AssertionError("Unexpected cost CostCollectEvidence");}
        @Override public PaymentDecision visit(CostDiscard cost) {throw new AssertionError("Unexpected cost CostDiscard");}
        @Override public PaymentDecision visit(CostDamage cost) {throw new AssertionError("Unexpected cost CostDamage");}
        @Override public PaymentDecision visit(CostDraw cost) {throw new AssertionError("Unexpected cost CostDraw");}
        @Override public PaymentDecision visit(CostExile cost) {throw new AssertionError("Unexpected cost CostExile");}
        @Override public PaymentDecision visit(CostExileFromStack cost) {throw new AssertionError("Unexpected cost CostExileFromStack");}
        @Override public PaymentDecision visit(CostExiledMoveToGrave cost) {throw new AssertionError("Unexpected cost CostExiledMoveToGrave");}
        @Override public PaymentDecision visit(CostExert cost) {throw new AssertionError("Unexpected cost CostExert");}
        @Override public PaymentDecision visit(CostEnlist cost) {throw new AssertionError("Unexpected cost CostEnlist");}
        @Override public PaymentDecision visit(CostFlipCoin cost) {throw new AssertionError("Unexpected cost CostFlipCoin");}
        @Override public PaymentDecision visit(CostForage cost) {throw new AssertionError("Unexpected cost CostForage");}
        @Override public PaymentDecision visit(CostRollDice cost) {throw new AssertionError("Unexpected cost CostRollDice");}
        @Override public PaymentDecision visit(CostMill cost) {throw new AssertionError("Unexpected cost CostMill");}
        @Override public PaymentDecision visit(CostAddMana cost) {throw new AssertionError("Unexpected cost CostAddMana");}
        @Override public PaymentDecision visit(CostPayLife cost) {throw new AssertionError("Unexpected cost CostPayLife");}
        @Override public PaymentDecision visit(CostPayEnergy cost) {throw new AssertionError("Unexpected cost CostPayEnergy");}
        @Override public PaymentDecision visit(CostGainLife cost) {throw new AssertionError("Unexpected cost CostGainLife");}
        @Override public PaymentDecision visit(CostPartMana cost) {return PaymentDecision.number(0);}
        @Override public PaymentDecision visit(CostPromiseGift cost) {throw new AssertionError("Unexpected cost CostPromiseGift");}
        @Override public PaymentDecision visit(CostPutCardToLib cost) {throw new AssertionError("Unexpected cost CostPutCardToLib");}
        @Override public PaymentDecision visit(CostTap cost) {throw new AssertionError("Unexpected cost CostTap");}
        @Override public PaymentDecision visit(CostSacrifice cost) {throw new AssertionError("Unexpected cost CostSacrifice");}
        @Override public PaymentDecision visit(CostReturn cost) {throw new AssertionError("Unexpected cost CostReturn");}
        @Override public PaymentDecision visit(CostReveal cost) {throw new AssertionError("Unexpected cost CostReveal");}
        @Override public PaymentDecision visit(CostRevealChosen cost) {throw new AssertionError("Unexpected cost CostRevealChosen");}
        @Override public PaymentDecision visit(CostRemoveAnyCounter cost) {throw new AssertionError("Unexpected cost CostRemoveAnyCounter");}
        @Override public PaymentDecision visit(CostRemoveCounter cost) {throw new AssertionError("Unexpected cost CostRemoveCounter");}
        @Override public PaymentDecision visit(CostPutCounter cost) {throw new AssertionError("Unexpected cost CostPutCounter");}
        @Override public PaymentDecision visit(CostPutCounterYou cost) {throw new AssertionError("Unexpected cost CostPutCounterYou");}
        @Override public PaymentDecision visit(CostUntapType cost) {throw new AssertionError("Unexpected cost CostUntapType");}
        @Override public PaymentDecision visit(CostUntap cost) {throw new AssertionError("Unexpected cost CostUntap");}
        @Override public PaymentDecision visit(CostUnattach cost) {throw new AssertionError("Unexpected cost CostUnattach");}
        @Override public PaymentDecision visit(CostTapType cost) {throw new AssertionError("Unexpected cost CostTapType");}
        @Override public PaymentDecision visit(CostPayShards cost) {throw new AssertionError("Unexpected cost CostPayShards");}
        @Override public PaymentDecision visit(CostBlight cost) {throw new AssertionError("Unexpected cost CostBlight");}
    }
}
