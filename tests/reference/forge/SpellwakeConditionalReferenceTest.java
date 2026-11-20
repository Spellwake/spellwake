package forge.ai;

import com.google.gson.GsonBuilder;
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
import java.nio.file.*;
import java.io.*;
import java.security.MessageDigest;
import forge.game.ability.AbilityUtils;
import forge.game.ability.effects.LifeGainEffect;
import forge.game.ability.effects.ChangeZoneEffect;
import java.util.*;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Actual normal-paid conditional spell runs; literal partial definitions only. */
public class SpellwakeConditionalReferenceTest extends AITest {
    @Test public void lifeTrue() throws Exception { run("CF1"); }
    @Test public void lifeFalseTail() throws Exception { run("CF2"); }
    @Test public void changedCurrentLife() throws Exception { run("CF3"); }
    @Test public void illegalTargetPrelude() throws Exception { run("CF4"); }
    void run(String id) throws Exception {
        Script s=new Script(id);boolean passed=false;
        try {
            s.seed();s.capture("prepared");s.game.getPhaseHandler().setupFirstTurn(s.players.get(0),null);
            for(int n=0;n<300&&!s.done;n++){Assert.assertFalse(s.game.isGameOver());s.game.getPhaseHandler().mainLoopStep();}
            Assert.assertTrue(s.done,"bounded normal loop did not settle");
            Assert.assertEquals(s.payments,id.equals("CF4")?2:1);
            Assert.assertEquals(s.players.get(0).getLife(),switch(id){case "CF1"->23;case "CF2"->21;case "CF3"->28;default->20;});
            Assert.assertEquals(s.players.get(1).getLife(),20);Assert.assertTrue(s.game.getStack().isEmpty());
            Assert.assertEquals(s.game.getCardState(s.original).getZone().getZoneType(),ZoneType.Graveyard);
            if(s.creature!=null)Assert.assertEquals(s.game.getCardState(s.creature).getZone().getZoneType(),ZoneType.Hand);
            passed=true;
        } finally {
            Map<String,Object> report=new LinkedHashMap<>();report.put("schema_version",1);report.put("kind","actual-forge-conditional-observations");report.put("scenario",id);report.put("assertions_passed",passed);report.put("payments",s.payments);report.put("casts",s.casts);report.put("targets",s.targets);report.put("rows",s.rows);report.put("runtime_classes",runtimeClasses());
            report.put("limits","Prepared literal partial Programs, injected blue followed by real normal payment/casting and Game main loop. No native instruction/cursor/cause/branch facts, complete-card or opening-game equivalence. Reused Forge/dependencies/JVM/resource closure requires separate scope disclosure.");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir"))).resolve(id+".json"),new GsonBuilder().serializeNulls().create().toJson(report)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    static List<Map<String,Object>> runtimeClasses() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();
        for(Class<?> type:List.of(Game.class,CardRules.Reader.class,SpellAbility.class,SpellAbilityCondition.class,PlaySpellAbility.class,AbilityUtils.class,LifeGainEffect.class,ChangeZoneEffect.class,SpellwakeConditionalReferenceTest.class,StrictRejectingController.class)) {
            String resource="/"+type.getName().replace('.','/')+".class";
            byte[] bytes;try(InputStream input=Objects.requireNonNull(type.getResourceAsStream(resource))){bytes=input.readAllBytes();}
            Map<String,Object> row=new LinkedHashMap<>();row.put("name",type.getName());row.put("resource",resource.substring(1));row.put("url",Objects.requireNonNull(type.getResource(resource)).toString());row.put("bytes",bytes.length);row.put("sha256",HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes)));row.put("code_source",type.getProtectionDomain().getCodeSource().getLocation().toString());rows.add(row);
        }
        return rows;
    }
    static class Script {
        final String id;final Game game;final List<Player> players;
        final List<Map<String,Object>> casts=new ArrayList<>(),targets=new ArrayList<>(),rows=new ArrayList<>();
        Card original,creature,response;SpellAbility selected;int payments;boolean done;
        Script(String id){
            this.id=id;List<RegisteredPlayer> registered=new ArrayList<>();for(int n=0;n<2;n++)registered.add(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("P"+n)));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(registered,rules,new Match(rules,registered,"Spellwake conditional flow"));game.setAge(GameStage.Play);players=new ArrayList<>(game.getPlayers());
            for(int n=0;n<2;n++){Assert.assertEquals(players.get(n).getName(),"P"+n);((Controller)players.get(n).getController()).script=this;Assert.assertFalse(players.get(n).getController().isAI());}
        }
        Card seed(Player player,ZoneType zone,String name,String type,String cost,String...extra){
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:"+cost,"Types:"+type,"Oracle:"));lines.addAll(List.of(extra));
            Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);card.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(card);return card;
        }
        void seed(){
            for(Player p:players)for(int n=0;n<24;n++)seed(p,ZoneType.Library,p.getName()+" Library "+n,"Artifact","0");
            List<String> lines=new ArrayList<>();String main;
            if(id.equals("CF4"))main="SP$ ChangeZone | Cost$ U | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand | ConditionLifeTotal$ You | ConditionLifeAmount$ GE21 | SubAbility$ Tail";
            else if(id.equals("CF3")){main="SP$ GainLife | Cost$ U | Defined$ You | LifeAmount$ 5 | SubAbility$ Conditional";lines.add("SVar:Conditional:DB$ GainLife | Defined$ You | LifeAmount$ 2 | ConditionLifeTotal$ You | ConditionLifeAmount$ GE25 | SubAbility$ Tail");}
            else main="SP$ GainLife | Cost$ U | Defined$ You | LifeAmount$ 2 | ConditionLifeTotal$ You | ConditionLifeAmount$ "+(id.equals("CF1")?"GE20":"GE21")+" | SubAbility$ Tail";
            lines.add("A:"+main);lines.add("SVar:Tail:DB$ GainLife | Defined$ You | LifeAmount$ 1");original=seed(players.get(0),ZoneType.Hand,"S","Instant","U",lines.toArray(String[]::new));
            if(id.equals("CF4")){creature=seed(players.get(1),ZoneType.Battlefield,"C","Creature","0","PT:2/2");response=seed(players.get(1),ZoneType.Hand,"R","Instant","U","A:SP$ ChangeZone | Cost$ U | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand");}
        }
        String role(Card card){Assert.assertTrue(card.getName().startsWith("Spellwake "));return card.getName().substring(10);}
        Map<String,Object> card(Card card){Map<String,Object> row=new LinkedHashMap<>();row.put("id",card.getId());row.put("role",role(card));row.put("owner",card.getOwner().getName());row.put("controller",card.getController().getName());row.put("zone",card.getZone()==null?null:card.getZone().getZoneType().name());row.put("timestamp",card.getGameTimestamp());return row;}
        void capture(String point){Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);row.put("life",players.stream().map(p->Map.of("player",p.getName(),"life",p.getLife())).toList());List<Map<String,Object>> stack=new ArrayList<>();for(SpellAbilityStackInstance item:game.getStack()){SpellAbility ability=item.getSpellAbility();stack.add(Map.of("entry",ability.getId(),"role",role(ability.getHostCard()),"controller",ability.getActivatingPlayer().getName(),"targets",ability.getTargets().getTargetCards().stream().map(this::card).toList()));}row.put("stack",stack);row.put("original",card(game.getCardState(original)));row.put("creature",creature==null?null:card(game.getCardState(creature)));rows.add(row);}
        List<SpellAbility> choose(Player player){
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            Card next=null;
            if(casts.isEmpty()&&player==players.get(0)&&game.getStack().isEmpty())next=game.getCardState(original);
            else if(id.equals("CF4")&&casts.size()==1&&game.getStack().size()==1&&player==players.get(1))next=game.getCardState(response);
            else if(casts.size()==(id.equals("CF4")?2:1)&&game.getStack().isEmpty()){capture("settled");done=true;return null;}
            if(next==null)return null;selected=next.getFirstSpellAbility();selected.setActivatingPlayer(player);Assert.assertTrue(selected.canPlay());player.getManaPool().addMana(new Mana(ManaAtom.fromName("U"),next,null,player));return List.of(selected);
        }
        boolean play(Player player,SpellAbility ability){Assert.assertSame(ability,selected);Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));SpellAbility actual=game.getStack().peekAbility();Assert.assertTrue(actual.isSpell());Assert.assertFalse(actual.isCopied());Assert.assertSame(actual.getActivatingPlayer(),player);Assert.assertEquals(player.getManaPool().getAmountOfColor(ManaAtom.fromName("U")),0);casts.add(Map.of("entry",actual.getId(),"source",card(actual.getHostCard()),"caster",player.getName(),"blue_after",0));capture("paid-cast");return true;}
        boolean target(SpellAbility ability){Assert.assertSame(ability,selected);Assert.assertEquals(id,"CF4");List<String> domain=new ArrayList<>();for(GameEntity candidate:ability.getTargetRestrictions().getAllCandidates(ability)){Assert.assertTrue(candidate instanceof Card);domain.add(role((Card)candidate));}Assert.assertEquals(domain,List.of("C"));Card chosen=game.getCardState(creature);Assert.assertTrue(ability.canTarget(chosen));ability.getTargets().add(chosen);Assert.assertTrue(ability.isTargetNumberValid());targets.add(Map.of("entry",ability.getId(),"domain",domain,"selected",card(chosen)));return true;}
    }
    static class Lobby extends LobbyPlayer implements IGameEntitiesFactory {
        Lobby(String name){super(name);}
        @Override public Player createIngamePlayer(Game game,int id){Player p=new Player(getName(),game,id);p.setFirstController(new Controller(game,p,this));return p;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw new AssertionError("Unexpected control");}
        @Override public void hear(LobbyPlayer from,String message){throw new AssertionError("Unexpected message");}
    }
    static class Controller extends StrictRejectingController {
        Script script;Controller(Game game,Player player,LobbyPlayer lobby){super(game,player,lobby);}
        @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected callback "+method);}
        @Override public List<SpellAbility> chooseSpellAbilityToPlay(){return script.choose(getPlayer());}
        @Override public boolean playChosenSpellAbility(SpellAbility ability){return script.play(getPlayer(),ability);}
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.selected);return abilities.get(0);}
        @Override public boolean chooseTargetsFor(SpellAbility ability){return script.target(ability);}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,effect);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.payments++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(ManaAtom.fromName("U"),ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);return new StrictRejectingController.Costs(player,ability,effect){@Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}@Override public boolean paysRightAfterDecision(){return true;}@Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}};}
    }
}
