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

/** Selected prepared normal-paid mechanisms, not complete-card or runtime qualification. */
public class SpellwakeTargetedTriggerReferenceTest extends AITest {
    @Test public void requiredCreature() throws Exception { run("TR1"); }
    @Test public void noLegalCreature() throws Exception { run("TR2"); }
    @Test public void distinctPlayerRoles() throws Exception { run("TR3"); }
    @Test public void causingSpell() throws Exception { run("TR4"); }
    @Test public void targetLostBeforeResolution() throws Exception { run("TR5"); }
    @Test public void mixedNonfirstApnap() throws Exception { run("TR6"); }
    void run(String id) throws Exception {
        Script s = new Script(id); boolean passed=false;
        try {
            s.seed(); s.capture("prepared"); s.game.getPhaseHandler().setupFirstTurn(s.caster,null);
            for(int n=0;n<300&&!s.done;n++){Assert.assertFalse(s.game.isGameOver());s.game.getPhaseHandler().mainLoopStep();}
            Assert.assertTrue(s.done,"normal loop did not settle"); s.verify(); passed=true;
        } finally {
            Map<String,Object> report=new LinkedHashMap<>(); report.put("schema_version",1);report.put("kind","actual-forge-targeted-trigger-observations");report.put("scenario",id);report.put("assertions_passed",passed);
            report.put("casts",s.casts);report.put("placements",s.placements);report.put("target_callbacks",s.targets);
            report.put("body_callbacks",s.bodies);report.put("orders",s.orders);report.put("rows",s.rows);
            report.put("pending_body_callbacks",s.allowedBodies.values().stream().toList());report.put("payments",s.payments);
            report.put("limits","Literal partial definitions; prepared main/libraries, injected U before real paid cast; real target setup/stack/body callbacks. No native provisional-ID or complete native cause-record parity. Reused Forge/dependencies/resources/JVM not closed.");
            Files.writeString(Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir"))).resolve(id+".json"),new GsonBuilder().serializeNulls().create().toJson(report)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
    static class Script {
        final String id; final Game game; final List<Player> seats; final Player caster;
        final List<Map<String,Object>> casts=new ArrayList<>(),placements=new ArrayList<>(),targets=new ArrayList<>(),bodies=new ArrayList<>(),orders=new ArrayList<>(),rows=new ArrayList<>();
        final Map<SpellAbility,Map<String,Object>> allowedBodies=new IdentityHashMap<>();
        Card original,creature,response; SpellAbility originalAbility,selected,resolvingBody; WrappedAbility admitting;
        int payments; boolean done,readyCaptured;
        Script(String id){
            this.id=id; int count=(id.equals("TR3")||id.equals("TR6"))?3:2;
            List<RegisteredPlayer> registered=new ArrayList<>();for(int n=0;n<count;n++)registered.add(new RegisteredPlayer(new Deck()).setPlayer(new Lobby("P"+n)));
            GameRules rules=new GameRules(GameType.Constructed);game=new Game(registered,rules,new Match(rules,registered,"Spellwake targeted triggers"));game.setAge(GameStage.Play);
            seats=new ArrayList<>(game.getPlayers());for(int n=0;n<count;n++){Assert.assertEquals(seats.get(n).getName(),"P"+n);((Controller)seats.get(n).getController()).script=this;Assert.assertFalse(seats.get(n).getController().isAI());}
            caster=seats.get((id.equals("TR3")||id.equals("TR6"))?1:0);
        }
        Card seed(Player player,ZoneType zone,String name,String type,String cost,String...extra){
            List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:"+cost,"Types:"+type,"Oracle:"));lines.addAll(List.of(extra));
            Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);card.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(card);return card;
        }
        String[] trigger(boolean self,String name,String body){return new String[]{"T:Mode$ SpellCast | ValidCard$ "+(self?"Card.Self":"Card")+" | TriggerZones$ "+(self?"Stack":"Battlefield")+" | Execute$ "+name+" | TriggerDescription$ "+name,"SVar:"+name+":"+body};}
        void seed(){
            for(Player p:seats)for(int n=0;n<24;n++)seed(p,ZoneType.Library,p.getName()+" Library "+n,"Artifact","0");
            List<String> extra=new ArrayList<>();boolean permanent=id.equals("TR2");
            if(!permanent)extra.add("A:SP$ GainLife | Cost$ U | Defined$ You | LifeAmount$ 2");
            if(!id.equals("TR3")&&!id.equals("TR6")){
                String body=id.equals("TR4")?"DB$ Counter | ValidTgts$ Spell":"DB$ ChangeZone | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand";
                if(id.equals("TR1")||id.equals("TR5"))body+=" | SubAbility$ Tail";
                extra.addAll(List.of(trigger(true,"T",body)));extra.add("SVar:Tail:DB$ GainLife | Defined$ You | LifeAmount$ 1");
            }
            original=seed(caster,ZoneType.Hand,"S",permanent?"Artifact":"Instant","U",extra.toArray(String[]::new));
            if(id.equals("TR1")||id.equals("TR5"))creature=seed(seats.get(1),ZoneType.Battlefield,"C","Creature","0","PT:2/2");
            if(id.equals("TR5"))response=seed(seats.get(1),ZoneType.Hand,"R","Instant","U","A:SP$ ChangeZone | Cost$ U | ValidTgts$ Creature | Origin$ Battlefield | Destination$ Hand");
            if(id.equals("TR3")){
                List<String> lines=new ArrayList<>(List.of(trigger(false,"T","DB$ GainLife | ValidTgts$ Player | LifeAmount$ 2 | SubAbility$ Tail")));lines.add("SVar:Tail:DB$ GainLife | Defined$ You | LifeAmount$ 1");
                seed(seats.get(0),ZoneType.Battlefield,"W0","Artifact","0",lines.toArray(String[]::new));
            }
            if(id.equals("TR6"))for(int n=0;n<3;n++){
                List<String> lines=new ArrayList<>(List.of(trigger(false,"N","DB$ GainLife | Defined$ You | LifeAmount$ 1")));lines.addAll(List.of(trigger(false,"T","DB$ GainLife | ValidTgts$ Player | LifeAmount$ 2")));
                seed(seats.get(n),ZoneType.Battlefield,"W"+n,"Artifact","0",lines.toArray(String[]::new));
            }
        }
        String role(Card card){Assert.assertTrue(card.getName().startsWith("Spellwake "));return card.getName().substring(10);}
        Map<String,Object> card(Card card){Map<String,Object> v=new LinkedHashMap<>();v.put("id",card.getId());v.put("role",role(card));v.put("owner",card.getOwner().getName());v.put("controller",card.getController().getName());v.put("zone",card.getZone()==null?null:card.getZone().getZoneType().name());v.put("timestamp",card.getGameTimestamp());return v;}
        String definition(WrappedAbility ability){return ability.getTrigger().getParam("Execute");}
        List<String> stack(){List<String> values=new ArrayList<>();for(SpellAbilityStackInstance entry:game.getStack()){SpellAbility ability=entry.getSpellAbility();values.add(ability.getId()+":"+role(ability.getHostCard())+":"+(ability instanceof WrappedAbility w?definition(w):"spell"));}return values;}
        List<Map<String,Object>> stackEntries(){
            List<Map<String,Object>> values=new ArrayList<>();
            for(SpellAbilityStackInstance item:game.getStack()){
                SpellAbility ability=item.getSpellAbility();Map<String,Object> row=new LinkedHashMap<>();
                row.put("entry",ability.getId());row.put("source",card(ability.getHostCard()));row.put("controller",ability.getActivatingPlayer().getName());row.put("definition",ability instanceof WrappedAbility w?definition(w):"spell");
                List<String> selectedTargets=new ArrayList<>();List<Map<String,Object>> targetCards=new ArrayList<>();
                for(Card target:ability.getTargets().getTargetCards()){selectedTargets.add("Object:"+role(target));targetCards.add(card(target));}
                for(Player target:ability.getTargets().getTargetPlayers())selectedTargets.add("Player:"+target.getName());
                for(SpellAbility target:ability.getTargets().getTargetSpells())selectedTargets.add("Stack:"+target.getId()+":"+role(target.getHostCard()));
                row.put("targets",selectedTargets);row.put("target_cards",targetCards);values.add(row);
            }
            return values;
        }
        void capture(String point){Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);row.put("life",seats.stream().map(p->Map.of("player",p.getName(),"life",p.getLife())).toList());row.put("stack",stack());row.put("stack_entries",stackEntries());row.put("original",card(game.getCardState(original)));row.put("creature",creature==null?null:card(game.getCardState(creature)));row.put("casts",casts.size());row.put("placements",placements.size());row.put("bodies",bodies.size());rows.add(row);}
        SpellAbility expected(){return resolvingBody!=null?resolvingBody:admitting!=null?admitting:selected;}
        List<SpellAbility> choose(Player player){
            if(done||game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            if(!readyCaptured&&!casts.isEmpty()&&placements.size()==(id.equals("TR6")?6:1)&&!game.getStack().isFrozen()&&game.getStack().size()==(id.equals("TR2")?1:id.equals("TR6")?7:2)){capture("ready-after-placement");readyCaptured=true;}
            Card next=null;
            if(casts.isEmpty()&&player==caster&&game.getStack().isEmpty())next=game.getCardState(original);
            else if(id.equals("TR5")&&casts.size()==1&&placements.size()==1&&game.getStack().size()==2&&player==seats.get(1))next=game.getCardState(response);
            else if(casts.size()==(id.equals("TR5")?2:1)&&game.getStack().isEmpty()){capture("settled");done=true;return null;}
            if(next==null)return null;selected=next.getFirstSpellAbility();selected.setActivatingPlayer(player);Assert.assertTrue(selected.canPlay());player.getManaPool().addMana(new Mana(ManaAtom.fromName("U"),next,null,player));return List.of(selected);
        }
        boolean play(Player player,SpellAbility ability){
            Assert.assertSame(ability,selected);Assert.assertTrue(PlaySpellAbility.playSpellAbility(player.getController(),player,ability));Assert.assertEquals(player.getManaPool().getAmountOfColor(ManaAtom.fromName("U")),0);
            SpellAbility actual=game.getStack().peekAbility();Assert.assertTrue(actual.isSpell());Assert.assertFalse(actual.isCopied());Assert.assertSame(actual.getActivatingPlayer(),player);Assert.assertEquals(actual.getHostCard().getId(),ability.getHostCard().getId());
            if(casts.isEmpty())originalAbility=actual;Map<String,Object> row=new LinkedHashMap<>();row.put("entry",actual.getId());row.put("source",card(actual.getHostCard()));row.put("caster",player.getName());row.put("blue_after",0);casts.add(row);capture("paid-cast");return true;
        }
        void place(Player player,List<SpellAbility> offered){
            int expected=id.equals("TR6")?2:1;Assert.assertEquals(offered.size(),expected);List<WrappedAbility> ordered=new ArrayList<>();
            for(SpellAbility ability:offered){Assert.assertTrue(ability instanceof WrappedAbility);WrappedAbility w=(WrappedAbility)ability;Assert.assertSame(w.getActivatingPlayer(),player);Assert.assertEquals(w.getTrigger().getMode(),TriggerType.SpellCast);Assert.assertFalse(w.isOptionalTrigger());Assert.assertSame(w.getTriggeringObject(AbilityKey.SpellAbility),originalAbility);Assert.assertSame(w.getTriggeringObject(AbilityKey.Activator),caster);ordered.add(w);}
            if(id.equals("TR6")){List<String> desired=player==seats.get(2)?List.of("N","T"):List.of("T","N");ordered.sort(Comparator.comparingInt(w->desired.indexOf(definition(w))));Assert.assertEquals(orders.size(),List.of(seats.get(1),seats.get(2),seats.get(0)).indexOf(player));orders.add(Map.of("controller",player.getName(),"definitions",ordered.stream().map(this::definition).toList()));}
            for(WrappedAbility ability:ordered){
                Map<String,Object> row=new LinkedHashMap<>();row.put("entry",ability.getId());row.put("body_id",ability.getWrappedAbility().getId());row.put("source",card(ability.getHostCard()));row.put("controller",player.getName());row.put("definition",definition(ability));SpellAbility cause=(SpellAbility)ability.getTriggeringObject(AbilityKey.SpellAbility);row.put("cause_entry",cause.getId());row.put("cause",card(cause.getHostCard()));row.put("caster",((Player)ability.getTriggeringObject(AbilityKey.Activator)).getName());row.put("stack_before",stack());
                row.put("pre_setup_domain",ability.usesTargeting()?domain(ability,id.equals("TR4")):List.of());admitting=ability;boolean accepted;try{accepted=PlaySpellAbility.playSpellAbility(player.getController(),player,ability);}finally{admitting=null;}
                row.put("accepted",accepted);row.put("stack_after",stack());placements.add(row);
                if(id.equals("TR2")){Assert.assertFalse(accepted);Assert.assertFalse(allowedBodies.containsKey(ability.getWrappedAbility()));}else{Assert.assertTrue(accepted);allowedBodies.put(ability.getWrappedAbility(),row);}
            }
            capture("normal-controller-cohort");
        }
        List<String> domain(SpellAbility ability,boolean spell){
            List<String> values=new ArrayList<>();
            if(spell){for(SpellAbilityStackInstance row:game.getStack()){SpellAbility candidate=row.getSpellAbility();if(ability.canTarget(candidate))values.add("Stack:"+candidate.getId()+":"+role(candidate.getHostCard()));}}
            else for(GameEntity candidate:ability.getTargetRestrictions().getAllCandidates(ability))values.add(candidate instanceof Player p?"Player:"+p.getName():"Object:"+role((Card)candidate));
            return values;
        }
        boolean target(Player player,SpellAbility ability){
            Assert.assertSame(ability,expected());boolean spell=id.equals("TR4")&&ability==admitting;List<String> domain=domain(ability,spell);
            Map<String,Object> row=new LinkedHashMap<>();row.put("ability",ability.getId());row.put("callback_kind",ability.getClass().getName());row.put("controller",player.getName());row.put("source",card(ability.getHostCard()));row.put("domain",domain);row.put("trigger",ability==admitting);
            if(id.equals("TR2")){Assert.assertTrue(domain.isEmpty());row.put("selected",null);row.put("accepted",false);targets.add(row);return false;}
            if(spell){Assert.assertEquals(domain,List.of("Stack:"+originalAbility.getId()+":S"));ability.getTargets().add(originalAbility);row.put("selected","Stack:"+originalAbility.getId()+":S");}
            else if(id.equals("TR3")||id.equals("TR6")){Assert.assertEquals(new HashSet<>(domain),Set.of("Player:P0","Player:P1","Player:P2"));Player chosen=id.equals("TR3")?seats.get(2):seats.get((seats.indexOf(player)+1)%3);Assert.assertTrue(ability.canTarget(chosen));ability.getTargets().add(chosen);row.put("selected","Player:"+chosen.getName());}
            else{Assert.assertEquals(domain,List.of("Object:C"));Card chosen=game.getCardState(creature);Assert.assertTrue(ability.canTarget(chosen));ability.getTargets().add(chosen);row.put("selected","Object:C");row.put("selected_card",card(chosen));}
            Assert.assertTrue(ability.isTargetNumberValid());row.put("accepted",true);targets.add(row);return true;
        }
        void body(Player player,SpellAbility ability,boolean setup){
            Assert.assertFalse(setup);Map<String,Object> placement=allowedBodies.remove(ability);Assert.assertNotNull(placement);Assert.assertSame(player,ability.getActivatingPlayer());capture("before-trigger-body");resolvingBody=ability;
            try{Assert.assertTrue(PlaySpellAbility.playSpellAbilityNoStack(player.getController(),player,ability,true));}finally{resolvingBody=null;}
            bodies.add(Map.of("entry",placement.get("entry"),"body_id",ability.getId(),"source",placement.get("source"),"controller",player.getName(),"definition",placement.get("definition")));capture("after-trigger-body");
        }
        void verify(){
            Assert.assertEquals(payments,id.equals("TR5")?2:1);Assert.assertEquals(placements.size(),id.equals("TR6")?6:1);
            if(id.equals("TR2"))Assert.assertTrue(targets.size()<=1);else Assert.assertEquals(targets.size(),id.equals("TR6")?3:id.equals("TR5")?2:1);
            Assert.assertEquals(bodies.size(),id.equals("TR2")||id.equals("TR5")?0:id.equals("TR6")?6:1);
            Assert.assertEquals(allowedBodies.size(),id.equals("TR5")?1:0);Assert.assertTrue(game.getStack().isEmpty());Assert.assertTrue(readyCaptured);
            int[] life=id.equals("TR3")?new int[]{21,22,22}:id.equals("TR6")?new int[]{23,25,23}:new int[]{id.equals("TR1")?23:id.equals("TR5")?22:20,20};
            for(int n=0;n<life.length;n++)Assert.assertEquals(seats.get(n).getLife(),life[n]);
            Assert.assertEquals(game.getCardState(original).getZone().getZoneType(),id.equals("TR2")?ZoneType.Battlefield:ZoneType.Graveyard);
            if(creature!=null)Assert.assertEquals(game.getCardState(creature).getZone().getZoneType(),ZoneType.Hand);
        }
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
        @Override public SpellAbility getAbilityToPlay(Card host,List<SpellAbility> abilities,ITriggerEvent event){Assert.assertEquals(abilities.size(),1);Assert.assertSame(abilities.get(0),script.expected());return abilities.get(0);}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){script.place(getPlayer(),abilities);}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void playSpellAbilityNoStack(SpellAbility ability,boolean setup){script.body(getPlayer(),ability,setup);}
        @Override public boolean chooseTargetsFor(SpellAbility ability){return script.target(getPlayer(),ability);}
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
        @Override public List<CostPart> orderCosts(List<CostPart> costs){for(CostPart cost:costs)Assert.assertTrue(cost instanceof CostPartMana);return costs;}
        @Override public boolean payManaCost(ManaCost cost,CostPartMana part,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.expected());Assert.assertEquals(effect,ability.isTrigger());if(ability!=script.selected)Assert.assertTrue(cost.isZero());return PlaySpellAbility.payManaCost(this,cost,part,ability,getPlayer(),prompt,matrix,effect);}
        @Override public boolean applyManaToCost(ManaCostBeingPaid cost,SpellAbility ability,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(ability,script.expected());Assert.assertSame(ability,script.selected);Assert.assertFalse(effect);script.payments++;getPlayer().pushPaidForSA(ability);ability.setManaCostBeingPaid(cost);try{Assert.assertTrue(getPlayer().getManaPool().tryPayCostWithColor(ManaAtom.fromName("U"),ability,cost,ability.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{ability.setManaCostBeingPaid(null);getPlayer().popPaidForSA();}}
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player,SpellAbility ability,boolean effect,String prompt){Assert.assertSame(ability,script.expected());Assert.assertEquals(effect,ability.isTrigger());return new StrictRejectingController.Costs(player,ability,effect){@Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected cost "+method);}@Override public boolean paysRightAfterDecision(){return true;}@Override public PaymentDecision visit(CostPartMana cost){return PaymentDecision.number(0);}};}
    }
}
