package forge.ai;

import com.google.gson.GsonBuilder;
import forge.LobbyPlayer;
import forge.card.*;
import forge.card.mana.*;
import forge.deck.Deck;
import forge.game.*;
import forge.game.ability.*;
import forge.game.card.*;
import forge.game.cost.*;
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

/** Four prepared normal-paid mechanisms. No full card, opening or runtime qualification. */
public class SpellwakeCastTriggerReferenceTest extends AITest {
    @Test public void normalPaidFilteredWatcher() throws Exception { run("watcher"); }
    @Test public void normalPaidPermanentSelfCast() throws Exception { run("permanent-self"); }
    @Test public void normalPaidInstructionSelfCast() throws Exception { run("instruction-self"); }
    @Test public void normalPaidCopyKeepsTriggerWithoutCasting() throws Exception { run("copy-self"); }

    private void run(String scenario) throws Exception {
        Script s = new Script(scenario); boolean passed = false;
        try {
            s.seed(); s.capture("prepared"); s.game.getPhaseHandler().setupFirstTurn(s.actor, null);
            for (int n=0; n<220 && !s.done; n++) {
                Assert.assertFalse(s.game.isGameOver()); s.game.getPhaseHandler().mainLoopStep();
            }
            Assert.assertTrue(s.done, "normal priority loop did not settle");
            Assert.assertEquals(s.payments, s.casts.size());
            Assert.assertEquals(s.bodies.size(), s.triggers.size());
            Assert.assertTrue(s.allowedBodies.isEmpty()); passed = true;
        } finally {
            Map<String,Object> result = new LinkedHashMap<>();
            result.put("scenario", scenario); result.put("assertions_passed", passed);
            result.put("casts", s.casts); result.put("payments", s.payments);
            result.put("triggers", s.triggers); result.put("bodies", s.bodies); result.put("copies", s.copies);
            result.put("rows", s.rows); result.put("trigger_zero_payments", s.triggerZeroPayments);
            result.put("body_zero_payments", s.bodyZeroPayments); result.put("target_choices", s.targetChoices);
            result.put("scope", "actual normal paid partial-program casts, mandatory trigger/body and copy placement callbacks; prepared libraries/main phase and injected U; reused Forge/resources/runtime, no complete card or numeric ID/runtime/performance parity");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")))
                .resolve(scenario+".json"), new GsonBuilder().serializeNulls().create().toJson(result)+"\n", StandardOpenOption.CREATE_NEW);
        }
    }

    private static final class Script {
        final String scenario; final Game game; final Player actor, other;
        final List<Map<String,Object>> casts=new ArrayList<>(), triggers=new ArrayList<>(), bodies=new ArrayList<>(), copies=new ArrayList<>(), rows=new ArrayList<>();
        final Map<SpellAbility,Map<String,Object>> allowedBodies = new IdentityHashMap<>();
        Card original, observer, negative, copier; SpellAbility originalAbility, selected, target, admittingTrigger, resolvingBody;
        int payments, triggerZeroPayments, bodyZeroPayments, targetChoices; boolean done;
        Script(String scenario) {
            this.scenario=scenario;
            List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("B")),new RegisteredPlayer(new Deck()).setPlayer(new Lobby("A")));
            GameRules rules=new GameRules(GameType.Constructed);
            game=new Game(players,rules,new Match(rules,players,"Spellwake cast-trigger comparison")); game.setAge(GameStage.Play);
            actor=game.getPlayers().get(1); other=game.getPlayers().get(0);
            for(Player player:game.getPlayers()) { ((Controller)player.getController()).script=this; Assert.assertFalse(player.getController().isAI()); }
        }
        Card seed(Player player,ZoneType zone,String role,String types,String cost,String... extra) {
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+role,"ManaCost:"+cost,"Types:"+types,"Oracle:")); lines.addAll(List.of(extra));
            Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);
            card.setGameTimestamp(game.getNextTimestamp()); player.getZone(zone).add(card); return card;
        }
        String[] castTrigger(boolean self) {
            return new String[]{"T:Mode$ SpellCast | ValidCard$ "+(self?"Card.Self":"Instant,Sorcery")+" | ValidActivatingPlayer$ You | TriggerZones$ "+(self?"Stack":"Battlefield")+" | Execute$ CastBody | TriggerDescription$ Whenever the selected spell becomes cast, gain 1 life.",
                "SVar:CastBody:DB$ GainLife | Defined$ You | LifeAmount$ 1"};
        }
        void seed() {
            for(Player p:game.getPlayers()) for(int n=0;n<24;n++) seed(p,ZoneType.Library,"Library "+n,"Artifact","0");
            List<String> extra=new ArrayList<>(); boolean permanent=scenario.equals("permanent-self");
            if(!permanent) extra.add("A:SP$ GainLife | Cost$ U | Defined$ You | LifeAmount$ 2");
            if(!scenario.equals("watcher")) extra.addAll(List.of(castTrigger(true)));
            original=seed(actor,ZoneType.Hand,"Producer",permanent?"Artifact":"Instant","U",extra.toArray(String[]::new));
            if(scenario.equals("watcher")||scenario.equals("copy-self")) observer=seed(actor,ZoneType.Battlefield,"Observer","Artifact","0",castTrigger(false));
            if(scenario.equals("watcher")) negative=seed(actor,ZoneType.Hand,"Negative","Artifact","U");
            if(scenario.equals("copy-self")) copier=seed(actor,ZoneType.Hand,"Copier","Instant","U","A:SP$ CopySpellAbility | Cost$ U | ValidTgts$ Spell | Controller$ You | Amount$ 1");
        }
        String role(Card card) { String name=card.getName(); Assert.assertTrue(name.startsWith("Spellwake ")); return name.substring("Spellwake ".length()); }
        String zone(Card card) { return card.getZone()==null?"none":card.getZone().getZoneType().name(); }
        Map<String,Object> card(Card card) {
            Map<String,Object> value=new LinkedHashMap<>(); value.put("id",card.getId()); value.put("role",role(card)); value.put("owner",card.getOwner().getName()); value.put("controller",card.getController().getName()); value.put("zone",zone(card)); value.put("types",card.getType().toString()); value.put("timestamp",card.getGameTimestamp()); return value;
        }
        void capture(String point) {
            Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);row.put("stack",game.getStack().size());row.put("life_a",actor.getLife());row.put("life_b",other.getLife());row.put("casts",casts.size());row.put("triggers",triggers.size());row.put("bodies",bodies.size());row.put("copies",copies.size());row.put("producer_zone",zone(game.getCardState(original)));row.put("negative_zone",negative==null?null:zone(game.getCardState(negative)));rows.add(row);
        }
        SpellAbility expectedAbility() { return resolvingBody!=null?resolvingBody:admittingTrigger!=null?admittingTrigger:selected; }
        List<SpellAbility> choose(Player player) {
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            Card next=null; target=null;
            if(casts.isEmpty()&&player==actor&&game.getStack().isEmpty()) next=game.getCardState(original);
            else if(scenario.equals("watcher")&&casts.size()==1&&player==actor&&game.getStack().isEmpty()) next=game.getCardState(negative);
            else if(scenario.equals("copy-self")&&casts.size()==1&&bodies.size()==2&&player==actor&&game.getStack().size()==1) {next=game.getCardState(copier);target=originalAbility;}
            else if(casts.size()==((scenario.equals("watcher")||scenario.equals("copy-self"))?2:1)&&game.getStack().isEmpty()) {
                capture("settled");done=true;return null;
            }
            if(next==null)return null;
            selected=next.getFirstSpellAbility();selected.setActivatingPlayer(player);Assert.assertTrue(selected.canPlay());
            player.getManaPool().addMana(new Mana(ManaAtom.fromName("U"),next,null,player));return List.of(selected);
        }
        boolean play(Player player,SpellAbility ability) {
            Assert.assertSame(ability,selected);Assert.assertSame(player,actor);
            Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));
            Assert.assertEquals(player.getManaPool().getAmountOfColor(ManaAtom.fromName("U")),0);
            SpellAbility actual=game.getStack().peekAbility(); Assert.assertTrue(actual.isSpell()); Assert.assertFalse(actual.isCopied());
            Assert.assertEquals(actual.getHostCard().getId(),ability.getHostCard().getId());Assert.assertSame(actual.getActivatingPlayer(),player);
            if(casts.isEmpty())originalAbility=actual;
            Map<String,Object> row=new LinkedHashMap<>();row.put("card",card(actual.getHostCard()));row.put("caster",player.getName());row.put("blue_after",player.getManaPool().getAmountOfColor(ManaAtom.fromName("U")));row.put("stack",game.getStack().size());row.put("copied",actual.isCopied());casts.add(row);capture("paid-cast");return true;
        }
        void place(Player player,List<SpellAbility> offered) {
            if(offered.isEmpty())return;
            for(SpellAbility ability:offered) {
                if(ability.isCopied()) {
                    Assert.assertEquals(scenario,"copy-self");Assert.assertEquals(offered.size(),1);Assert.assertTrue(ability.isSpell());Assert.assertSame(player,actor);Assert.assertSame(ability.getHostCard().getOwner(),player);
                    Assert.assertEquals(ability.getTargets().size(),0);
                    Card host=ability.getHostCard();List<Map<String,Object>> definitions=new ArrayList<>();
                    for(Trigger trigger:host.getTriggers()) {Map<String,Object> definition=new LinkedHashMap<>();definition.put("mode",trigger.getMode().name());definition.put("valid_card",trigger.getParam("ValidCard"));definition.put("zones",trigger.getParam("TriggerZones"));definition.put("execute",trigger.getParam("Execute"));definitions.add(definition);}
                    Assert.assertEquals(definitions.size(),1);Assert.assertEquals(definitions.get(0).get("mode"),"SpellCast");Assert.assertEquals(definitions.get(0).get("valid_card"),"Card.Self");
                    if(!host.isInZone(ZoneType.Stack)) ability.setHostCard(game.getAction().moveToStack(host,ability));else game.getStackZone().add(host);
                    int before=game.getStack().size();game.getStack().add(ability);
                    Map<String,Object> row=new LinkedHashMap<>();row.put("card",card(ability.getHostCard()));row.put("copied",ability.isCopied());row.put("controller",player.getName());row.put("donor",card(originalAbility.getHostCard()));row.put("definitions",definitions);row.put("stack_before",before);row.put("stack_after",game.getStack().size());row.put("casts_at_copy",casts.size());row.put("triggers_at_copy",triggers.size());copies.add(row);capture("direct-copy-placed");
                    continue;
                }
                Assert.assertTrue(ability instanceof WrappedAbility); WrappedAbility wrapped=(WrappedAbility)ability;
                Assert.assertEquals(wrapped.getTrigger().getMode(),TriggerType.SpellCast);Assert.assertFalse(wrapped.isOptionalTrigger());Assert.assertFalse(wrapped.usesTargeting());Assert.assertSame(wrapped.getActivatingPlayer(),player);Assert.assertEquals(wrapped.getWrappedAbility().getApi(),ApiType.GainLife);
                Object causing=wrapped.getTriggeringObject(AbilityKey.SpellAbility);Assert.assertTrue(causing instanceof SpellAbility);SpellAbility cause=(SpellAbility)causing;
                Object causingCard=wrapped.getTriggeringObject(AbilityKey.Card);Assert.assertTrue(causingCard instanceof Card);Assert.assertEquals(((Card)causingCard).getId(),cause.getHostCard().getId());
                Object caster=wrapped.getTriggeringObject(AbilityKey.Activator);Assert.assertSame(caster,actor);
                Assert.assertTrue(role(wrapped.getHostCard()).equals("Producer")||role(wrapped.getHostCard()).equals("Observer"));
                Map<String,Object> row=new LinkedHashMap<>();row.put("source",card(wrapped.getHostCard()));row.put("cause",card(cause.getHostCard()));row.put("controller",player.getName());row.put("caster",((Player)caster).getName());row.put("mode",wrapped.getTrigger().getMode().name());row.put("self",wrapped.getHostCard().getId()==cause.getHostCard().getId());row.put("stack_before",game.getStack().size());row.put("trigger_id",wrapped.getTrigger().getId());
                allowedBodies.put(wrapped.getWrappedAbility(),row);
                admittingTrigger=wrapped;try{Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,wrapped));}finally{admittingTrigger=null;}
                row.put("stack_after",game.getStack().size());triggers.add(row);
            }
            capture("normal-cohort-placed");
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
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.expectedAbility());return abilities.get(0);}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){script.place(getPlayer(),abilities);}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void playSpellAbilityNoStack(SpellAbility ability,boolean setup) {
            Assert.assertFalse(setup);Map<String,Object> placement=script.allowedBodies.remove(ability);Assert.assertNotNull(placement);int before=getPlayer().getLife();
            script.resolvingBody=ability;try{Assert.assertTrue(PlaySpellAbility.playSpellAbilityNoStack(this,getPlayer(),ability,true));}finally{script.resolvingBody=null;}
            Map<String,Object> row=new LinkedHashMap<>();row.put("source",placement.get("source"));row.put("cause",placement.get("cause"));row.put("controller",getPlayer().getName());row.put("life_before",before);row.put("life_after",getPlayer().getLife());script.bodies.add(row);script.capture("mandatory-trigger-body-return");
        }
        @Override public boolean chooseTargetsFor(SpellAbility ability){Assert.assertSame(ability,script.selected);Assert.assertNotNull(script.target);Assert.assertTrue(ability.canTarget(script.target));ability.getTargets().add(script.target);Assert.assertTrue(ability.isTargetNumberValid());script.targetChoices++;return true;}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.expectedAbility());Assert.assertEquals(effect,ability.isTrigger());if(ability!=script.selected){Assert.assertTrue(cost.isZero());if(script.resolvingBody==ability)script.bodyZeroPayments++;else script.triggerZeroPayments++;}return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,effect);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.expectedAbility());Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.payments++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(ManaAtom.fromName("U"),ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(ability,script.expectedAbility());Assert.assertEquals(effect,ability.isTrigger());return new StrictRejectingController.Costs(player,ability,effect){@Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}@Override public boolean paysRightAfterDecision(){return true;}@Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}};}
    }
}
