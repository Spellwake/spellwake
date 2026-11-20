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

/** Actual paid casting/priority/resolution over synthetic seeds and injected mana.
 * The standard retained Saproling token script is used. These finite observations
 * do not qualify complete cards, opening or the reused Forge/runtime closure.
 */
public class SpellwakeTokenReferenceTest extends AITest {
    @Test public void normalPaidLiteralCreation() throws Exception {run("single");}
    @Test public void normalOwnedCopyCreatesForItsController() throws Exception {run("copy");}
    @Test public void normalTokenBounceCeasesBeforePriority() throws Exception {run("bounce");}
    @Test public void normalMixedCardTokenStateActions() throws Exception {run("mixed");}
    private void run(String scenario) throws Exception {
        Script script=new Script(scenario);boolean passed=false;
        try {
            script.seed();script.capture("prepared");script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            for(int n=0;n<120&&!script.done;n++) {
                Assert.assertFalse(script.game.isGameOver());script.game.getPhaseHandler().mainLoopStep();Assert.assertTrue(n<119,"bounded priority exhausted");
            }
            Assert.assertTrue(script.done);Assert.assertTrue(script.priorities>0);Assert.assertEquals(script.payments,script.actions);passed=true;
        } finally {
            Map<String,Object> result=new LinkedHashMap<>();result.put("scenario",scenario);result.put("assertions_passed",passed);
            result.put("casts",script.actions);result.put("mana_payments",script.payments);result.put("priority_callbacks",script.priorities);
            result.put("copy_callbacks",script.copyCallbacks);result.put("target_choices",script.targetChoices);result.put("zone_order_callbacks",script.zoneOrders);result.put("rows",script.rows);
            result.put("scope","normal paid casts and actual priority/resolution; synthetic seed and injected G/UU differ from native land funding; mixed enchantment costs G here versus native free; raw Forge zone collections retained without assuming native pile orientation; exact native/Forge numeric identity and runtime/resource closure unqualified");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir"))).resolve(scenario+".json"),new GsonBuilder().serializeNulls().create().toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    private static final class Script {
        final String scenario;final Game game;final Player actor,other;
        final List<Map<String,Object>> rows=new ArrayList<>();final List<Map<String,Object>> zoneOrders=new ArrayList<>();final List<SpellAbility> copies=new ArrayList<>();
        Card original,copier,secondary,first,second,trackedToken;SpellAbility originalAbility,selected,targetSpell;Card targetCard;
        int actions,priorities,payments,copyCallbacks,targetChoices;boolean done;byte payColor;
        Script(String scenario) {
            this.scenario=scenario;List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(players,rules,new Match(rules,players,"Spellwake token foundation"));game.setAge(GameStage.Play);
            actor=game.getPlayers().get(1);other=game.getPlayers().get(0);for(Player player:game.getPlayers()){((Controller)player.getController()).script=this;Assert.assertFalse(player.getController().isAI());}
        }
        Card seed(Player player,ZoneType zone,String name,String types,String cost,String... extra) {
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:"+cost,"Types:"+types,"Oracle:"));lines.addAll(List.of(extra));
            Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);
            card.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(card);return card;
        }
        Card current(Card card){return game.getCardState(card);}
        void seed() {
            for(Player player:game.getPlayers())for(int n=0;n<24;n++)seed(player,ZoneType.Library,"Library "+n,"Artifact","0");
            original=seed(actor,ZoneType.Hand,"Producer","Instant","G","A:SP$ Token | Cost$ G | TokenScript$ g_1_1_saproling | TokenOwner$ You");
            if(scenario.equals("copy"))copier=seed(other,ZoneType.Hand,"Copier","Instant","U U","A:SP$ CopySpellAbility | Cost$ U U | ValidTgts$ Spell | Controller$ You");
            if(scenario.equals("bounce"))secondary=seed(actor,ZoneType.Hand,"Bounce","Instant","G","A:SP$ ChangeZone | Cost$ G | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand");
            if(scenario.equals("mixed")) {
                first=seed(actor,ZoneType.Battlefield,"First Physical","Creature Elf","0","PT:1/1");second=seed(actor,ZoneType.Battlefield,"Second Physical","Creature Goblin","0","PT:1/1");
                secondary=seed(actor,ZoneType.Hand,"Shrink","Enchantment","G","S:Mode$ Continuous | Affected$ Creature | AddPower$ -1 | AddToughness$ -1");
            }
        }
        List<Card> tokens() {List<Card> values=new ArrayList<>();for(Card card:game.getCardsIn(ZoneType.Battlefield))if(card.isToken())values.add(card);return values;}
        Map<String,Object> card(Card card) {
            Map<String,Object> value=new LinkedHashMap<>();value.put("id",card.getId());value.put("name",card.getName());value.put("token",card.isToken());value.put("owner",card.getOwner().getName());value.put("controller",card.getController().getName());
            value.put("zone",card.getZone()==null?null:card.getZone().getZoneType().name());value.put("types",card.getType().toString());value.put("color_mask",card.getColor().getColor());value.put("power",card.getNetPower());value.put("toughness",card.getNetToughness());value.put("tapped",card.isTapped());return value;
        }
        void capture(String point) {
            Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);row.put("casts",actions);row.put("stack",game.getStack().size());row.put("tokens",tokens().stream().map(this::card).toList());
            row.put("graveyard_a",actor.getCardsIn(ZoneType.Graveyard).stream().map(this::card).toList());row.put("hand_a",actor.getCardsIn(ZoneType.Hand).stream().map(this::card).toList());row.put("tracked_token_still_in_game",trackedToken==null?null:game.getCardsInGame().contains(trackedToken));row.put("life_a",actor.getLife());row.put("life_b",other.getLife());rows.add(row);
        }
        List<SpellAbility> choose(Player player) {
            priorities++;Assert.assertSame(player,game.getPhaseHandler().getPriorityPlayer());
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            Card next=null;targetSpell=null;targetCard=null;
            if(actions==0&&player==actor&&game.getStack().isEmpty())next=current(original);
            else if(scenario.equals("copy")&&actions==1&&player==other&&game.getStack().size()==1){next=current(copier);targetSpell=originalAbility;}
            else if((scenario.equals("bounce")||scenario.equals("mixed"))&&actions==1&&player==actor&&game.getStack().isEmpty()) {
                Assert.assertEquals(tokens().size(),1);trackedToken=tokens().get(0);capture("token-before-departure");next=current(secondary);if(scenario.equals("bounce"))targetCard=trackedToken;
            } else if(actions==(scenario.equals("single")?1:2)&&game.getStack().isEmpty()) {
                capture("settlement-before-assertions");
                if(scenario.equals("single")||scenario.equals("copy")) {
                    Assert.assertEquals(tokens().size(),scenario.equals("single")?1:2);Set<Player> owners=new HashSet<>();Set<Integer> ids=new HashSet<>();
                    for(Card token:tokens()){Assert.assertEquals(token.getName(),"Saproling Token");Assert.assertTrue(token.isCreature());Assert.assertTrue(token.getType().hasSubtype("Saproling"));Assert.assertEquals(token.getColor().getColor(),MagicColor.GREEN);Assert.assertEquals(token.getNetPower(),1);Assert.assertEquals(token.getNetToughness(),1);Assert.assertFalse(token.isTapped());Assert.assertSame(token.getController(),token.getOwner());owners.add(token.getOwner());Assert.assertTrue(ids.add(token.getId()));}
                    Assert.assertEquals(owners,scenario.equals("single")?Set.of(actor):Set.of(actor,other));
                } else {
                    Assert.assertTrue(tokens().isEmpty());Assert.assertNotNull(trackedToken);Assert.assertFalse(game.getCardsInGame().contains(trackedToken));
                    for(Card card:actor.getCardsIn(ZoneType.Hand))Assert.assertFalse(card.isToken());for(Card card:actor.getCardsIn(ZoneType.Graveyard))Assert.assertFalse(card.isToken());
                    if(scenario.equals("mixed")){Assert.assertTrue(current(first).isInZone(ZoneType.Graveyard));Assert.assertTrue(current(second).isInZone(ZoneType.Graveyard));}
                }
                Assert.assertEquals(copies.size(),scenario.equals("copy")?1:0);capture("settled");done=true;return null;
            }
            if(next==null)return null;selected=next.getFirstSpellAbility();selected.setActivatingPlayer(player);Assert.assertTrue(selected.canPlay());
            boolean isCopy=copier!=null&&next.equals(current(copier));payColor=ManaAtom.fromName(isCopy?"U":"G");for(int n=0;n<(isCopy?2:1);n++)player.getManaPool().addMana(new Mana(payColor,next,null,player));return List.of(selected);
        }
        boolean play(Player player,SpellAbility ability) {
            Assert.assertSame(ability,selected);Assert.assertSame(ability.getActivatingPlayer(),player);int before=game.getStack().size();Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));Assert.assertEquals(game.getStack().size(),before+1);
            if(actions==0)originalAbility=game.getStack().peekAbility();Assert.assertEquals(player.getManaPool().getAmountOfColor(payColor),0);actions++;capture("cast");return true;
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
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities) {
            if(abilities.isEmpty())return;Assert.assertEquals(script.scenario,"copy");Assert.assertEquals(abilities.size(),1);script.copyCallbacks++;
            SpellAbility next=abilities.get(0);Assert.assertTrue(next.isCopied());Assert.assertTrue(next.isSpell());Assert.assertSame(next.getActivatingPlayer(),getPlayer());Assert.assertSame(next.getHostCard().getOwner(),getPlayer());Assert.assertEquals(next.getTargets().size(),0);
            if(!next.getHostCard().isInZone(ZoneType.Stack))next.setHostCard(getGame().getAction().moveToStack(next.getHostCard(),next));else getGame().getStackZone().add(next.getHostCard());getGame().getStack().add(next);script.copies.add(next);script.capture("copy-placed");
        }
        @Override public boolean chooseTargetsFor(SpellAbility ability) {
            Assert.assertSame(ability,script.selected);if(script.targetSpell!=null){Assert.assertTrue(ability.canTarget(script.targetSpell));ability.getTargets().add(script.targetSpell);}else {Assert.assertNotNull(script.targetCard);Assert.assertTrue(ability.canTarget(script.targetCard));ability.getTargets().add(script.targetCard);}Assert.assertTrue(ability.isTargetNumberValid());script.targetChoices++;return true;
        }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source) {
            script.zoneOrders.add(Map.of("destination",destination.name(),"domain",cards.stream().map(script::card).toList()));
            if(cards.size()<=1)return cards;Assert.assertEquals(script.scenario,"mixed");Assert.assertEquals(destination,ZoneType.Graveyard);
            CardCollection ordered=new CardCollection();for(Card card:cards)if(card.equals(script.second))ordered.add(card);for(Card card:cards)if(card.equals(script.first))ordered.add(card);for(Card card:cards)if(card.isToken())ordered.add(card);Assert.assertEquals(ordered.size(),cards.size());return ordered;
        }
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,false);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect) {
            Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.payments++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);
            try{for(int n=0;n<2&&!cost.isPaid();n++)Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(script.payColor,ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}
        }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return new StrictRejectingController.Costs(player,ability,false){
            @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}
            @Override public boolean paysRightAfterDecision(){return true;}
            @Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}
        };}
    }
}
