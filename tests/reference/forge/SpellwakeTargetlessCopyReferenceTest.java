package forge.ai;

import forge.LobbyPlayer;
import forge.card.*;
import forge.card.mana.*;
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

/** Synthetic hands/libraries/BF and injected mana; actual paid casting, priority,
 * copy effect and resolution. Copied placement is a prescribed non-AI controller
 * callback using the same empty-target placement sequence as the human adapter.
 * This does not qualify opening, complete cards or reused runtime/resource origin.
 */
public class SpellwakeTargetlessCopyReferenceTest extends AITest {
    @Test public void normalPaidTargetlessCopyUsesDifferentController() throws Exception {run("different-controller");}
    @Test public void normalRepeatedTargetlessCopies() throws Exception {run("repeated");}
    @Test public void normalCopyOfCopySurvivesOriginalCountering() throws Exception {run("copy-of-copy-counter");}
    @Test public void normalCopiesMakeFreshResolutionTypeChoices() throws Exception {run("fresh-values");}
    private void run(String scenario) throws Exception {
        Script script=new Script(scenario);boolean passed=false;
        try {
            script.seed();script.capture("prepared");script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            for(int n=0;n<100&&!script.done;n++) {
                Assert.assertFalse(script.game.isGameOver());script.game.getPhaseHandler().mainLoopStep();Assert.assertTrue(n<99,"bounded priority exhausted");
            }
            Assert.assertTrue(script.done);Assert.assertTrue(script.priorities>0);
            Assert.assertEquals(script.manaPayments,script.actions);passed=true;
        } finally {
            Map<String,Object> value=new LinkedHashMap<>();value.put("scenario",scenario);value.put("assertions_passed",passed);
            value.put("casts",script.actions);value.put("mana_payments",script.manaPayments);value.put("copy_callbacks",script.copyCallbacks);value.put("target_choices",script.targetChoices);
            value.put("type_choices",script.typeChoices);value.put("priority_callbacks",script.priorities);value.put("rows",script.rows);
            value.put("scope","actual normal paid casts/priority/resolve; synthetic hand/library/BF and injected G at each cast; prescribed empty-target copied placement; repeated uses Amount2 batch rather than native two separate instructions; no opening/fullcard/numericjoin/runtimeclosure");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir"))).resolve(scenario+".json"),new GsonBuilder().serializeNulls().create().toJson(value)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    private static final class Script {
        final String scenario;final Game game;final Player actor,other;
        final List<Map<String,Object>> rows=new ArrayList<>();final List<SpellAbility> copies=new ArrayList<>();
        final List<String> typeChoices=new ArrayList<>();final Map<Player,Integer> libraryBaseline=new HashMap<>();
        Card original,firstCopier,secondCopier,counter;SpellAbility originalAbility,selected,target;
        int actions,priorities,manaPayments,copyCallbacks,targetChoices;boolean done;
        Script(String scenario){
            this.scenario=scenario;List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(players,rules,new Match(rules,players,"Spellwake targetless copying"));
            game.setAge(GameStage.Play);actor=game.getPlayers().get(1);other=game.getPlayers().get(0);
            for(Player player:game.getPlayers()){((Controller)player.getController()).script=this;Assert.assertFalse(player.getController().isAI());}
        }
        Card seed(Player player,ZoneType zone,String name,String types,String cost,String... extra){
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:"+cost,"Types:"+types,"Oracle:"));lines.addAll(List.of(extra));
            Card value=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);
            value.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(value);return value;
        }
        void seed(){
            for(Player p:game.getPlayers())for(int n=0;n<24;n++)seed(p,ZoneType.Library,"Library "+n,"Artifact","0");
            seed(actor,ZoneType.Battlefield,"Elf","Creature Elf","0","PT:1/1");
            seed(other,ZoneType.Battlefield,"Goblin One","Creature Goblin","0","PT:1/1");seed(other,ZoneType.Battlefield,"Goblin Two","Creature Goblin","0","PT:1/1");
            if(scenario.equals("fresh-values"))original=seed(actor,ZoneType.Hand,"Targetless","Snow Kindred Instant Elf","G",
                "A:SP$ ChooseType | Cost$ G | Defined$ You | Type$ Creature | SubAbility$ DBDraw",
                "SVar:DBDraw:DB$ Draw | Defined$ You | NumCards$ X","SVar:X:Count$Valid Permanent.YouCtrl+ChosenType");
            else original=seed(actor,ZoneType.Hand,"Targetless","Snow Kindred Instant Elf","G",
                "A:SP$ GainLife | Cost$ G | Defined$ You | LifeAmount$ 3 | SubAbility$ DBDraw","SVar:DBDraw:DB$ Draw | Defined$ You | NumCards$ 1");
            firstCopier=seed(other,ZoneType.Hand,"First Copier","Instant","G","A:SP$ CopySpellAbility | Cost$ G | ValidTgts$ Spell | Controller$ You | Amount$ "+(scenario.equals("repeated")?2:1));
            if(scenario.equals("copy-of-copy-counter")){
                secondCopier=seed(actor,ZoneType.Hand,"Second Copier","Instant","G","A:SP$ CopySpellAbility | Cost$ G | ValidTgts$ Spell | Controller$ You");
                counter=seed(actor,ZoneType.Hand,"Counter","Instant","G","A:SP$ Counter | Cost$ G | ValidTgts$ Spell");
            }
        }
        Card current(Card card){return game.getCardState(card);}
        void traits(SpellAbility ability){Card card=ability.getHostCard();Assert.assertTrue(card.isKindred());Assert.assertTrue(card.isInstant());Assert.assertFalse(card.isCreature());Assert.assertTrue(card.getType().hasSupertype(forge.card.CardType.Supertype.Snow));Assert.assertEquals(card.getType().getCreatureTypes(),Set.of("Elf"));Assert.assertEquals(ability.getTargets().size(),0);}
        void capture(String point){
            Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);row.put("actions",actions);row.put("stack",game.getStack().size());row.put("copied_ids",copies.stream().map(a->a.getHostCard().getId()).toList());
            row.put("life_a",actor.getLife());row.put("life_b",other.getLife());row.put("library_a",actor.getCardsIn(ZoneType.Library).size());row.put("library_b",other.getCardsIn(ZoneType.Library).size());
            row.put("pool_a",actor.getManaPool().getAmountOfColor(ManaAtom.fromName("G")));row.put("pool_b",other.getManaPool().getAmountOfColor(ManaAtom.fromName("G")));
            row.put("copied_objects",copies.stream().map(a->Map.of("id",a.getHostCard().getId(),"copied",a.isCopied(),"owner",a.getHostCard().getOwner().getName(),"controller",a.getActivatingPlayer().getName(),"zone",a.getHostCard().getZone()==null?"none":a.getHostCard().getZone().getZoneType().name())).toList());row.put("original_zone",current(original).getZone().getZoneType().name());row.put("type_choices",new ArrayList<>(typeChoices));rows.add(row);
        }
        List<SpellAbility> choose(Player player){
            priorities++;Assert.assertSame(player,game.getPhaseHandler().getPriorityPlayer());
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            Card next=null;target=null;
            if(actions==0&&player==actor&&game.getStack().isEmpty()){
                for(Player p:game.getPlayers())libraryBaseline.put(p,p.getCardsIn(ZoneType.Library).size());next=current(original);
            }else if(actions==1&&player==other&&game.getStack().size()==1){next=current(firstCopier);target=originalAbility;}
            else if(scenario.equals("copy-of-copy-counter")&&actions==2&&player==actor&&copies.size()==1&&current(firstCopier).isInZone(ZoneType.Graveyard)){next=current(secondCopier);target=copies.get(0);}
            else if(scenario.equals("copy-of-copy-counter")&&actions==3&&player==actor&&copies.size()==2&&current(secondCopier).isInZone(ZoneType.Graveyard)){next=current(counter);target=originalAbility;}
            else if(actions==(scenario.equals("copy-of-copy-counter")?4:2)&&game.getStack().isEmpty()){
                capture("settlement-before-assertions");
                int bDraws=scenario.equals("repeated")||scenario.equals("fresh-values")?2:1;
                Assert.assertEquals(copies.size(),scenario.equals("repeated")||scenario.equals("copy-of-copy-counter")?2:1);
                Assert.assertEquals(libraryBaseline.get(actor)-actor.getCardsIn(ZoneType.Library).size(),1);
                Assert.assertEquals(libraryBaseline.get(other)-other.getCardsIn(ZoneType.Library).size(),bDraws);
                Assert.assertEquals(actor.getLife(),scenario.equals("fresh-values")?20:23);Assert.assertEquals(other.getLife(),scenario.equals("fresh-values")?20:scenario.equals("repeated")?26:23);
                if(scenario.equals("fresh-values"))Assert.assertEquals(typeChoices,List.of("B:Goblin","A:Elf"));else Assert.assertTrue(typeChoices.isEmpty());
                Assert.assertTrue(current(original).isInZone(ZoneType.Graveyard));for(SpellAbility copy:copies)Assert.assertFalse(game.getCardsInGame().contains(copy.getHostCard()));
                Assert.assertEquals(targetChoices,actions-1);capture("settled");done=true;return null;
            }
            if(next==null)return null;
            selected=next.getFirstSpellAbility();selected.setActivatingPlayer(player);Assert.assertTrue(selected.canPlay());
            player.getManaPool().addMana(new Mana(ManaAtom.fromName("G"),next,null,player));return List.of(selected);
        }
        boolean play(Player player,SpellAbility ability){
            Assert.assertSame(ability,selected);Assert.assertSame(ability.getActivatingPlayer(),player);int before=game.getStack().size();
            Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));Assert.assertEquals(game.getStack().size(),before+1);
            if(actions==0){originalAbility=game.getStack().peekAbility();traits(originalAbility);}
            Assert.assertEquals(player.getManaPool().getAmountOfColor(ManaAtom.fromName("G")),0);actions++;capture("cast");return true;
        }
    }
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
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.selected);return script.selected;}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){
            if(abilities.isEmpty())return;script.copyCallbacks++;Assert.assertEquals(abilities.size(),script.scenario.equals("repeated")?2:1);
            for(int i=abilities.size()-1;i>=0;i--){SpellAbility next=abilities.get(i);Assert.assertTrue(next.isCopied());Assert.assertTrue(next.isSpell());Assert.assertSame(next.getActivatingPlayer(),getPlayer());Assert.assertSame(next.getHostCard().getOwner(),getPlayer());script.traits(next);
                if(!next.getHostCard().isInZone(ZoneType.Stack))next.setHostCard(getGame().getAction().moveToStack(next.getHostCard(),next));else getGame().getStackZone().add(next.getHostCard());
                Assert.assertEquals(next.getTargets().size(),0);getGame().getStack().add(next);for(SpellAbility prior:script.copies)Assert.assertNotEquals(prior.getHostCard().getId(),next.getHostCard().getId());script.copies.add(next);
            }script.capture("copies-placed");
        }
        @Override public boolean chooseTargetsFor(SpellAbility ability){Assert.assertSame(ability,script.selected);Assert.assertNotNull(script.target);Assert.assertTrue(ability.canTarget(script.target));ability.getTargets().add(script.target);Assert.assertTrue(ability.isTargetNumberValid());script.targetChoices++;return true;}
        @Override public String chooseSomeType(String kind,SpellAbility ability,Collection<String> offered,boolean optional){Assert.assertEquals(script.scenario,"fresh-values");Assert.assertEquals(kind,"Creature");Assert.assertFalse(optional);String value=getPlayer()==script.actor?"Elf":"Goblin";Assert.assertTrue(offered.contains(value));script.typeChoices.add((getPlayer()==script.actor?"A:":"B:")+value);return value;}
        @Override public void notifyOfValue(SpellAbility ability,GameObject related,String value){Assert.assertEquals(script.scenario,"fresh-values");}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,false);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){
            Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.manaPayments++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);
            try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(ManaAtom.fromName("G"),ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}
            finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}
        }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return new StrictRejectingController.Costs(player,ability,false){
            @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}
            @Override public boolean paysRightAfterDecision(){return true;}
            @Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}
        };}
    }
}
