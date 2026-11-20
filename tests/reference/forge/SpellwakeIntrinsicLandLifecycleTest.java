package forge.ai;
import forge.LobbyPlayer;
import forge.card.*;
import forge.deck.Deck;
import forge.game.*;
import forge.game.card.*;
import forge.game.cost.*;
import forge.game.phase.PhaseType;
import forge.game.player.*;
import forge.game.spellability.*;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import forge.util.ITriggerEvent;
import com.google.gson.GsonBuilder;
import org.testng.Assert;
import org.testng.annotations.Test;
import java.nio.file.*;
import java.util.*;

/** Actual strict priority-wrapper land play then intrinsic tap, for all5types.
 * Hands/libraries are explicitly prepared; no deck opening or fullcard equality.
 */
public class SpellwakeIntrinsicLandLifecycleTest extends AITest {
    @Test public void ordinaryLandPlayAndPaidIntrinsicManaForAllFiveTypes() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            String[] types={"Plains","Island","Swamp","Mountain","Forest"},colors={"W","U","B","R","G"};
            for(int i=0;i<5;i++){
                Script script=new Script(types[i],colors[i],rows);script.seed();script.capture("prepared");
                script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
                for(int n=0;n<50&&!script.done;n++){Assert.assertFalse(script.game.isGameOver());script.game.getPhaseHandler().mainLoopStep();Assert.assertTrue(n<49,"priority loop exhausted");}
                Assert.assertTrue(script.done);Assert.assertEquals(script.actions,2);Assert.assertEquals(script.tapDecisions,1);Assert.assertTrue(script.priorities>0);
            }
            passed=true;
        } finally {
            Map<String,Object> value=new LinkedHashMap<>();value.put("assertions_passed",passed);value.put("rows",rows);
            value.put("scope","trusted hands/libraries before actual setupFirstTurn, strict non-AI priority callbacks, actual PlaySpellAbility land and intrinsic mana wrappers with prescribed CostTap payment; no deck opening, completecards or fullruntime closure");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir"))).resolve("normal-intrinsic-land.json"),new GsonBuilder().create().toJson(value)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    private static final class Script {
        final Game game;final Player actor;final String type,color;final List<Map<String,Object>> rows;Card original;SpellAbility selected;
        int actions=0,tapDecisions=0,priorities=0,zeroManaCallbacks=0;boolean done=false;
        Script(String type,String color,List<Map<String,Object>> rows){
            this.type=type;this.color=color;this.rows=rows;List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(players,rules,new Match(rules,players,"Spellwake intrinsic land wrappers"));game.setAge(GameStage.Play);actor=game.getPlayers().get(1);
            for(Player player:game.getPlayers()){((Controller)player.getController()).script=this;Assert.assertFalse(player.getController().isAI());}
        }
        Card seed(Player player,ZoneType zone,String name,String types){
            Card value=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of("Name:Spellwake "+name,"ManaCost:0","Types:"+types,"Oracle:")),"???",CardRarity.Common),player);
            value.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(value);return value;
        }
        void seed(){for(Player player:game.getPlayers())for(int n=0;n<8;n++)seed(player,ZoneType.Library,"Library "+n,"Artifact");original=seed(actor,ZoneType.Hand,type,"Basic Land "+type);}
        Card current(){return game.getCardState(original);}
        void capture(String point){Map<String,Object> row=new LinkedHashMap<>();row.put("type",type);row.put("point",point);row.put("zone",current().getZone().getZoneType().name());row.put("land_types",current().getType().getLandTypes().stream().sorted().toList());row.put("tapped",current().isTapped());row.put("stack",game.getStack().size());row.put("lands_played",actor.getLandsPlayedThisTurn());row.put("pool",actor.getManaPool().getAmountOfColor(MagicColor.fromName(color)));row.put("tap_decisions",tapDecisions);row.put("zero_mana_callbacks",zeroManaCallbacks);row.put("priority_callbacks",priorities);rows.add(row);}
        List<SpellAbility> choose(Player player){
            priorities++;Assert.assertSame(player,game.getPhaseHandler().getPriorityPlayer());
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1||player!=actor||!game.getStack().isEmpty())return null;
            if(actions==2){capture("settled");Assert.assertTrue(current().isTapped());Assert.assertEquals(actor.getManaPool().getAmountOfColor(MagicColor.fromName(color)),1);done=true;return null;}
            if(actions==0){Assert.assertTrue(actor.canPlayLand(current(),false,null));selected=new LandAbility(current(),current().getCurrentState());}
            else {Assert.assertTrue(current().isInPlay());Assert.assertFalse(current().isTapped());Assert.assertEquals(current().getManaAbilities().size(),1);selected=current().getManaAbilities().get(0);Assert.assertTrue(selected.isManaAbility());Assert.assertEquals(selected.getManaPart().getOrigProduced(),color);}
            selected.setActivatingPlayer(actor);Assert.assertTrue(selected.canPlay());return List.of(selected);
        }
        boolean play(Player player,SpellAbility ability){
            Assert.assertSame(player,actor);Assert.assertSame(ability,selected);Assert.assertTrue(ability.canPlay());boolean result=PlaySpellAbility.playSpellAbility(player.getController(),player,ability);
            Assert.assertTrue(current().isInPlay());Assert.assertEquals(actor.getLandsPlayedThisTurn(),1);Assert.assertTrue(game.getStack().isEmpty());
            if(actions==0){Assert.assertFalse(current().isTapped());Assert.assertEquals(actor.getManaPool().getAmountOfColor(MagicColor.fromName(color)),0);}
            else {Assert.assertTrue(current().isTapped());Assert.assertEquals(actor.getManaPool().getAmountOfColor(MagicColor.fromName(color)),1);}
            actions++;capture(actions==1?"land-played":"mana-produced");return result;
        }
    }
    private static final class Lobby extends LobbyPlayer implements IGameEntitiesFactory {
        Lobby(String name){super(name);}
        @Override public Player createIngamePlayer(Game game,int id){Player player=new Player(getName(),game,id);player.setFirstController(new Controller(game,player,this));return player;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw new AssertionError("Unexpected control");}
        @Override public void hear(LobbyPlayer from,String message){throw new AssertionError("Unexpected lobby message");}
    }
    private static final class Controller extends StrictRejectingController {
        Script script;Controller(Game game,Player player,LobbyPlayer lobby){super(game,player,lobby);}
        @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected callback "+method);}
        @Override public List<SpellAbility> chooseSpellAbilityToPlay(){return script.choose(getPlayer());}
        @Override public boolean playChosenSpellAbility(SpellAbility ability){return script.play(getPlayer(),ability);}
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.selected);return script.selected;}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility cause){Assert.assertTrue(cards.size()<=1);return cards;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs){Assert.assertTrue(cost instanceof CostTap || cost instanceof CostPartMana);if(cost instanceof CostPartMana mana)Assert.assertTrue(mana.getManaCostFor(script.selected).isZero());}return costs;}
        @Override public boolean payManaCost(forge.card.mana.ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,forge.game.mana.ManaConversionMatrix matrix,boolean effect){Assert.assertTrue(cost.isZero());Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.zeroManaCallbacks++;return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,false);}
        @Override public boolean applyManaToCost(forge.game.mana.ManaCostBeingPaid cost,SpellAbility ability,String prompt,forge.game.mana.ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);Assert.assertTrue(cost.isPaid());return true;}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(player,script.actor);Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return new StrictRejectingController.Costs(player,ability,false){
            @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}
            @Override public boolean paysRightAfterDecision(){return false;}
            @Override public PaymentDecision visit(CostPartMana cost){Assert.assertTrue(cost.getManaCostFor(ability).isZero());return PaymentDecision.number(0);}
            @Override public PaymentDecision visit(CostTap cost){Assert.assertEquals(script.actions,1);Assert.assertTrue(cost.canPay(ability,player,false));script.tapDecisions++;return PaymentDecision.number(0);}
        };}
    }
}
