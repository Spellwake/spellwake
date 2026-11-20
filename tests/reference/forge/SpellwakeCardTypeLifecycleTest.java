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
import com.google.gson.GsonBuilder;
import org.testng.Assert;
import org.testng.annotations.Test;
import java.nio.file.*;
import java.util.*;

/** Explicit synthetic hands/libraries/prepared source and injected G mana.
 * Actual setupFirstTurn, paid PlaySpellAbility, priority and resolution; no
 * registered deck opening, dev phase selection, direct resolver or AI decision.
 */
public class SpellwakeCardTypeLifecycleTest extends AITest {
    @Test public void normalPaidKindredEnchantmentEntry() throws Exception {run("enchantment");}
    @Test public void normalPaidKindredInstantResolution() throws Exception {run("instant");}
    @Test public void normalPaidKindredSorceryResolution() throws Exception {run("sorcery");}
    private void run(String scenario) throws Exception {
        Script script=new Script(scenario);boolean passed=false;
        try {
            script.seed();script.capture("prepared");script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            for(int step=0;step<50&&!script.done;step++) {
                Assert.assertFalse(script.game.isGameOver());script.game.getPhaseHandler().mainLoopStep();
                Assert.assertTrue(step<49,"bounded priority loop exhausted");
            }
            Assert.assertTrue(script.done);Assert.assertEquals(script.casts,1);
            Assert.assertTrue(script.priorityCalls>0);Assert.assertEquals(script.manaCallbacks,1);
            Assert.assertEquals(script.game.getPhaseHandler().getTurn(),1);passed=true;
        } finally {
            script.capture("final");Map<String,Object> result=new LinkedHashMap<>();
            result.put("scenario",scenario);result.put("assertions_passed",passed);
            result.put("ordinary_paid_wrapper",true);result.put("direct_resolution",false);
            result.put("prepared_mana",true);result.put("mana_activation_compared",false);
            result.put("registered_opening",false);result.put("casts",script.casts);
            result.put("priority_callbacks",script.priorityCalls);result.put("cost_callbacks",script.costCallbacks);
            result.put("mana_callbacks",script.manaCallbacks);result.put("rows",script.rows);
            Path dir=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
            Files.writeString(dir.resolve(scenario+".json"),new GsonBuilder().serializeNulls().create().toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    private static final class Lobby extends LobbyPlayer implements IGameEntitiesFactory {
        Lobby(String name){super(name);}
        @Override public Player createIngamePlayer(Game game,int id){Player p=new Player(getName(),game,id);p.setFirstController(new Controller(game,p,this));return p;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw new AssertionError("Unexpected mind slave");}
        @Override public void hear(LobbyPlayer from,String message){throw new AssertionError("Unexpected lobby message");}
    }
    private static final class Script {
        final String scenario;final Game game;final Player actor;
        final List<Map<String,Object>> rows=new ArrayList<>();Card paid,manaSource;
        SpellAbility selected;int casts,priorityCalls,costCallbacks,manaCallbacks;boolean done;
        Script(String scenario){
            this.scenario=scenario;
            List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(players,rules,new Match(rules,players,"Spellwake normal card-type probes"));
            game.setAge(GameStage.Play);actor=game.getPlayers().get(1);
            for(Player p:game.getPlayers()){((Controller)p.getController()).script=this;Assert.assertFalse(p.getController().isAI());}
        }
        Card card(Player p,ZoneType zone,List<String> lines){
            Card value=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),p);
            value.setGameTimestamp(game.getNextTimestamp());p.getZone(zone).add(value);return value;
        }
        void seed(){
            for(Player p:game.getPlayers())for(int i=0;i<8;i++)card(p,ZoneType.Library,List.of("Name:Spellwake Library "+i,"ManaCost:0","Types:Artifact","Oracle:"));
            manaSource=card(actor,ZoneType.Battlefield,List.of("Name:Spellwake Prepared Mana Source","ManaCost:0","Types:Land","Oracle:"));
            String type=switch(scenario){case "enchantment"->"Enchantment";case "instant"->"Instant";case "sorcery"->"Sorcery";default->throw new AssertionError("Unknown role");};
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake Paid "+scenario,"ManaCost:G","Types:Kindred "+type+" Elf","Oracle:"));
            if(!scenario.equals("enchantment"))lines.add("A:SP$ GainLife | Cost$ G | LifeAmount$ 3");
            paid=card(actor,ZoneType.Hand,lines);
        }
        Card current(){return game.getCardState(paid);}
        void capture(String point){
            Card value=current();Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);
            row.put("zone",value.getZone().getZoneType().name());row.put("types",value.getType().getCoreTypes().stream().map(Enum::name).sorted().toList());
            row.put("creature_types",value.getType().getCreatureTypes().stream().sorted().toList());row.put("creature",value.isCreature());
            row.put("life",actor.getLife());row.put("green",actor.getManaPool().getAmountOfColor(ManaAtom.fromName("G")));
            row.put("stack",game.getStack().size());row.put("turn",game.getPhaseHandler().getTurn());
            row.put("phase",String.valueOf(game.getPhaseHandler().getPhase()));rows.add(row);
        }
        void traits(){Assert.assertTrue(current().isKindred());Assert.assertFalse(current().isCreature());Assert.assertEquals(current().getType().getCreatureTypes(),Set.of("Elf"));}
        List<SpellAbility> choose(Player p){
            priorityCalls++;Assert.assertSame(p,game.getPhaseHandler().getPriorityPlayer());
            if(game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1||p!=actor||!game.getStack().isEmpty())return null;
            if(casts==1){
                capture("settled");traits();
                Assert.assertEquals(current().getZone().getZoneType(),scenario.equals("enchantment")?ZoneType.Battlefield:ZoneType.Graveyard);
                Assert.assertEquals(actor.getLife(),scenario.equals("enchantment")?20:23);
                Assert.assertEquals(actor.getManaPool().getAmountOfColor(ManaAtom.fromName("G")),0);done=true;return null;
            }
            Assert.assertEquals(casts,0);selected=current().getFirstSpellAbility();selected.setActivatingPlayer(actor);
            Assert.assertTrue(selected.canPlay());actor.getManaPool().addMana(new Mana(ManaAtom.fromName("G"),manaSource,null,actor));return List.of(selected);
        }
        boolean play(Player p,SpellAbility ability){
            Assert.assertSame(p,actor);Assert.assertSame(ability,selected);Assert.assertTrue(ability.canPlay());
            Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,ability));Assert.assertEquals(game.getStack().size(),1);
            Assert.assertEquals(current().getZone().getZoneType(),ZoneType.Stack);traits();capture("announced");casts++;return true;
        }
    }
    private static final class Controller extends StrictRejectingController {
        Script script;Controller(Game game,Player p,LobbyPlayer lobby){super(game,p,lobby);}
        @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected callback "+method);}
        @Override public List<SpellAbility> chooseSpellAbilityToPlay(){return script.choose(getPlayer());}
        @Override public boolean playChosenSpellAbility(SpellAbility ability){return script.play(getPlayer(),ability);}
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.selected);return script.selected;}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player payer,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(payer,script.actor);Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.costCallbacks++;return new ManaCosts(payer,ability);}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,false);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){
            Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.manaCallbacks++;
            getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);
            try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(ManaAtom.fromName("G"),ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}
            finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}
        }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
    }
    /** Reuse the shared strict cost boundary; admit only this fixture's mana cost. */
    private static final class ManaCosts extends StrictRejectingController.Costs {
        ManaCosts(Player player,SpellAbility ability){super(player,ability,false);}
        @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost callback "+method);}
        @Override public boolean paysRightAfterDecision(){return true;}
        @Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}
    }
}
