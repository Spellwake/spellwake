package forge.ai;

import com.google.common.collect.*;
import com.google.common.eventbus.Subscribe;
import com.google.gson.*;
import forge.LobbyPlayer;
import forge.MulliganDefs;
import forge.StaticData;
import forge.card.*;
import forge.card.mana.*;
import forge.deck.*;
import forge.game.*;
import forge.game.ability.effects.RollDiceEffect;
import forge.game.card.*;
import forge.game.card.sticker.Sticker;
import forge.game.combat.*;
import forge.game.cost.*;
import forge.game.event.*;
import forge.game.keyword.KeywordInterface;
import forge.game.mana.*;
import forge.game.phase.PhaseType;
import forge.game.player.*;
import forge.game.replacement.ReplacementEffect;
import forge.game.spellability.*;
import forge.game.staticability.StaticAbility;
import forge.game.trigger.WrappedAbility;
import forge.game.zone.*;
import forge.item.*;
import forge.model.FModel;
import forge.util.*;
import forge.util.collect.FCollectionView;
import org.apache.commons.lang3.tuple.*;
import org.testng.annotations.Test;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.function.*;

/** Complete restricted registered-deck games. Only frozen input recipes drive play. */
public class OrdinaryGameReferenceTest extends AITest {
    static final String PIN = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    static final String INPUT_SHA = "9d460e7f930c85dac30e0adf1897da689091b8992d9daad07bd68f42db1f2e32";
    static final Gson JSON = new GsonBuilder().serializeNulls().disableHtmlEscaping().setObjectToNumberStrategy(ToNumberPolicy.LONG_OR_DOUBLE).create();
    @Test public void g01() throws Exception { run("G01"); }
    @Test public void g02a() throws Exception { run("G02a"); }
    @Test public void g02b() throws Exception { run("G02b"); }
    @Test public void g03() throws Exception { run("G03"); }

    @Test public void forbiddenControllerLeaves() throws Exception {
        JsonObject root=loadInputs();
        JsonObject fixture=objects(root.getAsJsonArray("cases")).stream().filter(c->str(c,"case_id").equals("G01")).findFirst().orElseThrow();
        List<Map<String,Object>> controls=new ArrayList<>();
        for(String method:List.of("tempShowCards","endTempShowCards","sideboardsByItself","getSideboardingReport","getAnteResult")){
            Script s=new Script(root,fixture);s.construct();
            StrictController controller=(StrictController)s.seats.get("A").getController();
            AssertionError rejected=null;
            try {
                switch(method){
                    case "tempShowCards" -> controller.tempShowCards(List.of());
                    case "endTempShowCards" -> controller.endTempShowCards();
                    case "sideboardsByItself" -> controller.sideboardsByItself();
                    case "getSideboardingReport" -> controller.getSideboardingReport();
                    case "getAnteResult" -> controller.getAnteResult();
                    default -> throw new IllegalArgumentException(method);
                }
            }catch(AssertionError error){rejected=error;}
            if(rejected==null||!rejected.getMessage().equals("Unexpected callback "+method))throw new AssertionError("Unsupported controller leaf was not rejected: "+method,rejected);
            List<Map<String,Object>> callbacks=s.streams.get("callbacks");
            if(s.failures.size()!=1||callbacks.size()!=2||!callbacks.get(0).get("kind").equals("entry")||!callbacks.get(1).get("kind").equals("failure")||!s.callStack.isEmpty())throw new AssertionError("Missing fatal callback evidence: "+method);
            boolean sticky=false;
            try{s.healthy();}catch(AssertionError error){sticky=error.getMessage().equals("Sticky observer/protocol failure");}
            if(!sticky)throw new AssertionError("Unsupported callback failure was not sticky: "+method);
            controls.add(m("method",method,"rejected",true,"sticky_failure",true,"callbacks",callbacks,"failures",s.failures));
        }
        Path output=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")),"strict-controller-controls.json");
        Files.writeString(output,JSON.toJson(m("schema_version",1,"kind","ordinary-forbidden-controller-leaf-controls","magic_games_started",0,"controls",controls))+"\n",StandardOpenOption.CREATE_NEW);
    }

    static Map<String,Object> m(Object... items) {
        Map<String,Object> out=new LinkedHashMap<>();
        for(int i=0;i<items.length;i+=2) out.put((String)items[i],items[i+1]);
        return out;
    }
    static String sha(byte[] bytes) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
    }
    static String str(JsonObject o,String k) { return o.get(k).getAsString(); }
    static int number(JsonObject o,String k) { return o.get(k).getAsInt(); }
    static List<JsonObject> objects(JsonArray a) {
        List<JsonObject> out=new ArrayList<>(); for(JsonElement x:a)out.add(x.getAsJsonObject()); return out;
    }
    static List<String> strings(JsonArray a) {
        List<String> out=new ArrayList<>(); for(JsonElement x:a)out.add(x.getAsString()); return out;
    }
    static JsonObject loadInputs() throws Exception {
        Path input=Path.of(Objects.requireNonNull(System.getProperty("spellwake.inputs.path")));
        byte[] bytes=Files.readAllBytes(input);
        if(!sha(bytes).equals(INPUT_SHA))throw new AssertionError("Frozen input bytes mismatch");
        return JsonParser.parseString(new String(bytes,StandardCharsets.UTF_8)).getAsJsonObject();
    }
    synchronized void run(String id) throws Exception {
        JsonObject root=loadInputs();
        JsonObject fixture=null;
        for(JsonObject c:objects(root.getAsJsonArray("cases")))if(str(c,"case_id").equals(id))fixture=c;
        if(fixture==null)throw new AssertionError("Missing frozen input case");
        Script s=new Script(root,fixture);
        Random oldRandom=MyRandom.getRandom();
        MulliganDefs.MulliganRule oldRule=StaticData.instance().getMulliganRule();
        boolean oldFilter=StaticData.instance().getFilteredHandsEnabled();
        Throwable thrown=null;
        s.loaderClasses=runtimeClasses();
        try {
            s.construct();
            StaticData.instance().setMulliganRule(MulliganDefs.MulliganRule.London);
            StaticData.instance().setFilteredHandsEnabled(false);
            s.entropy.installedCallCount=MyRandom.callCount();
            MyRandom.setRandom(s.entropy);
            s.game.subscribeToEvents(new Events(s));
            s.add("operations","start_game_entry",m("hook",null,"registered_inventory",s.fixture.get("inventory")));
            s.match.startGame(s.game,null);
            s.naturalReturn=true;
            long seq=s.add("operations","start_game_return",m("context",s.header()));
            s.observe("terminal.return",seq,true,m("kind","natural_return"));
            s.finish();
        } catch(Throwable t) { thrown=t; if(s.failures.isEmpty())s.fatal("uncaught",m("class",t.getClass().getName(),"message",t.getMessage())); s.failCallbacks(t); }
        finally {
            MyRandom.setRandom(oldRandom); StaticData.instance().setMulliganRule(oldRule); StaticData.instance().setFilteredHandsEnabled(oldFilter);
            Path dir=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
            Files.writeString(dir.resolve(id+".json"),JSON.toJson(s.output())+"\n",StandardOpenOption.CREATE_NEW);
        }
        if(thrown!=null)throw new AssertionError("Recorded ordinary game failure "+id,thrown);
    }

    static List<Map<String,Object>> runtimeClasses() throws Exception {
        List<Map<String,Object>> out=new ArrayList<>();
        String[] names={"forge.ai.OrdinaryGameReferenceTest","forge.ai.OrdinaryGameReferenceTest$Script","forge.ai.OrdinaryGameReferenceTest$StrictLobby","forge.ai.OrdinaryGameReferenceTest$StrictController","forge.ai.OrdinaryGameReferenceTest$StrictCosts","forge.ai.OrdinaryGameReferenceTest$RecordedEntropy","forge.ai.OrdinaryGameReferenceTest$Events","forge.ai.AITest","forge.card.CardRules","forge.card.CardType","forge.item.PaperCard","forge.game.GameEntity","forge.game.GameOutcome","forge.game.player.PlayerOutcome","forge.game.card.CounterEnumType","forge.game.Game","forge.game.Match","forge.game.GameAction","forge.game.GameActionUtil","forge.game.phase.PhaseHandler","forge.game.player.Player","forge.game.player.PlayerController","forge.game.player.PlaySpellAbility","forge.game.card.Card","forge.game.card.CardFactory","forge.game.card.CardState","forge.game.zone.Zone","forge.game.zone.PlayerZone","forge.game.zone.MagicStack","forge.game.cost.Cost","forge.game.cost.CostPayment","forge.game.cost.CostTap","forge.game.cost.CostPartMana","forge.game.mana.ManaPool","forge.game.mana.ManaCostBeingPaid","forge.game.spellability.SpellPermanent","forge.game.spellability.AbilityManaPart","forge.game.combat.Combat","forge.game.combat.CombatUtil","forge.util.MyRandom","forge.util.MyRandom$CountingRandom","forge.util.Aggregates","java.util.Random","java.util.Collections","java.lang.StackWalker","com.google.common.eventbus.EventBus","com.google.gson.Gson"};
        for(String name:names){Class<?> type=Class.forName(name);String resource="/"+name.replace('.','/')+".class";byte[] bytes;try(InputStream in=type.getResourceAsStream(resource)){if(in==null)throw new AssertionError("Missing class bytes "+name);bytes=in.readAllBytes();}Path file=Path.of(System.getProperty("spellwake.evidence.dir"),"loader-classes",name.replace('.','/')+".class");Files.createDirectories(file.getParent());if(Files.exists(file)){if(!Arrays.equals(bytes,Files.readAllBytes(file)))throw new AssertionError("Changed loaded class "+name);}else Files.write(file,bytes,StandardOpenOption.CREATE_NEW);out.add(m("class",name,"code_source",type.getProtectionDomain().getCodeSource()==null?null:type.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",type.getResource(resource).toString(),"sha256",sha(bytes)));}
        return out;
    }

    /** No AI controller is ever installed, including beneath terminal override clearing. */
    static final class StrictLobby extends LobbyPlayer implements IGameEntitiesFactory {
        final Script script; final String seat;
        StrictLobby(String seat,Script script){super("ordinary-"+seat);this.seat=seat;this.script=script;}
        @Override public Player createIngamePlayer(Game game,int id){
            Player p=new Player(getName(),game,id);
            p.setFirstController(new StrictController(game,p,this,script));
            script.seats.put(seat,p); return p;
        }
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw script.bad("mind_control_factory",m("master",master.getId(),"slave",slave.getId()));}
        @Override public void hear(LobbyPlayer player,String message){throw script.bad("lobby_message",m("sender",player.getName(),"message",message));}
    }

    static final class Call {
        final long id,sequence; final String method; final Player player;
        final List<String> stackTags;
        Call(long id,long sequence,String method,Player player,List<String> stackTags){this.id=id;this.sequence=sequence;this.method=method;this.player=player;this.stackTags=stackTags;}
    }
    static final class Produced {
        final Mana mana; final String source,operation; final long timestamp;
        Produced(Mana mana,String source,String operation,long timestamp){this.mana=mana;this.source=source;this.operation=operation;this.timestamp=timestamp;}
    }

    static final class Script {
        final JsonObject root,fixture; final String id;
        Game game; Match match;
        final Map<String,Player> seats=new LinkedHashMap<>();
        final Map<String,PaperCard> papers=new LinkedHashMap<>();
        final Map<String,Object> paperCards=new LinkedHashMap<>();
        final Map<Integer,String> aliases=new LinkedHashMap<>();
        final Map<String,Integer> physical=new LinkedHashMap<>();
        final Map<String,JsonObject> inventory=new LinkedHashMap<>();
        final List<JsonObject> actions,decisions,opening;
        final Set<Integer> openingUsed=new LinkedHashSet<>();
        int actionCursor,decisionCursor,choiceCursor,orderCount,priorityCount;
        final Set<String> assigned=new LinkedHashSet<>(),complained=new LinkedHashSet<>(),unsupported=new LinkedHashSet<>();
        final Set<String> housekeepingSeen=new LinkedHashSet<>();
        final Map<String,Integer> housekeepingCounts=new LinkedHashMap<>();
        final IdentityHashMap<Object,String> objectReferences=new IdentityHashMap<>();
        final Map<String,Produced> produced=new LinkedHashMap<>();
        final Map<String,List<Map<String,Object>>> streams=new LinkedHashMap<>();
        final List<Map<String,Object>> failures;
        final Map<String,Object> checkpoints=new LinkedHashMap<>();
        final List<Map<String,Object>> observations=new ArrayList<>();
        final Set<String> observed=new LinkedHashSet<>();
        final Deque<Call> callStack=new ArrayDeque<>();
        final Set<Integer> playerDamageTurns=new LinkedHashSet<>();
        final RecordedEntropy entropy;
        long global,callIdentity,ordinaryIdentity;
        boolean naturalReturn,finishedEvent;
        List<Map<String,Object>> loaderClasses=List.of();
        SpellAbility selected; JsonObject selectedAction; String ordinaryOperation;
        JsonObject resolving,discardPending;
        Script(JsonObject root,JsonObject fixture){
            this.root=root;this.fixture=fixture;id=str(fixture,"case_id");
            actions=objects(fixture.getAsJsonArray("actions"));decisions=objects(fixture.getAsJsonArray("decisions"));opening=objects(fixture.getAsJsonArray("opening_policy"));
            for(String k:List.of("callbacks","events","random","operations","failures"))streams.put(k,new ArrayList<>());
            failures=streams.get("failures");
            for(JsonObject row:objects(fixture.getAsJsonArray("inventory")))inventory.put(str(row,"alias"),row);
            entropy=new RecordedEntropy(this);
        }
        void construct(){
            require(str(root,"schema").equals("ordinary-game-inputs-v1"),"input_schema",m());
            for(String key:root.getAsJsonObject("definitions").keySet()){
                JsonObject d=root.getAsJsonObject("definitions").getAsJsonObject(key);
                PaperCard p=FModel.getMagicDb().getCommonCards().getCard(str(d,"name"),str(d,"set").toUpperCase(Locale.ROOT),str(d,"collector_number"));
                require(p!=null,"printing_missing",m("definition",key));
                CardRules rules=p.getRules();CardType type=rules.getType();
                paperCards.put(key,m("name",p.getName(),"set",p.getEdition(),"collector_number",p.getCollectorNumber(),"foil",p.isFoil(),"art_index",p.getArtIndex(),"functional_variant",p.getFunctionalVariant(),"oracle_text",rules.getOracleText(),"mana_cost",rules.getManaCost().toString(),"printed_power",rules.getPower(),"printed_toughness",rules.getToughness(),"type",type.toString(),"core_types",type.getCoreTypes().stream().map(Enum::name).sorted().toList(),"supertypes",type.getSupertypes().stream().map(Enum::name).sorted().toList(),"subtypes",type.getSubtypes().stream().sorted().toList(),"basic_land",type.isBasicLand(),"creature",type.isCreature()));
                require(p.getName().equals(str(d,"name"))&&p.getEdition().equalsIgnoreCase(str(d,"set"))&&p.getCollectorNumber().equals(str(d,"collector_number"))&&!p.isFoil(),"printing_identity",m("definition",key));
                require(p.getRules().getOracleText().equals(key.startsWith("plains")?"({T}: Add {W}.)":""),"printing_text",m("definition",key));
                if(key.startsWith("silvercoat"))require(p.getRules().getManaCost().toString().equals("{1}{W}")&&p.getRules().getIntPower()==2&&p.getRules().getIntToughness()==2,"lion_semantics",m());
                require(key.startsWith("plains")?type.isBasicLand()&&type.hasSubtype("Plains"):type.isCreature()&&type.hasSubtype("Cat"),"printing_type",m("definition",key));
                papers.put(key,p);
            }
            List<RegisteredPlayer> registered=new ArrayList<>();
            for(String seat:List.of("A","B")){
                Deck d=new Deck("ordinary-"+id+"-"+seat);
                for(JsonObject row:inventory.values())if(str(row,"owner").equals(seat))d.getMain().add(papers.get(str(row,"definition_ref")),1);
                require(d.getMain().countAll()==60,"deck_count",m("seat",seat));
                RegisteredPlayer rp=new RegisteredPlayer(d).setPlayer(new StrictLobby(seat,this));
                rp.setStartingLife(20);rp.setStartingHand(7);rp.setTeamNumber(seat.equals("A")?1:2);rp.setRandomFoil(false);registered.add(rp);
            }
            GameRules rules=new GameRules(GameType.Constructed);rules.setOrderCombatants(false);
            match=new Match(rules,registered,"Ordinary strict reference "+id);game=match.createGame();
            for(Player p:seats.values())strict(p);
        }
        void strict(Player p){require(p.getController().getClass()==StrictController.class&&!p.getController().isAI()&&!p.getController().isGuiPlayer()&&!p.getController().isOrderedZone()&&!p.getController().canPlayUnlimitedLands()&&p.getController().getFullControl().isEmpty(),"strict_base_controller",m("player_id",p.getId()));}
        long add(String stream,String kind,Map<String,Object> fields){
            List<Map<String,Object>> target=streams.get(stream);
            Map<String,Object> row=m("ordinal",target.size(),"global_sequence",++global,"kind",kind,"case_id",id);
            row.putAll(fields);target.add(JSON.fromJson(JSON.toJson(row),new com.google.gson.reflect.TypeToken<LinkedHashMap<String,Object>>(){}.getType()));return global;
        }
        void fatal(String code,Map<String,Object> detail){add("failures","fatal",m("code",code,"detail",detail,"callback_id",callStack.isEmpty()?null:callStack.peek().id));}
        void failCallbacks(Throwable cause){while(!callStack.isEmpty()){Call c=callStack.pop();add("callbacks","failure",m("callback_id",c.id,"method",c.method,"actor",seat(c.player),"exception_class",cause.getClass().getName(),"message",cause.getMessage()));}}
        AssertionError bad(String code,Map<String,Object> detail){fatal(code,detail);return new AssertionError(code+": "+JSON.toJson(detail));}
        void require(boolean condition,String code,Map<String,Object> detail){if(!condition)throw bad(code,detail);}
        void healthy(){if(!failures.isEmpty())throw new AssertionError("Sticky observer/protocol failure");}
        String seat(Player p){if(p==null)return null;for(var e:seats.entrySet())if(e.getValue()==p)return e.getKey();throw bad("unknown_seat",m("id",p.getId()));}
        String seatView(PlayerView p){if(p==null)return null;for(var e:seats.entrySet())if(e.getValue().getId()==p.getId())return e.getKey();throw bad("unknown_seat_view",m("id",p.getId()));}
        int turn(){return game==null?0:game.getPhaseHandler().getTurn();}
        int personal(){return (turn()+1)/2;}
        String step(){PhaseType p=game==null?null:game.getPhaseHandler().getPhase();if(p==null)return null;return switch(p){
            case UNTAP->"untap";case UPKEEP->"upkeep";case DRAW->"draw";case MAIN1->"precombat_main";case COMBAT_BEGIN->"beginning_combat";case COMBAT_DECLARE_ATTACKERS->"declare_attackers";case COMBAT_DECLARE_BLOCKERS->"declare_blockers";case COMBAT_FIRST_STRIKE_DAMAGE->"first_strike_damage";case COMBAT_DAMAGE->"combat_damage";case COMBAT_END->"end_combat";case MAIN2->"postcombat_main";case END_OF_TURN->"end";case CLEANUP->"cleanup";default->throw bad("phase_variant",m("phase",p.name()));};}
        Map<String,Object> pool(Player p){Map<String,Object> out=new LinkedHashMap<>();for(char c:"WUBRGC".toCharArray())out.put(String.valueOf(c),String.valueOf(p.getManaPool().getAmountOfColor(ManaAtom.fromName(String.valueOf(c)))));return out;}
        Map<String,Object> header(){
            if(game==null)return m("stage","construction");
            Map<String,Object> players=new LinkedHashMap<>();
            for(var e:seats.entrySet()){Player p=e.getValue();Map<String,Object> counts=new LinkedHashMap<>();for(ZoneType z:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command))counts.put(z.name().toLowerCase(Locale.ROOT),p.getCardsIn(z).size());players.put(e.getKey(),m("life",String.valueOf(p.getLife()),"mana",pool(p),"counts",counts,"lost",p.hasLost()));}
            return m("age",game.getAge().name(),"individual_turn",turn(),"personal_turn",personal(),"step",step(),"active",seat(game.getPhaseHandler().getPlayerTurn()),"raw_priority",seat(game.getPhaseHandler().getPriorityPlayer()),"stack_size",game.getStack().size(),"game_over",game.isGameOver(),"materialized_aliases",aliases.size(),"action_cursor",actionCursor,"decision_cursor",decisionCursor,"players",players);
        }
        Call enter(Player p,String method,Map<String,Object> arguments){
            healthy();strict(p);
            require(++choiceCursor<=fixture.getAsJsonObject("watchdog").get("maximum_callbacks").getAsInt(),"callback_watchdog",m());
            long cid=++callIdentity;List<Map<String,Object>> frames=stackFrames();List<String> tags=tagsFromFrames(frames);
            long seq=add("callbacks","entry",m("callback_id",cid,"parent_callback_id",callStack.isEmpty()?null:callStack.peek().id,"method",method,"callback_kind",method.startsWith("visit_")?"cost_visitor":"controller","actor",seat(p),"controller_class",p.getController().getClass().getName(),"arguments",arguments,"context",header(),"operation_id",ordinaryOperation,"actual_stack_tags",tags,"actual_stack_frames",frames));
            Call call=new Call(cid,seq,method,p,tags);callStack.push(call);return call;
        }
        void callerPath(Call call,String... expected){
            String anchor="forge.ai.OrdinaryGameReferenceTest$StrictController."+call.method;int offset=call.stackTags.indexOf(anchor);
            require(offset>=0&&call.stackTags.lastIndexOf(anchor)==offset&&call.stackTags.size()>=offset+1+expected.length&&call.stackTags.subList(offset+1,offset+1+expected.length).equals(List.of(expected)),"callback_source_path",m("callback_id",call.id,"method",call.method,"required_path",List.of(expected),"actual_stack_tags",call.stackTags));
        }
        long leave(Call c,Object reply){healthy();require(callStack.peek()==c,"callback_nesting",m("callback_id",c.id));callStack.pop();return add("callbacks","return",m("callback_id",c.id,"method",c.method,"actor",seat(c.player),"reply",reply,"context",header(),"operation_id",ordinaryOperation));}
        AssertionError unexpected(Player p,String method,Object... args){Call c=enter(p,method,m("offered",diagnostics(args)));fatal("unexpected_callback",m("method",method,"actor",seat(p),"callback_id",c.id));add("callbacks","failure",m("callback_id",c.id,"method",method));callStack.pop();return new AssertionError("Unexpected callback "+method);}
        Object diagnostics(Object[] args){List<Object> out=new ArrayList<>();for(Object a:args)out.add(diagnostic(a,0));return out;}
        Object diagnostic(Object value,int depth){
            if(value==null||value instanceof String||value instanceof Number||value instanceof Boolean)return value;
            if(value instanceof Enum<?> e)return m("actual_class",e.getClass().getName(),"value",e.name());
            if(value instanceof Card c)return m("actual_class",c.getClass().getName(),"physical_id",String.valueOf(c.getId()),"known_alias",aliases.get(c.getId()),"name",c.getName());
            if(value instanceof CardView c)return m("actual_class",c.getClass().getName(),"physical_id",String.valueOf(c.getId()),"known_alias",aliases.get(c.getId()));
            if(value instanceof Player p)return m("actual_class",p.getClass().getName(),"player_id",p.getId(),"seat",seat(p));
            if(value instanceof SpellAbility sa)return m("actual_class",sa.getClass().getName(),"ability_id",sa.getId(),"source",diagnostic(sa.getHostCard(),depth+1),"spell",sa.isSpell(),"mana_ability",sa.isManaAbility());
            if(value instanceof PaperCard p)return m("actual_class",p.getClass().getName(),"name",p.getName(),"set",p.getEdition(),"collector_number",p.getCollectorNumber(),"foil",p.isFoil());
            if(value instanceof CostPart cost)return m("actual_class",cost.getClass().getName(),"cost",cost.toString());
            if(value instanceof Cost cost)return m("actual_class",cost.getClass().getName(),"parts",diagnostic(cost.getCostParts(),depth+1));
            if(depth<6&&value instanceof Map<?,?> map){List<Object> entries=new ArrayList<>();for(var e:map.entrySet())entries.add(m("key",diagnostic(e.getKey(),depth+1),"value",diagnostic(e.getValue(),depth+1)));return m("actual_class",value.getClass().getName(),"entries",entries);}
            if(depth<6&&value instanceof Iterable<?> values){List<Object> entries=new ArrayList<>();for(Object x:values)entries.add(diagnostic(x,depth+1));return m("actual_class",value.getClass().getName(),"values",entries);}
            if(depth<6&&value.getClass().isArray()){List<Object> entries=new ArrayList<>();for(int i=0;i<java.lang.reflect.Array.getLength(value);i++)entries.add(diagnostic(java.lang.reflect.Array.get(value,i),depth+1));return m("actual_class",value.getClass().getName(),"values",entries);}
            return m("actual_class",value.getClass().getName(),"unprojected_fatal_argument",true);
        }
        Object objectRef(Object value){if(value==null)return null;return m("object_ref",objectReferences.computeIfAbsent(value,k->"object-"+(objectReferences.size()+1)),"actual_class",value.getClass().getName());}
        String alias(Card c){String a=aliases.get(c.getId());if(a==null)throw bad("unknown_card",m("physical_id",c.getId(),"name",c.getName()));return a;}
        List<Card> actualCards(){List<Card> out=new ArrayList<>();for(Player p:seats.values())for(ZoneType z:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command))for(Card c:p.getCardsIn(z))out.add(c);for(Card c:game.getStackZone().getCards())out.add(c);return out;}
        Card card(String alias){Integer id=physical.get(alias);require(id!=null,"unmaterialized_card",m("alias",alias));Card found=null;for(Card c:actualCards())if(c.getId()==id){require(found==null,"duplicate_physical_membership",m("alias",alias));found=c;}require(found!=null,"missing_physical",m("alias",alias));return found;}
        List<String> sequence(Iterable<Card> cards){List<String> out=new ArrayList<>();for(Card c:cards)out.add(alias(c));return out;}
        List<String> domain(Iterable<Card> cards){List<String> out=sequence(cards);Collections.sort(out);return out;}
        Map<String,Object> cardState(Card c){boolean bf=c.isInPlay(),creature=bf&&c.isCreature();return m("physical_id",String.valueOf(c.getId()),"timestamp",String.valueOf(c.getGameTimestamp()),"owner",seat(c.getOwner()),"controller",bf||c.isInZone(ZoneType.Stack)?seat(c.getController()):null,"zone",c.getZone().getZoneType().name().toLowerCase(Locale.ROOT),"tapped",bf?c.isTapped():null,"power",creature?String.valueOf(c.getNetPower()):null,"toughness",creature?String.valueOf(c.getNetToughness()):null,"marked_damage",creature?String.valueOf(c.getDamage()):null,"sick",creature?c.isSick():null,"plus_one_counters",creature?String.valueOf(c.getCounters(CounterEnumType.P1P1)):null,"minus_one_counters",creature?String.valueOf(c.getCounters(CounterEnumType.M1M1)):null);}
        Map<String,Object> outcome(){
            if(game==null||game.getOutcome()==null)return null;
            GameOutcome actual=game.getOutcome();RegisteredPlayer winner=actual.getWinningPlayer();String winningSeat=null;
            Map<String,Object> players=new LinkedHashMap<>();
            for(var e:seats.entrySet()){
                Player p=e.getValue();PlayerOutcome result=p.getOutcome();
                require(result!=null,"missing_registered_outcome",m("seat",e.getKey()));
                if(winner==p.getRegisteredPlayer()){require(winningSeat==null,"duplicate_winner",m());winningSeat=e.getKey();}
                players.put(e.getKey(),m("won",result.hasWon(),"lost",p.hasLost(),"loss_reason",result.lossState==null?null:result.lossState.name(),"alternative_win_source",result.altWinSourceName,"lose_condition_spell",result.loseConditionSpell,"game_outcome_is_winner",actual.isWinner(p.getRegisteredPlayer())));
            }
            require(winner==null||winningSeat!=null,"unknown_registered_winner",m());
            return m("end_reason",actual.getWinCondition().name(),"draw",actual.isDraw(),"winning_seat",winningSeat,"winning_team",actual.getWinningTeam(),"players",players);
        }
        Map<String,Object> world(){
            Map<String,Object> cards=new LinkedHashMap<>(),players=new LinkedHashMap<>();Set<Integer> seen=new HashSet<>();
            for(Card c:actualCards()){require(seen.add(c.getId()),"world_duplicate",m("id",c.getId()));cards.put(alias(c),cardState(c));}
            require(cards.size()==aliases.size(),"world_inventory",m("cards",cards.size(),"aliases",aliases.size()));
            for(var e:seats.entrySet()){Player p=e.getValue();List<String> gy=sequence(p.getCardsIn(ZoneType.Graveyard));Collections.reverse(gy);players.put(e.getKey(),m("life",String.valueOf(p.getLife()),"mana",pool(p),"library",sequence(p.getCardsIn(ZoneType.Library)),"hand",domain(p.getCardsIn(ZoneType.Hand)),"battlefield",domain(p.getCardsIn(ZoneType.Battlefield)),"graveyard",gy,"exile",domain(p.getCardsIn(ZoneType.Exile)),"won",p.getOutcome()==null?null:p.getOutcome().hasWon(),"lost",p.hasLost(),"loss_reason",p.getOutcome()==null||p.getOutcome().lossState==null?null:p.getOutcome().lossState.name(),"lands_played",p.getLandsPlayedThisTurn()));}
            List<Object> stack=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())stack.add(m("stack_id",si.getId(),"ability",ability(si.getSpellAbility()),"controller",seat(si.getActivatingPlayer())));Collections.reverse(stack);
            return m("header",header(),"cards",cards,"players",players,"stack",stack);
        }
        Map<String,Object> registeredPlayers(){Map<String,Object> out=new LinkedHashMap<>();for(var entry:seats.entrySet())out.put(entry.getKey(),m("player_id",entry.getValue().getId()));return out;}
        Map<String,Object> otherKnownWorld(String owner){
            String other=owner.equals("A")?"B":"A";Player player=seats.get(other);
            Map<String,Object> cards=new TreeMap<>(),zones=new LinkedHashMap<>();
            for(Card card:actualCards())if(card.getOwner()==player)require(cards.put(alias(card),cardState(card))==null,"other_world_duplicate",m("card",card.getId()));
            long known=physical.keySet().stream().filter(a->str(inventory.get(a),"owner").equals(other)).count();
            require(cards.size()==known,"other_world_known_inventory",m("seat",other,"cards",cards.size(),"known",known));
            for(ZoneType zone:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command))zones.put(zone.name().toLowerCase(Locale.ROOT),sequence(player.getCardsIn(zone)));
            List<Card> stack=new ArrayList<>();for(Card card:game.getStackZone().getCards())if(card.getOwner()==player)stack.add(card);zones.put("stack",sequence(stack));
            return m("seat",other,"player_id",player.getId(),"materialized_aliases",cards.size(),"life",String.valueOf(player.getLife()),"mana",pool(player),"lands_played",player.getLandsPlayedThisTurn(),"cards",cards,"actual_zone_order",zones);
        }
        Map<String,Object> ability(SpellAbility a){return m("ability_id",a.getId(),"source",alias(a.getHostCard()),"source_timestamp",String.valueOf(a.getHostCard().getGameTimestamp()),"spell",a.isSpell(),"mana_ability",a.isManaAbility(),"land_ability",a.isLandAbility(),"targets_cards",sequence(a.getTargets().getTargetCards()),"target_players",java.util.stream.StreamSupport.stream(a.getTargets().getTargetPlayers().spliterator(),false).map(this::seat).toList());}
        Map<String,Object> view(CardView c){if(c==null)return null;String a=aliases.get(c.getId());require(a!=null,"unknown_card_view",m("id",c.getId()));return m("physical_id",String.valueOf(c.getId()),"alias",a,"owner",seatView(c.getOwner()),"controller",seatView(c.getController()),"zone",c.getZone()==null?null:c.getZone().name().toLowerCase(Locale.ROOT));}
        Map<String,Object> view(SpellAbilityView a){return a==null?null:m("ability_id",a.getId(),"host",view(a.getHostCard()),"is_spell",a.isSpell(),"description",a.getDescription());}
        Map<String,Object> view(StackItemView si){return si==null?null:m("stack_id",si.getId(),"source",view(si.getSourceCard()),"activating_player",seatView(si.getActivatingPlayer()),"target_cards",si.getTargetCards()==null?null:si.getTargetCards().stream().map(this::view).toList(),"target_players",si.getTargetPlayers()==null?null:si.getTargetPlayers().stream().map(this::seatView).toList(),"target_stack_ids",si.getTargetStackItems()==null?null:si.getTargetStackItems().stream().map(StackItemView::getId).toList());}
        Map<String,Object> zone(ZoneView z){return z==null?null:m("player",seatView(z.player()),"zone_type",z.zoneType().name().toLowerCase(Locale.ROOT));}
        void observe(String key,long source,boolean full,Object evidence){
            require(observed.add(key),"duplicate_selector",m("key",key));String checkpoint=full?"observation-"+source:null;
            if(full&&!checkpoints.containsKey(checkpoint))checkpoints.put(checkpoint,world());
            observations.add(m("key",key,"observation_id","observation-"+source,"source_global_sequence",source,"scope",full?"full_checkpoint":"candidate_evidence","reference_only",key.equals("setup.predraw")||key.equals("terminal.return"),"linked_observation_key",key.equals("terminal.return")?"terminal":null,"checkpoint_ref",checkpoint,"evidence",evidence));
        }
        void once(String key,long source,boolean full,Object evidence){if(!observed.contains(key))observe(key,source,full,evidence);}

        JsonObject atDecision(Player p,String kind){
            require(decisionCursor<decisions.size(),"extra_decision",m("kind",kind));JsonObject d=decisions.get(decisionCursor);
            require(str(d,"kind").equals(kind)&&str(d,"seat").equals(seat(p))&&number(d,"individual_turn")==turn()&&str(d,"step").equals(step()),"decision_context",m("actual_kind",kind,"actor",seat(p),"next_policy",d,"context",header()));return d;
        }
        JsonObject opening(Player p,String family){
            for(int i=0;i<opening.size();i++)if(!openingUsed.contains(i)&&str(opening.get(i),"chooser").equals(seat(p))&&str(opening.get(i),"reply").equals("bottom")==family.equals("bottom")){openingUsed.add(i);return opening.get(i);}
            throw bad("opening_context",m("actor",seat(p),"family",family));
        }
        boolean keep(Player p,Player first,int charge){Call c=enter(p,"mulliganKeepHand",m("first",seat(first),"charged",charge,"hand",sequence(p.getCardsIn(ZoneType.Hand))));require(first==seats.get("A")&&game.getAge()==GameStage.Mulligan,"keep_context",m());JsonObject d=opening(p,"declaration");boolean keep=str(d,"reply").equals("keep");require(keep||str(d,"reply").equals("mulligan"),"keep_reply",m());leave(c,m("keep",keep,"input_policy",d));return keep;}
        CardCollectionView bottom(Player p,CardCollectionView hand,int count){Call c=enter(p,"tuckCardsViaMulligan",m("domain",sequence(hand),"required",count));JsonObject d=opening(p,"bottom");List<String> wanted=strings(d.getAsJsonArray("aliases"));require(count==wanted.size()&&hand.size()==7&&seat(p).equals("A")&&game.getAge()==GameStage.Mulligan,"bottom_context",m());observe("mulligan.bottom.before",c.sequence,true,m("policy",d));CardCollection out=new CardCollection();for(String a:wanted){require(hand.contains(card(a)),"bottom_domain",m("alias",a));out.add(card(a));}leave(c,m("selected",sequence(out),"input_policy",d));return out;}
        List<SpellAbility> choose(Player p){
            Call c=enter(p,"chooseSpellAbilityToPlay",m("offered_domain","actual_priority_context"));
            require(p==game.getPhaseHandler().getPriorityPlayer()&&game.getAge()==GameStage.Play&&turn()<=number(fixture.getAsJsonObject("watchdog"),"maximum_individual_turn"),"priority_context",m());priorityCount++;
            require(strings(fixture.getAsJsonObject("pass_policy").getAsJsonArray("allowed_steps")).contains(step()),"priority_step",m());
            if(turn()==1&&step().equals("upkeep")&&p==seats.get("A"))once("opening.complete",c.sequence,true,m());
            if(discardPending!=null&&step().equals("upkeep")){
                String who=str(discardPending,"seat");int n=number(discardPending,"personal_turn");String key="cleanup."+who+"."+n;
                once(key,c.sequence,n==1||id.equals("G03")&&(who.equals("B")&&n==53||who.equals("A")&&n==54),m("policy",discardPending));
                if(turn()==3)once("rotation.first",c.sequence,true,m());
                if(id.equals("G03")&&who.equals("B")&&n==53)once("cleanup.tail.B",c.sequence,true,m());
                if(id.equals("G03")&&who.equals("A")&&n==54)once("cleanup.tail.A",c.sequence,true,m());discardPending=null;
            }
            if(resolving!=null&&game.getStack().isEmpty()&&card(str(resolving,"alias")).isInPlay()){
                once("resolve."+str(resolving,"seat")+"."+number(resolving,"personal_turn"),c.sequence,true,m("action",resolving));resolving=null;
            }
            if(orderCount==2&&step().equals("combat_damage")&&game.getStack().isEmpty())once("trade.settled",c.sequence,true,m());
            if(playerDamageTurns.contains(turn())&&step().equals("combat_damage"))once("damage."+personal(),c.sequence,true,m());
            if(id.equals("G03")&&step().equals("draw")&&p==game.getPhaseHandler().getPlayerTurn()&&p.getCardsIn(ZoneType.Library).isEmpty())once("draw.last."+seat(p),c.sequence,true,m());
            if(id.equals("G03")&&turn()==108&&step().equals("upkeep"))once(p==seats.get("B")?"failed-draw.before":"failed-draw.after-first-pass",c.sequence,true,m());
            if(actionCursor<actions.size()){
                JsonObject a=actions.get(actionCursor);int at=number(a,"individual_turn");
                require(turn()<=at,"missed_action_turn",m("action",a));
                if(turn()==at&&seat(p).equals(str(a,"seat"))&&step().equals(str(a,"step"))&&game.getStack().isEmpty()){
                    selectedAction=a;Card source=card(str(a,"alias"));String kind=str(a,"kind");
                    if(kind.equals("play_land")){require(source.isInZone(ZoneType.Hand),"land_source",m());selected=new LandAbility(source,source.getCurrentState());}
                    else if(kind.equals("activate_mana")){
                        List<SpellAbility> opts=new ArrayList<>();for(SpellAbility sa:source.getManaAbilities())if(sa.isManaAbility()&&sa.getManaPart().getOrigProduced().equals("W"))opts.add(sa);
                        require(opts.size()==1,"fixed_mana_domain",m("count",opts.size()));selected=opts.get(0);
                    }else if(kind.equals("cast_untargeted")){require(source.isInZone(ZoneType.Hand)&&source.getName().equals("Silvercoat Lion"),"spell_source",m());selected=source.getFirstSpellAbility();}
                    else throw bad("action_kind",m("kind",kind));
                    selected.setActivatingPlayer(p);require(selected.canPlay()&&!selected.usesTargeting(),"chosen_action_legality",m("action",a));
                    leave(c,m("kind","chosen","policy",a,"ability",ability(selected)));return List.of(selected);
                }
                if(turn()==at&&p==game.getPhaseHandler().getPlayerTurn()&&game.getStack().isEmpty())require(phaseRank(step())<=phaseRank(str(a,"step")),"missed_action_step",m("action",a));
            }
            for(SpellAbilityStackInstance si:game.getStack())require(si.getSpellAbility().isSpell()&&si.getSourceCard().getName().equals("Silvercoat Lion")&&!si.getSpellAbility().usesTargeting(),"unplanned_stack",m());
            leave(c,m("kind","pass"));return null;
        }
        int phaseRank(String s){return List.of("untap","upkeep","draw","precombat_main","beginning_combat","declare_attackers","declare_blockers","first_strike_damage","combat_damage","end_combat","postcombat_main","end","cleanup").indexOf(s);}
        boolean play(Player p,SpellAbility sa){
            Call c=enter(p,"playChosenSpellAbility",m("ability",ability(sa),"policy",selectedAction));require(sa==selected&&selectedAction==actions.get(actionCursor),"selected_identity",m());
            ordinaryOperation=id+".ordinary."+(++ordinaryIdentity);JsonObject a=selectedAction;String kind=str(a,"kind");int stackBefore=game.getStack().size(),landsBefore=p.getLandsPlayedThisTurn();
            add("operations","ordinary_entry",m("operation_id",ordinaryOperation,"policy",a,"ability",ability(sa),"context",header()));
            boolean result=PlaySpellAbility.playSpellAbility(p.getController(),p,sa);require(result,"play_false",m("action",a));
            Card source=card(str(a,"alias"));
            if(kind.equals("play_land"))require(source.isInPlay()&&!source.isTapped()&&p.getLandsPlayedThisTurn()==landsBefore+1&&game.getStack().size()==stackBefore,"land_admission",m());
            else if(kind.equals("activate_mana")){
                require(source.isTapped()&&game.getStack().size()==stackBefore,"mana_admission",m());Collection<Mana> actual=sa.getManaPart().getLastManaProduced();require(actual.size()==1,"mana_produced_domain",m());Mana mana=actual.iterator().next();
                require(mana.getSourceCard().getId()==source.getId()&&mana.getColor()==ManaAtom.WHITE&&mana.getPlayer()==p&&!mana.isSnow(),"mana_source",m());produced.put(str(a,"alias"),new Produced(mana,str(a,"alias"),ordinaryOperation,source.getGameTimestamp()));
            }else {require(source.isInZone(ZoneType.Stack)&&game.getStack().size()==stackBefore+1&&p.getManaPool().isEmpty(),"cast_admission",m());resolving=a;}
            actionCursor++;long seq=leave(c,m("result",result,"policy",a,"actual_ability",ability(sa)));add("operations","ordinary_return",m("operation_id",ordinaryOperation,"callback_return_sequence",seq,"policy",a,"context",header()));
            String suffix=str(a,"seat")+"."+number(a,"personal_turn");
            if(kind.equals("play_land"))observe("land."+suffix,seq,true,m());else if(kind.equals("activate_mana"))observe("mana."+suffix+"."+(str(a,"alias").endsWith(".P1")?"1":"2"),seq,true,m());else observe("cast."+suffix,seq,true,m());
            selected=null;selectedAction=null;ordinaryOperation=null;return result;
        }
        SpellAbility select(Player p,Card host,List<SpellAbility> options,ITriggerEvent event){Call c=enter(p,"getAbilityToPlay",m("host",alias(host),"domain",options.stream().map(this::ability).toList(),"trigger_event",event==null?null:event.getClass().getName()));callerPath(c,"forge.game.player.PlayerController.getAbilityToPlay","forge.game.player.PlaySpellAbility.chooseOptionalAdditionalCosts");require(event==null&&options.size()==1&&options.get(0)==selected&&host.getId()==selected.getHostCard().getId(),"ability_selection_domain",m());leave(c,ability(selected));return selected;}
        CostDecisionMakerBase costs(Player p,Player payer,SpellAbility sa,boolean effect,String prompt){Call c=enter(p,"getCostDecisionMaker",m("payer",seat(payer),"ability",ability(sa),"effect",effect,"prompt",prompt,"original_cost_parts",diagnostic(sa.getPayCosts().getCostParts(),0)));callerPath(c,"forge.game.player.PlayerController.getCostDecisionMaker","forge.game.player.PlaySpellAbility.playAbility");require(p==payer&&sa==selected&&!effect&&sa.getPayCosts().getCostParts().size()==1,"cost_context",m());StrictCosts out=new StrictCosts(p,sa,this);leave(c,m("visitor",objectRef(out),"payer",seat(payer),"effect",false,"ability",ability(sa),"source",alias(sa.getHostCard())));return out;}
        Map<String,Object> costScope(CostPart part,SpellAbility sa){
            CostPaymentStack.Entry entry=game.costPaymentStack.peek();List<CostPart> original=entry==null?List.of():entry.payment().getCost().getCostParts();String origin="unrecognized";
            if(original.size()==1){CostPart first=original.get(0);if(first instanceof CostTap&&part instanceof CostTap)origin="original";else if(first instanceof CostTap&&part instanceof CostPartMana mana&&mana.getManaCostFor(sa).isZero())origin="implicit_zero_mana";else if(first instanceof CostPartMana&&part instanceof CostPartMana)origin="original";}
            return m("part_origin",origin,"actual_part",objectRef(part),"payment",entry==null?null:objectRef(entry.payment()),"stack_part_is_actual_part",entry!=null&&entry.cost()==part,"payment_ability_is_selected",entry!=null&&entry.payment().getAbility()==sa&&sa==selected,"original_cost_parts",diagnostic(original,0),"original_part_identity_match",original.contains(part));
        }
        void validateCostScope(CostPart part,SpellAbility sa,String origin){CostPaymentStack.Entry entry=game.costPaymentStack.peek();Map<String,Object> scope=costScope(part,sa);require(entry!=null&&entry.cost()==part&&entry.payment().getAbility()==sa&&sa==selected&&origin.equals(scope.get("part_origin")),"cost_part_scope",scope);}
        boolean pay(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Call c=enter(p,"payManaCost",m("cost",cost.toString(),"ability",ability(sa),"payer",seat(p),"effect",effect,"prompt",prompt,"part",part.getClass().getName(),"scope",costScope(part,sa),"matrix",objectRef(matrix),"matrix_is_current_payment",game.costPaymentStack.peek()!=null&&matrix==game.costPaymentStack.peek().payment()));callerPath(c,"forge.game.player.PlayerController.payManaCost","forge.game.cost.CostPartMana.payAsDecided");validateCostScope(part,sa,sa.isManaAbility()?"implicit_zero_mana":"original");require(sa==selected&&!effect&&matrix==game.costPaymentStack.peek().payment()&&(sa.isManaAbility()?cost.isZero():cost.toString().equals("{1}{W}")),"mana_cost_context",m());boolean result=PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,false);require(result,"mana_cost_failed",m());leave(c,m("result",result,"pool",pool(p),"scope",costScope(part,sa)));return result;}
        Map<String,Object> token(Produced t){return m("production_operation_id",t.operation,"output_index",0,"source",t.source,"source_timestamp",String.valueOf(t.timestamp),"color","white");}
        boolean apply(Player p,ManaCostBeingPaid cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Call c=enter(p,"applyManaToCost",m("ability",ability(sa),"payer",seat(p),"effect",effect,"prompt",prompt,"unpaid",cost.toString(),"pool",pool(p),"matrix",objectRef(matrix)));callerPath(c,"forge.game.player.PlaySpellAbility.payManaCost");require(sa==selected&&sa.isSpell()&&!effect&&game.costPaymentStack.peek()!=null&&matrix==game.costPaymentStack.peek().payment()&&callStack.size()>1&&callStack.stream().skip(1).findFirst().orElseThrow().method.equals("payManaCost")&&cost.toManaCost().toString().equals("{1}{W}")&&p.getManaPool().totalMana()==2&&p.getManaPool().getAmountOfColor((byte)ManaAtom.WHITE)==2,"apply_context",m());
            Produced one=produced.get(seat(p)+".P1"),two=produced.get(seat(p)+".P2");require(one!=null&&two!=null&&one.mana!=two.mana,"named_tokens",m());
            List<Object> spent=new ArrayList<>();p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);
            try{for(Produced t:List.of(one,two)){require(t.mana.meetsManaRestrictions(sa)&&sa.allowsPayingWithShard(t.mana.getSourceCard(),t.mana.getColor()),"token_restriction",m());String before=cost.toString();require(p.getManaPool().tryPayCostWithMana(sa,cost,t.mana,false),"named_token_debit",token(t));sa.getPayingMana().add(t.mana);spent.add(m("token",token(t),"unpaid_before",before,"unpaid_after",cost.toString(),"pool_after",pool(p)));}require(cost.isPaid()&&p.getManaPool().isEmpty()&&sa.getPayingMana().size()==2,"paid_completion",m());}
            finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}
            leave(c,m("result",true,"spent",spent,"paid",cost.isPaid(),"paying_mana",sa.getPayingMana().stream().map(x->x==one.mana?token(one):token(two)).toList()));return true;
        }

        Object defenderEntity(GameEntity target){
            if(target instanceof Player player)return m("kind","player","seat",seat(player),"player_id",player.getId());
            if(target instanceof Card card)return m("kind","card","card",view(card.getView()));
            throw bad("attack_defender_entity",m("class",target.getClass().getName()));
        }
        void attackers(Player p,Player active,Combat combat){
            List<GameEntity> offeredDefenders=new ArrayList<>();for(GameEntity target:combat.getDefenders())offeredDefenders.add(target);
            Call c=enter(p,"declareAttackers",m("active",seat(active),"eligible",domain(CombatUtil.getPossibleAttackers(active)),"defender_domain",offeredDefenders.stream().map(this::defenderEntity).toList(),"actual_combat_is_current",combat==game.getCombat()));
            Player opponent=seats.get(seat(p).equals("A")?"B":"A");
            require(p==active&&combat==game.getCombat()&&combat.getAttackingPlayer()==active,"attack_actor_combat",m());
            require(offeredDefenders.size()==1&&offeredDefenders.get(0)==opponent,"attack_defender_domain",m("actual",offeredDefenders.stream().map(this::defenderEntity).toList()));
            JsonObject d=atDecision(p,"declare_attackers");
            if(id.startsWith("G02")&&turn()==5)observe("young.live-domain",c.sequence,true,m("eligible",domain(CombatUtil.getPossibleAttackers(active)),"young_can_attack",CombatUtil.canAttack(card("A.L2"),seats.get("B"))));
            List<String> chosen=strings(d.getAsJsonArray("aliases")),defenders=strings(d.getAsJsonArray("defenders"));require(chosen.size()==defenders.size(),"attack_shape",m());
            for(int i=0;i<chosen.size();i++){Card a=card(chosen.get(i));Player defender=seats.get(defenders.get(i));require(CombatUtil.canAttack(a,defender),"attack_legality",m("source",chosen.get(i)));combat.addAttacker(a,defender);}
            require(CombatUtil.validateAttackers(combat),"attack_validation",m());decisionCursor++;leave(c,m("input_policy",d,"selected",chosen,"defenders",defenders));
        }
        void blockers(Player p,Player defender,Combat combat){Call c=enter(p,"declareBlockers",m("defender",seat(defender),"attackers",domain(combat.getAttackers()),"available_creatures",domain(p.getCreaturesInPlay())));JsonObject d=atDecision(p,"declare_blockers");observe("trade.before",c.sequence,true,m());List<String> chosen=strings(d.getAsJsonArray("aliases")),targets=strings(d.getAsJsonArray("attackers"));require(chosen.size()==targets.size()&&p==defender,"block_shape",m());for(int i=0;i<chosen.size();i++){require(CombatUtil.canBlock(card(targets.get(i)),card(chosen.get(i)),combat),"block_legality",m());combat.addBlocker(card(targets.get(i)),card(chosen.get(i)));}require(CombatUtil.validateBlocks(combat,p)==null,"block_validation",m());decisionCursor++;leave(c,m("input_policy",d,"selected",chosen,"attackers",targets));}
        Map<Card,Integer> assign(Player p,Card source,CardCollectionView recipients,CardCollectionView remaining,int amount,GameEntity defender,boolean override){Call c=enter(p,"assignCombatDamage",m("source",alias(source),"recipients",sequence(recipients),"remaining",remaining==null?null:sequence(remaining),"amount",String.valueOf(amount),"defender",defender instanceof Player pl?seat(pl):null,"override_order",override));require(id.startsWith("G02")&&turn()==9&&step().equals("combat_damage")&&source.getController()==p&&recipients.size()==1&&amount==source.getNetPower()&&amount==2&&assigned.add(alias(source)),"assignment_context",m());String other=alias(source).startsWith("A.")?"B.":"A.";String expected=other+alias(source).substring(2);Card target=card(expected);require(recipients.contains(target),"assignment_recipient",m());Map<Card,Integer> result=new LinkedHashMap<>();result.put(target,amount);leave(c,m("assignments",List.of(m("recipient",expected,"amount",String.valueOf(amount)))));return result;}
        CardCollectionView order(Player p,CardCollectionView cards,ZoneType dest,SpellAbility cause){Call c=enter(p,"orderMoveToZoneList",m("offered_sequence",sequence(cards),"destination",dest.name(),"cause",cause==null?null:ability(cause)));JsonObject d=atDecision(p,"order_graveyard");require(dest==ZoneType.Graveyard&&cause==null&&cards.size()==2&&assigned.size()==4&&seat(p).equals(orderCount==0?"A":"B"),"graveyard_order_context",m());for(String a:List.of("A.L1","A.L2","B.L1","B.L2"))require(card(a).isInPlay()&&card(a).getDamage()==2,"held_simultaneous_deaths",m("alias",a));List<String> desired=strings(d.getAsJsonArray("aliases"));require(new HashSet<>(desired).equals(new HashSet<>(sequence(cards))),"graveyard_domain",m());observe("trade.owner."+seat(p),c.sequence,true,m("input_policy",d));CardCollection result=new CardCollection();for(String a:strings(d.getAsJsonArray("forge_callback_return_aliases")))result.add(card(a));orderCount++;decisionCursor++;leave(c,m("input_policy",d,"selected_top_to_bottom",desired,"returned_sequence",sequence(result)));return result;}
        CardCollectionView discard(Player p,int count){Call c=enter(p,"chooseCardsToDiscardToMaximumHandSize",m("required",count,"domain",sequence(p.getCardsIn(ZoneType.Hand))));JsonObject d=atDecision(p,"discard");List<String> chosen=strings(d.getAsJsonArray("aliases"));require(count==chosen.size()&&count==p.getCardsIn(ZoneType.Hand).size()-7&&game.getStack().isEmpty(),"discard_context",m());if(turn()==2)observe("cleanup.first.before",c.sequence,true,m());CardCollection result=new CardCollection();for(int i=chosen.size()-1;i>=0;i--){Card source=card(chosen.get(i));require(source.isInZone(ZoneType.Hand)&&source.getOwner()==p,"discard_domain",m());result.add(source);}discardPending=d;decisionCursor++;leave(c,m("input_policy",d,"selected_top_to_bottom",chosen,"returned_sequence",sequence(result)));return result;}
        void housekeeping(Player p,String method){
            Call c=enter(p,method,m());String branch,occurrence;List<Player> domain;
            if(method.equals("awaitNextInput")){
                callerPath(c,"forge.game.GameAction.determineFirstTurnPlayer","forge.game.GameAction.startGame","forge.game.Match.startGame");
                require(game.getAge()==GameStage.BeforeMulligan&&entropy.currentRole().equals("starting_chooser")&&entropy.state.equals("waiting_chooser")&&p==seats.get("B"),"await_context",m());branch="starting_chooser_notification";occurrence=branch;domain=List.of(seats.get("B"));
            }else if(method.equals("resetAtEndOfTurn")){
                callerPath(c,"forge.game.phase.PhaseHandler.onPhaseBegin","forge.game.phase.PhaseHandler.mainLoopStep","forge.game.phase.PhaseHandler.mainGameLoop");
                require(game.getAge()==GameStage.Play&&step().equals("end")&&p==game.getPhaseHandler().getPlayerTurn(),"reset_context",m());branch="end_of_turn_entry";occurrence=branch+":"+turn();domain=List.of(game.getPhaseHandler().getPlayerTurn());
            }else if(method.equals("autoPassCancel")&&game.getAge()==GameStage.Play){
                callerPath(c,"forge.game.phase.PhaseHandler.onPhaseBegin","forge.game.phase.PhaseHandler.mainLoopStep","forge.game.phase.PhaseHandler.mainGameLoop");
                require(!game.isGameOver()&&step().equals("cleanup"),"autopass_cleanup_context",m());branch="cleanup_current_seats";occurrence=branch+":"+turn();domain=new ArrayList<>(game.getPlayers());
            }else if(method.equals("autoPassCancel")&&game.getAge()==GameStage.GameOver){
                callerPath(c,"forge.game.GameAction.startGame","forge.game.Match.startGame");
                require(game.isGameOver()&&entropy.cursor==entropy.queue.size(),"autopass_terminal_context",m());branch="postloop_registered_seats";occurrence=branch;domain=new ArrayList<>(game.getRegisteredPlayers());
            }else throw bad("housekeeping_context",m("method",method));
            int index=housekeepingCounts.getOrDefault(occurrence,0);
            require(turn()<=number(fixture.getAsJsonObject("watchdog"),"maximum_individual_turn")&&index<domain.size()&&domain.get(index)==p&&housekeepingSeen.add(occurrence+":"+seat(p)),"housekeeping_occurrence",m("branch",branch,"occurrence",occurrence,"index",index,"domain",domain.stream().map(this::seat).toList()));
            housekeepingCounts.put(occurrence,index+1);leave(c,m("kind","void_ack","source_branch",branch,"occurrence",occurrence,"source_loop_index",index,"actual_domain",domain.stream().map(this::seat).toList()));
        }
        Map<DeckSection,List<? extends PaperCard>> complain(Player p,Deck deck){Call c=enter(p,"complainCardsCantPlayWell",m("main_count",deck.getMain().countAll(),"deck_is_registered",deck==p.getRegisteredPlayer().getDeck(),"deck",diagnosticDeck(deck)));callerPath(c,"forge.game.Match.prepareAllZones","forge.game.Match.startGame");require(game.getAge()==GameStage.BeforeMulligan&&deck==p.getRegisteredPlayer().getDeck()&&deck.getMain().countAll()==60&&entropy.state.equals("waiting_raw")&&entropy.cursor==(seat(p).equals("A")?1:2)&&complained.add(seat(p)),"complain_context",m());leave(c,m("empty_map",true,"preceding_operation_id",str(entropy.queue.get(entropy.cursor-1),"operation_id")));return Collections.emptyMap();}
        Object diagnosticDeck(Deck deck){List<Object> rows=new ArrayList<>();for(var row:deck.getMain())rows.add(m("printing",diagnostic(row.getKey(),0),"count",row.getValue()));rows.sort(Comparator.comparing(JSON::toJson));return m("name",deck.getName(),"main_count",deck.getMain().countAll(),"main_printings",rows);}
        void unsupported(Player p,Map<Player,List<PaperCard>> values){Map<String,Object> domain=new LinkedHashMap<>();for(var e:values.entrySet())domain.put(seat(e.getKey()),e.getValue().stream().map(PaperCard::getName).toList());Call c=enter(p,"revealUnsupported",m("domain",domain));require(values.size()==2&&values.keySet().containsAll(seats.values())&&values.values().stream().allMatch(List::isEmpty)&&aliases.size()==120&&unsupported.add(seat(p)),"unsupported_context",m());leave(c,m("kind","void_ack"));}
        Player starting(Player p,boolean first){Call c=enter(p,"chooseStartingPlayer",m("is_first_game",first));require(first&&p==seats.get("A")&&aliases.size()==120,"starting_context",m());entropy.chooser(p);long seq=leave(c,m("selected","A"));observe("setup.predraw",seq,true,m("reference_only",true));return seats.get("A");}
        void event(String tag,Map<String,Object> payload){
            healthy();long seq=add("events",tag,m("payload",payload,"context",header(),"parent_callback_id",callStack.isEmpty()?null:callStack.peek().id,"operation_id",ordinaryOperation));
            if(tag.equals("GameEventShuffle"))entropy.committed((String)payload.get("player"),seq);
            if(tag.equals("GameEventMulligan"))observe("mulligan.bottom.after",seq,true,m());
            if(tag.equals("GameEventAttackersDeclared")){
                if(id.equals("G01")&&turn()==3)observe("young.forced-empty",seq,true,payload);
                @SuppressWarnings("unchecked") List<Object> links=(List<Object>)payload.get("attackersMap");if(!links.isEmpty())observe("attack."+seat(game.getPhaseHandler().getPlayerTurn())+"."+personal(),seq,false,payload);
            }
            if(tag.equals("GameEventPlayerDamaged"))playerDamageTurns.add(turn());
            if(tag.equals("GameEventPlayerControl"))require(stackTags().stream().anyMatch(x->x.equals("forge.game.Game.setGameOver"))&&stackTags().stream().anyMatch(x->x.equals("forge.game.player.Player.clearController"))&&payload.get("newLobbyPlayerName")==null&&Boolean.TRUE.equals(payload.get("newControllerIsHuman")),"player_control_branch",payload);
            if(tag.equals("GameEventGameOutcome")){if(playerDamageTurns.contains(turn()))once("damage."+personal(),seq,true,m());observe("terminal",seq,true,payload);}
            if(tag.equals("GameEventGameFinished")){require(game.isGameOver(),"finished_without_outcome",m());finishedEvent=true;}
        }
        List<Map<String,Object>> stackFrames(){return StackWalker.getInstance().walk(frames->frames.map(f->m("class",f.getClassName(),"method",f.getMethodName(),"descriptor",f.getDescriptor())).toList());}
        List<String> tagsFromFrames(List<Map<String,Object>> frames){return frames.stream().map(f->f.get("class")+"."+f.get("method")).toList();}
        List<String> stackTags(){return StackWalker.getInstance().walk(frames->frames.map(f->f.getClassName()+"."+f.getMethodName()).toList());}
        void finish(){healthy();require(game.isGameOver()&&naturalReturn&&finishedEvent&&callStack.isEmpty()&&actionCursor==actions.size()&&decisionCursor==decisions.size()&&openingUsed.size()==opening.size()&&entropy.cursor==entropy.queue.size(),"incomplete_run",m("actions",actionCursor,"decisions",decisionCursor,"opening",openingUsed.size(),"random",entropy.cursor));require(housekeepingCounts.getOrDefault("starting_chooser_notification",0)==1&&housekeepingCounts.getOrDefault("postloop_registered_seats",0)==2&&housekeepingCounts.entrySet().stream().allMatch(e->!e.getKey().startsWith("cleanup_current_seats:")||e.getValue()==2),"incomplete_housekeeping",m("counts",housekeepingCounts));require(aliases.size()==120,"terminal_inventory",m());world();}
        Map<String,Object> output(){return m("schema","ordinary-game-forge-v1","schema_version",1,"case_id",id,"reference","forge","forge_commit",PIN,"inputs_sha256",INPUT_SHA,"fixture",fixture,"paper_cards",paperCards,"actual_outcome",outcome(),"physical_aliases",physical,"registered_players",registeredPlayers(),"callbacks",streams.get("callbacks"),"events",streams.get("events"),"random",streams.get("random"),"operations",streams.get("operations"),"failures",failures,"observations",observations,"checkpoints",checkpoints,"runtime",m("loader_classes",loaderClasses,"java_home",System.getProperty("java.home"),"java_version",System.getProperty("java.runtime.version"),"java_vendor",System.getProperty("java.vendor"),"java_class_path",System.getProperty("java.class.path"),"surefire_test_class_path",System.getProperty("surefire.test.class.path")),"completion",m("natural_start_game_return",naturalReturn,"game_over",game!=null&&game.isGameOver(),"game_finished_event",finishedEvent,"actions_consumed",actionCursor,"actions_planned",actions.size(),"decisions_consumed",decisionCursor,"decisions_planned",decisions.size(),"opening_consumed",openingUsed.size(),"opening_planned",opening.size(),"random_operations_consumed",entropy.cursor,"random_operations_planned",entropy.queue.size(),"sticky_failure_count",failures.size(),"callback_depth",callStack.size(),"final_world_ref",observed.contains("terminal.return")?observations.get(observations.size()-1).get("checkpoint_ref"):null));}
    }

    static final class RecordedEntropy extends Random {
        private static final long serialVersionUID=1L;
        final Script s;final List<JsonObject> queue;final Map<String,JsonObject> plans=new LinkedHashMap<>();final Thread thread=Thread.currentThread();
        long installedCallCount;int cursor,sample;boolean insideDelegate;String state="waiting_raw";List<String> before,target;List<Integer> indices,bounds;List<Long> words;Map<String,Object> otherBefore;
        RecordedEntropy(Script s){super(0);this.s=s;queue=objects(s.fixture.getAsJsonArray("forge_operation_queue"));for(JsonObject x:objects(s.fixture.getAsJsonArray("shuffle_operations")))plans.put(str(x,"operation_id"),x);}
        String currentRole(){return cursor<queue.size()?str(queue.get(cursor),"role"):"complete";}
        @Override public int nextInt(){
            if(insideDelegate)throw s.bad("random_reentrancy",m("actual_stack_tags",s.stackTags()));
            insideDelegate=true;
            try{return supply();}finally{insideDelegate=false;}
        }
        int supply(){
            s.healthy();s.require(Thread.currentThread()==thread&&cursor<queue.size(),"random_thread_or_excess",m());JsonObject op=queue.get(cursor);String role=str(op,"role");List<Map<String,Object>> frames=s.stackFrames();List<String> tags=s.tagsFromFrames(frames);validatePath(role,tags,frames);
            s.require(tags.contains("forge.util.MyRandom$CountingRandom.next")&&tags.contains("java.util.Random.nextInt"),"random_adapter_stack",m("actual_stack_tags",tags));
            if(role.equals("starting_chooser")){
                s.require(state.equals("waiting_raw")&&s.game.getAge()==GameStage.BeforeMulligan&&tags.contains("forge.util.Aggregates.random")&&tags.contains("forge.game.GameAction.determineFirstTurnPlayer")&&!tags.contains("forge.game.player.Player.shuffle"),"chooser_random_context",m("actual_stack_tags",tags));state="waiting_chooser";
                int value=number(op,"raw_signed_32");s.add("random","raw",m("operation_id",str(op,"operation_id"),"role",role,"planned_bound",number(op,"planned_bound"),"planned_index",number(op,"planned_index"),"raw_signed_32",value,"raw_unsigned_32",Integer.toUnsignedString(value),"actual_stack_tags",tags,"actual_stack_frames",frames,"actual_call_count",MyRandom.callCount(),"actual_call_count_delta",MyRandom.callCount()-installedCallCount));return value;
            }
            s.require(tags.contains("java.util.Collections.shuffle")&&tags.contains("forge.game.player.Player.shuffle")&&(role.equals("initial_shuffle")?tags.contains("forge.game.Match.prepareAllZones")&&s.game.getAge()==GameStage.BeforeMulligan:tags.contains("forge.game.mulligan.AbstractMulligan.mulligan")&&s.game.getAge()==GameStage.Mulligan),"shuffle_random_stack",m("actual_stack_tags",tags));
            if(state.equals("waiting_raw"))begin(op);
            s.require(state.equals("supplying")&&sample<words.size(),"random_tape_excess",m("operation",op,"state",state));long word=words.get(sample);int selected=indices.get(sample),bound=bounds.get(sample);
            s.add("random","raw",m("operation_id",str(op,"operation_id"),"role",role,"planned_owner",str(op,"planned_owner"),"sample_index",sample,"planned_bound",bound,"planned_index",selected,"raw_signed_32",(int)word,"raw_unsigned_32",Long.toUnsignedString(word),"actual_stack_tags",tags,"actual_stack_frames",frames,"actual_call_count",MyRandom.callCount(),"actual_call_count_delta",MyRandom.callCount()-installedCallCount));sample++;if(sample==words.size())state="waiting_callback";return (int)word;
        }
        void validatePath(String role,List<String> tags,List<Map<String,Object>> frames){
            String delegate="forge.ai.OrdinaryGameReferenceTest$RecordedEntropy.nextInt";
            int start=tags.indexOf(delegate);
            List<String> expected=new ArrayList<>(List.of("forge.util.MyRandom$CountingRandom.next","java.util.Random.nextInt"));
            if(role.equals("starting_chooser"))expected.addAll(List.of("forge.util.Aggregates.random","forge.game.GameAction.determineFirstTurnPlayer","forge.game.GameAction.startGame","forge.game.Match.startGame"));
            else {
                s.require(role.equals("initial_shuffle")||role.equals("mulligan_shuffle"),"random_role",m("role",role));
                expected.addAll(List.of("java.util.Collections.shuffle","forge.game.player.Player.shuffle"));
                if(role.equals("initial_shuffle"))expected.addAll(List.of("forge.game.Match.prepareAllZones","forge.game.Match.startGame"));
                else expected.addAll(List.of("forge.game.mulligan.AbstractMulligan.mulligan","forge.game.mulligan.MulliganService.runPlayerMulligans","forge.game.mulligan.MulliganService.perform","forge.game.GameAction.startGame","forge.game.Match.startGame"));
            }
            s.require(start>=0&&tags.lastIndexOf(delegate)==start&&tags.size()>=start+1+expected.size()&&tags.subList(start+1,start+1+expected.size()).equals(expected),"ordered_random_consumer_path",m("role",role,"required_path",expected,"actual_stack_tags",tags,"actual_stack_frames",frames));
            Map<String,String> descriptors=Map.of("forge.util.MyRandom$CountingRandom.next","(I)I","java.util.Random.nextInt","(I)I","java.util.Collections.shuffle","(Ljava/util/List;Ljava/util/Random;)V","forge.game.player.Player.shuffle","(Lforge/game/spellability/SpellAbility;)V");
            for(var frame:frames){String tag=frame.get("class")+"."+frame.get("method");if(descriptors.containsKey(tag))s.require(descriptors.get(tag).equals(frame.get("descriptor")),"random_method_descriptor",m("actual_frame",frame,"required_descriptor",descriptors.get(tag)));}
        }
        void begin(JsonObject op){Player p=s.seats.get(str(op,"planned_owner"));List<Card> library=new ArrayList<>();for(Card c:p.getCardsIn(ZoneType.Library))library.add(c);s.require(library.size()==60,"random_inventory",m("count",library.size()));
            if(str(op,"role").equals("initial_shuffle")){
                for(String definition:s.papers.keySet()){
                    List<Card> same=library.stream().filter(c->c.getPaperCard().getName().equals(s.papers.get(definition).getName())).sorted(Comparator.comparingInt(Card::getId)).toList();
                    List<JsonObject> aliases=s.inventory.values().stream().filter(x->str(x,"owner").equals(s.seat(p))&&str(x,"definition_ref").equals(definition)).toList();s.require(same.size()==aliases.size(),"materialized_definition_count",m("definition",definition));
                    for(int i=0;i<same.size();i++){Card c=same.get(i);String a=str(aliases.get(i),"alias");s.require(c.getPaperCard().getEdition().equals(s.papers.get(definition).getEdition())&&c.getPaperCard().getCollectorNumber().equals(s.papers.get(definition).getCollectorNumber())&&!s.physical.containsKey(a)&&!s.aliases.containsKey(c.getId()),"alias_binding",m("alias",a));s.aliases.put(c.getId(),a);s.physical.put(a,c.getId());}
                }
            }
            before=s.sequence(library);target=strings(plans.get(str(op,"target_ref")).getAsJsonArray("target_aliases"));s.require(new HashSet<>(before).size()==60&&new HashSet<>(before).equals(new HashSet<>(target)),"shuffle_target_inventory",m());List<String> shadow=new ArrayList<>(before);indices=new ArrayList<>();bounds=new ArrayList<>();words=new ArrayList<>();
            for(int i=shadow.size()-1;i>0;i--){int j=shadow.indexOf(target.get(i));s.require(j>=0&&j<=i,"shadow_prefix",m());int b=i+1;long raw=(b&(b-1))==0?((long)j<<(32-Integer.numberOfTrailingZeros(b))):2L*j;indices.add(j);bounds.add(b);words.add(raw);Collections.swap(shadow,i,j);}
            s.require(shadow.equals(target),"shadow_target",m());otherBefore=s.otherKnownWorld(str(op,"planned_owner"));sample=0;state="supplying";s.add("random","operation_begin",m("operation_id",str(op,"operation_id"),"role",str(op,"role"),"planned_owner",str(op,"planned_owner"),"observed_before",before,"target",target,"derived_indices",indices,"planned_bounds",bounds,"derived_raw_unsigned_32",words.stream().map(String::valueOf).toList(),"shadow_after",shadow,"materialized_aliases",s.aliases.size(),"other_known_world",otherBefore));
        }
        CardCollectionView shuffled(Player p,CardCollectionView list){
            Call c=s.enter(p,"cheatShuffle",m("operation_id",cursor<queue.size()?str(queue.get(cursor),"operation_id"):null,"received",s.sequence(list)));JsonObject op=queue.get(cursor);
            s.require(state.equals("waiting_callback")&&str(op,"planned_owner").equals(s.seat(p))&&s.sequence(list).equals(target)&&s.sequence(p.getCardsIn(ZoneType.Library)).equals(before),"shuffle_callback",m("state",state));
            Map<String,Object> other=s.otherKnownWorld(str(op,"planned_owner"));s.require(other.equals(otherBefore),"shuffle_callback_other_world",m("before",otherBefore,"actual",other));
            s.add("random","callback_verified",m("operation_id",str(op,"operation_id"),"callback_owner",s.seat(p),"actual_after",s.sequence(list),"unchanged_committed_before",s.sequence(p.getCardsIn(ZoneType.Library)),"callback_id",c.id,"other_known_world",other));state="waiting_commit";s.leave(c,m("returned_sequence",s.sequence(list),"identity_only",true));return list;
        }
        void committed(String owner,long event){
            JsonObject op=queue.get(cursor);s.require(state.equals("waiting_commit")&&str(op,"planned_owner").equals(owner)&&s.sequence(s.seats.get(owner).getCardsIn(ZoneType.Library)).equals(target),"shuffle_commit",m("state",state));
            Map<String,Object> other=s.otherKnownWorld(owner);s.require(other.equals(otherBefore),"shuffle_commit_other_world",m("before",otherBefore,"actual",other));
            s.add("random","operation_commit",m("operation_id",str(op,"operation_id"),"callback_owner",owner,"actual_committed",s.sequence(s.seats.get(owner).getCardsIn(ZoneType.Library)),"shuffle_event_sequence",event,"other_known_world",other));cursor++;state="waiting_raw";
        }
        void chooser(Player p){JsonObject op=queue.get(cursor);s.require(state.equals("waiting_chooser")&&s.seat(p).equals(str(op,"chosen_chooser")),"chooser_callback",m());s.add("random","chooser_complete",m("operation_id",str(op,"operation_id"),"actual_chooser",s.seat(p),"selected_starting_seat","A"));cursor++;state="waiting_raw";}
    }

    static final class StrictCosts extends CostDecisionMakerBase {
        final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script){super(player,false,ability,ability.getHostCard());this.script=script;}
        @Override public boolean paysRightAfterDecision(){return true;}
        @Override public PaymentDecision visit(CostTap cost) { boolean canPay=cost.canPay(ability,player,false);Call c=script.enter(player,"visit_CostTap",m("cost",cost.toString(),"ability",script.ability(ability),"payer",script.seat(player),"visitor",script.objectRef(this),"scope",script.costScope(cost,ability),"can_pay",canPay));script.validateCostScope(cost,ability,"original");script.require(ability==script.selected&&ability.isManaAbility()&&canPay&&!ability.getHostCard().isTapped(),"tap_cost_context",m());script.leave(c,m("payment_decision_number",0,"can_pay",canPay,"visitor",script.objectRef(this)));return PaymentDecision.number(0); }
        @Override public PaymentDecision visit(CostPartMana cost) { Call c=script.enter(player,"visit_CostPartMana",m("cost",cost.getManaCostFor(ability).toString(),"ability",script.ability(ability),"payer",script.seat(player),"visitor",script.objectRef(this),"scope",script.costScope(cost,ability)));script.validateCostScope(cost,ability,ability.isManaAbility()?"implicit_zero_mana":"original");script.require(ability==script.selected&&(ability.isManaAbility()?cost.getManaCostFor(ability).isZero():cost.getManaCostFor(ability).toString().equals("{1}{W}")),"mana_visitor_context",m());script.leave(c,m("payment_decision_number",0,"part_origin",ability.isManaAbility()?"implicit_zero_mana":"original","visitor",script.objectRef(this)));return PaymentDecision.number(0); }
        @Override public PaymentDecision visit(CostBehold cost) { throw script.unexpected(player,"visit_CostBehold",cost); }
        @Override public PaymentDecision visit(CostBeholdExile cost) { throw script.unexpected(player,"visit_CostBeholdExile",cost); }
        @Override public PaymentDecision visit(CostGainControl cost) { throw script.unexpected(player,"visit_CostGainControl",cost); }
        @Override public PaymentDecision visit(CostChooseColor cost) { throw script.unexpected(player,"visit_CostChooseColor",cost); }
        @Override public PaymentDecision visit(CostChooseCreatureType cost) { throw script.unexpected(player,"visit_CostChooseCreatureType",cost); }
        @Override public PaymentDecision visit(CostCollectEvidence cost) { throw script.unexpected(player,"visit_CostCollectEvidence",cost); }
        @Override public PaymentDecision visit(CostDiscard cost) { throw script.unexpected(player,"visit_CostDiscard",cost); }
        @Override public PaymentDecision visit(CostDamage cost) { throw script.unexpected(player,"visit_CostDamage",cost); }
        @Override public PaymentDecision visit(CostDraw cost) { throw script.unexpected(player,"visit_CostDraw",cost); }
        @Override public PaymentDecision visit(CostExile cost) { throw script.unexpected(player,"visit_CostExile",cost); }
        @Override public PaymentDecision visit(CostExileFromStack cost) { throw script.unexpected(player,"visit_CostExileFromStack",cost); }
        @Override public PaymentDecision visit(CostExiledMoveToGrave cost) { throw script.unexpected(player,"visit_CostExiledMoveToGrave",cost); }
        @Override public PaymentDecision visit(CostExert cost) { throw script.unexpected(player,"visit_CostExert",cost); }
        @Override public PaymentDecision visit(CostEnlist cost) { throw script.unexpected(player,"visit_CostEnlist",cost); }
        @Override public PaymentDecision visit(CostFlipCoin cost) { throw script.unexpected(player,"visit_CostFlipCoin",cost); }
        @Override public PaymentDecision visit(CostForage cost) { throw script.unexpected(player,"visit_CostForage",cost); }
        @Override public PaymentDecision visit(CostRollDice cost) { throw script.unexpected(player,"visit_CostRollDice",cost); }
        @Override public PaymentDecision visit(CostMill cost) { throw script.unexpected(player,"visit_CostMill",cost); }
        @Override public PaymentDecision visit(CostAddMana cost) { throw script.unexpected(player,"visit_CostAddMana",cost); }
        @Override public PaymentDecision visit(CostPayLife cost) { throw script.unexpected(player,"visit_CostPayLife",cost); }
        @Override public PaymentDecision visit(CostPayEnergy cost) { throw script.unexpected(player,"visit_CostPayEnergy",cost); }
        @Override public PaymentDecision visit(CostGainLife cost) { throw script.unexpected(player,"visit_CostGainLife",cost); }
        @Override public PaymentDecision visit(CostPromiseGift cost) { throw script.unexpected(player,"visit_CostPromiseGift",cost); }
        @Override public PaymentDecision visit(CostPutCardToLib cost) { throw script.unexpected(player,"visit_CostPutCardToLib",cost); }
        @Override public PaymentDecision visit(CostSacrifice cost) { throw script.unexpected(player,"visit_CostSacrifice",cost); }
        @Override public PaymentDecision visit(CostReturn cost) { throw script.unexpected(player,"visit_CostReturn",cost); }
        @Override public PaymentDecision visit(CostReveal cost) { throw script.unexpected(player,"visit_CostReveal",cost); }
        @Override public PaymentDecision visit(CostRevealChosen cost) { throw script.unexpected(player,"visit_CostRevealChosen",cost); }
        @Override public PaymentDecision visit(CostRemoveAnyCounter cost) { throw script.unexpected(player,"visit_CostRemoveAnyCounter",cost); }
        @Override public PaymentDecision visit(CostRemoveCounter cost) { throw script.unexpected(player,"visit_CostRemoveCounter",cost); }
        @Override public PaymentDecision visit(CostPutCounter cost) { throw script.unexpected(player,"visit_CostPutCounter",cost); }
        @Override public PaymentDecision visit(CostPutCounterYou cost) { throw script.unexpected(player,"visit_CostPutCounterYou",cost); }
        @Override public PaymentDecision visit(CostUntapType cost) { throw script.unexpected(player,"visit_CostUntapType",cost); }
        @Override public PaymentDecision visit(CostUntap cost) { throw script.unexpected(player,"visit_CostUntap",cost); }
        @Override public PaymentDecision visit(CostUnattach cost) { throw script.unexpected(player,"visit_CostUnattach",cost); }
        @Override public PaymentDecision visit(CostTapType cost) { throw script.unexpected(player,"visit_CostTapType",cost); }
        @Override public PaymentDecision visit(CostPayShards cost) { throw script.unexpected(player,"visit_CostPayShards",cost); }
        @Override public PaymentDecision visit(CostBlight cost) { throw script.unexpected(player,"visit_CostBlight",cost); }
    }

    static final class StrictController extends PlayerController {
        final Script script;
        StrictController(Game game,Player player,LobbyPlayer lobby,Script script){super(game,player,lobby);this.script=script;}
        @Override public void tempShowCards(Iterable<Card> cards){throw script.unexpected(getPlayer(),"tempShowCards",cards);}
        @Override public void endTempShowCards(){throw script.unexpected(getPlayer(),"endTempShowCards");}
        @Override public boolean sideboardsByItself(){throw script.unexpected(getPlayer(),"sideboardsByItself");}
        @Override public List<String> getSideboardingReport(){throw script.unexpected(getPlayer(),"getSideboardingReport");}
        @Override public GameOutcome.AnteResult getAnteResult(){throw script.unexpected(getPlayer(),"getAnteResult");}
        @Override public CardCollectionView cheatShuffle(CardCollectionView list){return script.entropy.shuffled(getPlayer(),list);}
        @Override public Map<DeckSection,List<? extends PaperCard>> complainCardsCantPlayWell(Deck deck){return script.complain(getPlayer(),deck);}
        @Override public void resetAtEndOfTurn(){script.housekeeping(getPlayer(),"resetAtEndOfTurn");}
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(getPlayer(),hostCard,abilities,triggerEvent); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw script.unexpected(getPlayer(),"playSpellAbilityNoStack",effectSA,mayChoseNewTargets); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { throw script.unexpected(getPlayer(),"orderSimultaneousSa",activePlayerSAs); }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { throw script.unexpected(getPlayer(),"orderAndPlaySimultaneousSa",activePlayerSAs); }
        @Override public boolean playTrigger(Card host, WrappedAbility wrapperAbility, boolean isMandatory) { throw script.unexpected(getPlayer(),"playTrigger",host,wrapperAbility,isMandatory); }
        @Override public boolean playSaFromPlayEffect(SpellAbility tgtSA) { throw script.unexpected(getPlayer(),"playSaFromPlayEffect",tgtSA); }
        @Override public List<PaperCard> sideboard(final Deck deck, GameType gameType, String message) { throw script.unexpected(getPlayer(),"sideboard",deck,gameType,message); }
        @Override public List<PaperCard> chooseCardsYouWonToAddToDeck(List<PaperCard> losses) { throw script.unexpected(getPlayer(),"chooseCardsYouWonToAddToDeck",losses); }
        @Override public Map<Card, Integer> assignCombatDamage(Card attacker, CardCollectionView blockers, CardCollectionView remaining, int damageDealt, GameEntity defender, boolean overrideOrder) { return script.assign(getPlayer(),attacker,blockers,remaining,damageDealt,defender,overrideOrder); }
        @Override public Map<GameEntity, Integer> divideShield(Card effectSource, Map<GameEntity, Integer> affected, int shieldAmount) { throw script.unexpected(getPlayer(),"divideShield",effectSource,affected,shieldAmount); }
        @Override public Map<Byte, Integer> specifyManaCombo(SpellAbility sa, ColorSet colorSet, int manaAmount, boolean different) { throw script.unexpected(getPlayer(),"specifyManaCombo",sa,colorSet,manaAmount,different); }
        @Override public CardCollectionView choosePermanentsToSacrifice(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw script.unexpected(getPlayer(),"choosePermanentsToSacrifice",sa,min,max,validTargets,message); }
        @Override public CardCollectionView choosePermanentsToDestroy(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw script.unexpected(getPlayer(),"choosePermanentsToDestroy",sa,min,max,validTargets,message); }
        @Override public Integer announceRequirements(SpellAbility ability, int min, int max, String announce) { throw script.unexpected(getPlayer(),"announceRequirements",ability,min,max,announce); }
        @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) { throw script.unexpected(getPlayer(),"chooseNewTargetsFor",ability,filter,optional); }
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { throw script.unexpected(getPlayer(),"chooseTargetsFor",currentAbility); }
        @Override public Pair<SpellAbilityStackInstance, GameObject> chooseTarget(SpellAbility sa, List<Pair<SpellAbilityStackInstance, GameObject>> allTargets) { throw script.unexpected(getPlayer(),"chooseTarget",sa,allTargets); }
        @Override public boolean helpPayForAssistSpell(ManaCostBeingPaid cost, SpellAbility sa, int max, int requested) { throw script.unexpected(getPlayer(),"helpPayForAssistSpell",cost,sa,max,requested); }
        @Override public Player choosePlayerToAssistPayment(FCollectionView<Player> optionList, SpellAbility sa, String title, int max) { throw script.unexpected(getPlayer(),"choosePlayerToAssistPayment",optionList,sa,title,max); }
        @Override public CardCollectionView chooseCardsForEffect(CardCollectionView sourceList, SpellAbility sa, String title, int min, int max, boolean isOptional, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseCardsForEffect",sourceList,sa,title,min,max,isOptional,params); }
        @Override public CardCollection chooseCardsForEffectMultiple(Map<String, CardCollection> validMap, SpellAbility sa, String title, boolean isOptional) { throw script.unexpected(getPlayer(),"chooseCardsForEffectMultiple",validMap,sa,title,isOptional); }
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSingleEntityForEffect",optionList,delayedReveal,sa,title,isOptional,relatedPlayer,params); }
        @Override public <T extends GameEntity> List<T> chooseEntitiesForEffect(FCollectionView<T> optionList, int min, int max, DelayedReveal delayedReveal, SpellAbility sa, String title, Player relatedPlayer, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseEntitiesForEffect",optionList,min,max,delayedReveal,sa,title,relatedPlayer,params); }
        @Override public List<SpellAbility> chooseSpellAbilitiesForEffect(List<SpellAbility> spells, SpellAbility sa, String title, int num, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSpellAbilitiesForEffect",spells,sa,title,num,params); }
        @Override public SpellAbility chooseSingleSpellForEffect(List<SpellAbility> spells, SpellAbility sa, String title, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSingleSpellForEffect",spells,sa,title,params); }
        @Override public boolean confirmAction(SpellAbility sa, PlayerActionConfirmMode mode, String message, List<String> options, Card cardToShow, Map<String, Object> params) { throw script.unexpected(getPlayer(),"confirmAction",sa,mode,message,options,cardToShow,params); }
        @Override public boolean confirmBidAction(SpellAbility sa, PlayerActionConfirmMode bidlife, String string, int bid, Player winner) { throw script.unexpected(getPlayer(),"confirmBidAction",sa,bidlife,string,bid,winner); }
        @Override public boolean confirmReplacementEffect(ReplacementEffect replacementEffect, SpellAbility effectSA, GameEntity affected, String question) { throw script.unexpected(getPlayer(),"confirmReplacementEffect",replacementEffect,effectSA,affected,question); }
        @Override public boolean confirmStaticApplication(Card hostCard, PlayerActionConfirmMode mode, String message, String logic) { throw script.unexpected(getPlayer(),"confirmStaticApplication",hostCard,mode,message,logic); }
        @Override public boolean confirmTrigger(WrappedAbility sa) { throw script.unexpected(getPlayer(),"confirmTrigger",sa); }
        @Override public List<Card> exertAttackers(List<Card> attackers) { throw script.unexpected(getPlayer(),"exertAttackers",attackers); }
        @Override public List<Card> enlistAttackers(List<Card> attackers) { throw script.unexpected(getPlayer(),"enlistAttackers",attackers); }
        @Override public void declareAttackers(Player attacker, Combat combat) { script.attackers(getPlayer(),attacker,combat); }
        @Override public void declareBlockers(Player defender, Combat combat) { script.blockers(getPlayer(),defender,combat); }
        @Override public CardCollection orderBlockers(Card attacker, CardCollection blockers) { throw script.unexpected(getPlayer(),"orderBlockers",attacker,blockers); }
        @Override public CardCollection orderBlocker(final Card attacker, final Card blocker, final CardCollection oldBlockers) { throw script.unexpected(getPlayer(),"orderBlocker",attacker,blocker,oldBlockers); }
        @Override public CardCollection orderAttackers(Card blocker, CardCollection attackers) { throw script.unexpected(getPlayer(),"orderAttackers",blocker,attackers); }
        @Override public void reveal(CardCollectionView cards, ZoneType zone, Player owner, String messagePrefix, boolean addMsgSuffix) { throw script.unexpected(getPlayer(),"reveal",cards,zone,owner,messagePrefix,addMsgSuffix); }
        @Override public void reveal(List<CardView> cards, ZoneType zone, PlayerView owner, String messagePrefix, boolean addMsgSuffix) { throw script.unexpected(getPlayer(),"reveal",cards,zone,owner,messagePrefix,addMsgSuffix); }
        @Override public void notifyOfValue(SpellAbility saSource, GameObject realtedTarget, String value) { throw script.unexpected(getPlayer(),"notifyOfValue",saSource,realtedTarget,value); }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForScry(CardCollection topN) { throw script.unexpected(getPlayer(),"arrangeForScry",topN); }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForSurveil(CardCollection topN) { throw script.unexpected(getPlayer(),"arrangeForSurveil",topN); }
        @Override public boolean willPutCardOnTop(Card c) { throw script.unexpected(getPlayer(),"willPutCardOnTop",c); }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destinationZone, SpellAbility source) { return script.order(getPlayer(),cards,destinationZone,source); }
        @Override public CardCollectionView chooseCardsToDiscardFrom(Player playerDiscard, SpellAbility sa, CardCollection validCards, int min, int max, CardCollectionView visibleToChooser) { throw script.unexpected(getPlayer(),"chooseCardsToDiscardFrom",playerDiscard,sa,validCards,min,max,visibleToChooser); }
        @Override public CardCollectionView chooseCardsToDiscardUnlessType(int min, CardCollectionView hand, String[] unlessTypes, SpellAbility sa) { throw script.unexpected(getPlayer(),"chooseCardsToDiscardUnlessType",min,hand,unlessTypes,sa); }
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { return script.discard(getPlayer(),numDiscard); }
        @Override public CardCollectionView chooseCardsToDelve(int genericAmount, CardCollection grave) { throw script.unexpected(getPlayer(),"chooseCardsToDelve",genericAmount,grave); }
        @Override public Map<Card, ManaCostShard> chooseCardsForConvokeOrImprovise(SpellAbility sa, ManaCost manaCost, CardCollectionView untappedCards, boolean artifacts, boolean creatures, Integer maxReduction) { throw script.unexpected(getPlayer(),"chooseCardsForConvokeOrImprovise",sa,manaCost,untappedCards,artifacts,creatures,maxReduction); }
        @Override public List<Card> chooseCardsForSplice(SpellAbility sa, List<Card> cards) { throw script.unexpected(getPlayer(),"chooseCardsForSplice",sa,cards); }
        @Override public CardCollectionView chooseCardsToRevealFromHand(int min, int max, CardCollectionView valid) { throw script.unexpected(getPlayer(),"chooseCardsToRevealFromHand",min,max,valid); }
        @Override public List<SpellAbility> chooseSaToActivateFromOpeningHand(List<SpellAbility> usableFromOpeningHand) { throw script.unexpected(getPlayer(),"chooseSaToActivateFromOpeningHand",usableFromOpeningHand); }
        @Override public Player chooseStartingPlayer(boolean isFirstGame) { return script.starting(getPlayer(),isFirstGame); }
        @Override public PlayerZone chooseStartingHand(List<PlayerZone> zones) { throw script.unexpected(getPlayer(),"chooseStartingHand",zones); }
        @Override public Mana chooseManaFromPool(List<Mana> manaChoices) { throw script.unexpected(getPlayer(),"chooseManaFromPool",manaChoices); }
        @Override public String chooseSomeType(String kindOfType, SpellAbility sa, Collection<String> validTypes, boolean isOptional) { throw script.unexpected(getPlayer(),"chooseSomeType",kindOfType,sa,validTypes,isOptional); }
        @Override public String chooseSector(Card assignee, String ai, List<String> sectors) { throw script.unexpected(getPlayer(),"chooseSector",assignee,ai,sectors); }
        @Override public Sticker chooseSticker(List<Sticker> options, Card target, SpellAbility sa, boolean isOptional) { throw script.unexpected(getPlayer(),"chooseSticker",options,target,sa,isOptional); }
        @Override public int chooseStickerNamePosition(Sticker sticker, Card target) { throw script.unexpected(getPlayer(),"chooseStickerNamePosition",sticker,target); }
        @Override public Card chooseCardToKeepStickers(CardCollectionView options) { throw script.unexpected(getPlayer(),"chooseCardToKeepStickers",options); }
        @Override public List<Card> chooseContraptionsToCrank(List<Card> contraptions) { throw script.unexpected(getPlayer(),"chooseContraptionsToCrank",contraptions); }
        @Override public int chooseSprocket(Card assignee, List<Integer> sprockets) { throw script.unexpected(getPlayer(),"chooseSprocket",assignee,sprockets); }
        @Override public PlanarDice choosePDRollToIgnore(List<PlanarDice> rolls) { throw script.unexpected(getPlayer(),"choosePDRollToIgnore",rolls); }
        @Override public Integer chooseRollToIgnore(List<Integer> rolls) { throw script.unexpected(getPlayer(),"chooseRollToIgnore",rolls); }
        @Override public List<Integer> chooseDiceToReroll(List<Integer> rolls) { throw script.unexpected(getPlayer(),"chooseDiceToReroll",rolls); }
        @Override public Integer chooseRollToModify(List<Integer> rolls) { throw script.unexpected(getPlayer(),"chooseRollToModify",rolls); }
        @Override public RollDiceEffect.DieRollResult chooseRollToSwap(List<RollDiceEffect.DieRollResult> rolls) { throw script.unexpected(getPlayer(),"chooseRollToSwap",rolls); }
        @Override public String chooseRollSwapValue(List<String> swapChoices, Integer currentResult, int power, int toughness) { throw script.unexpected(getPlayer(),"chooseRollSwapValue",swapChoices,currentResult,power,toughness); }
        @Override public Object vote(SpellAbility sa, String prompt, List<Object> options, ListMultimap<Object, Player> votes, Player forPlayer, boolean optional) { throw script.unexpected(getPlayer(),"vote",sa,prompt,options,votes,forPlayer,optional); }
        @Override public boolean mulliganKeepHand(Player player, int cardsToReturn) { return script.keep(getPlayer(),player,cardsToReturn); }
        @Override public CardCollectionView tuckCardsViaMulligan(CardCollectionView hand, int cardsToReturn) { return script.bottom(getPlayer(),hand,cardsToReturn); }
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { return script.choose(getPlayer()); }
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { return script.play(getPlayer(),sa); }
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { throw script.unexpected(getPlayer(),"chooseModeForAbility",sa,possible,min,num,allowRepeat); }
        @Override public int chooseNumberForCostReduction(final SpellAbility sa, final int min, final int max) { throw script.unexpected(getPlayer(),"chooseNumberForCostReduction",sa,min,max); }
        @Override public int chooseNumberForKeywordCost(SpellAbility sa, Cost cost, KeywordInterface keyword, String prompt, int max) { throw script.unexpected(getPlayer(),"chooseNumberForKeywordCost",sa,cost,keyword,prompt,max); }
        @Override public int chooseNumber(SpellAbility sa, String title, int min, int max) { throw script.unexpected(getPlayer(),"chooseNumber",sa,title,min,max); }
        @Override public int chooseNumber(SpellAbility sa, String title, List<Integer> values, Player relatedPlayer) { throw script.unexpected(getPlayer(),"chooseNumber",sa,title,values,relatedPlayer); }
        @Override public boolean chooseBinary(SpellAbility sa, String question, BinaryChoiceType kindOfChoice, Boolean defaultChoice) { throw script.unexpected(getPlayer(),"chooseBinary",sa,question,kindOfChoice,defaultChoice); }
        @Override public boolean chooseFlipResult(SpellAbility sa, Player flipper, boolean call) { throw script.unexpected(getPlayer(),"chooseFlipResult",sa,flipper,call); }
        @Override public byte chooseColor(String message, SpellAbility sa, ColorSet colors) { throw script.unexpected(getPlayer(),"chooseColor",message,sa,colors); }
        @Override public byte chooseColorAllowColorless(String message, Card c, ColorSet colors) { throw script.unexpected(getPlayer(),"chooseColorAllowColorless",message,c,colors); }
        @Override public ColorSet chooseColors(String message, SpellAbility sa, int min, int max, ColorSet options) { throw script.unexpected(getPlayer(),"chooseColors",message,sa,min,max,options); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, String message, Predicate<ICardFace> cpp, String name) { throw script.unexpected(getPlayer(),"chooseSingleCardFace",sa,message,cpp,name); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, List<ICardFace> faces, String message) { throw script.unexpected(getPlayer(),"chooseSingleCardFace",sa,faces,message); }
        @Override public CardState chooseSingleCardState(SpellAbility sa, List<CardState> states, String message, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSingleCardState",sa,states,message,params); }
        @Override public boolean chooseCardsPile(SpellAbility sa, CardCollectionView pile1, CardCollectionView pile2, String faceUp) { throw script.unexpected(getPlayer(),"chooseCardsPile",sa,pile1,pile2,faceUp); }
        @Override public CounterType chooseCounterType(List<CounterType> options, SpellAbility sa, String prompt, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseCounterType",options,sa,prompt,params); }
        @Override public String chooseKeywordForPump(List<String> options, SpellAbility sa, String prompt, Card tgtCard) { throw script.unexpected(getPlayer(),"chooseKeywordForPump",options,sa,prompt,tgtCard); }
        @Override public boolean confirmPayment(CostPart costPart, String string, SpellAbility sa) { throw script.unexpected(getPlayer(),"confirmPayment",costPart,string,sa); }
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { throw script.unexpected(getPlayer(),"chooseSingleReplacementEffect",possibleReplacers); }
        @Override public StaticAbility chooseSingleStaticAbility(List<StaticAbility> possibleReplacers) { throw script.unexpected(getPlayer(),"chooseSingleStaticAbility",possibleReplacers); }
        @Override public String chooseProtectionType(SpellAbility sa, List<String> choices) { throw script.unexpected(getPlayer(),"chooseProtectionType",sa,choices); }
        @Override public void revealAnte(String message, Multimap<Player, PaperCard> removedAnteCards) { throw script.unexpected(getPlayer(),"revealAnte",message,removedAnteCards); }
        @Override public void revealAISkipCards(String message, Map<Player, Map<DeckSection, List<? extends PaperCard>>> deckCards) { throw script.unexpected(getPlayer(),"revealAISkipCards",message,deckCards); }
        @Override public void revealUnsupported(Map<Player, List<PaperCard>> unsupported) { script.unsupported(getPlayer(),unsupported); }
        @Override public List<OptionalCostValue> chooseOptionalCosts(SpellAbility choosen, List<OptionalCostValue> optionalCostValues) { throw script.unexpected(getPlayer(),"chooseOptionalCosts",choosen,optionalCostValues); }
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { throw script.unexpected(getPlayer(),"orderCosts",costs); }
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { throw script.unexpected(getPlayer(),"payCostToPreventEffect",cost,sa,alreadyPaid,allPayers); }
        @Override public boolean payCostDuringRoll(Cost cost, SpellAbility sa) { throw script.unexpected(getPlayer(),"payCostDuringRoll",cost,sa); }
        @Override public boolean payCombatCost(Card card, Cost cost, SpellAbility sa, String prompt) { throw script.unexpected(getPlayer(),"payCombatCost",card,cost,sa,prompt); }
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.pay(getPlayer(),toPay,costPartMana,sa,prompt,matrix,effect); }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.apply(getPlayer(),toPay,ability,prompt,matrix,effect); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw script.unexpected(getPlayer(),"chooseCardsForCost",optionList,sa,cpl,amount,isOptional,prompt); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { return script.costs(getPlayer(),player,ability,effect,prompt); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw script.unexpected(getPlayer(),"chooseCardName",sa,cpp,valid,message); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw script.unexpected(getPlayer(),"chooseCardName",sa,faces,message); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw script.unexpected(getPlayer(),"chooseSingleCardForZoneChange",destination,origin,sa,fetchList,delayedReveal,selectPrompt,isOptional,decider); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw script.unexpected(getPlayer(),"chooseCardsForZoneChange",destination,origin,sa,fetchList,min,max,delayedReveal,selectPrompt,decider); }
        @Override public void autoPassCancel() { script.housekeeping(getPlayer(),"autoPassCancel"); }
        @Override public void awaitNextInput() { script.housekeeping(getPlayer(),"awaitNextInput"); }
        @Override public void cancelAwaitNextInput() { throw script.unexpected(getPlayer(),"cancelAwaitNextInput"); }
    }

    static final class Events {
        final Script s; Events(Script s){this.s=s;}
        void emit(String name,Supplier<Map<String,Object>> payload){try{s.event(name,payload.get());}catch(Throwable t){if(s.failures.isEmpty())s.fatal("observer_exception",m("event",name,"class",t.getClass().getName(),"message",t.getMessage()));}}
        Object entity(GameEntityView e){if(e instanceof PlayerView p)return m("kind","player","seat",s.seatView(p));if(e instanceof CardView c)return m("kind","card","card",s.view(c));throw s.bad("event_entity",m("class",e.getClass().getName()));}
        List<Map<String,Object>> sorted(List<Map<String,Object>> values){values.sort(Comparator.comparing(JSON::toJson));return values;}
        List<Map<String,Object>> attacks(GameEventAttackersDeclared e){List<Map<String,Object>> rows=new ArrayList<>();for(var x:e.attackersMap().entries())rows.add(m("defender",entity(x.getKey()),"attacker",s.view(x.getValue())));return sorted(rows);}
        List<Map<String,Object>> blocks(GameEventBlockersDeclared e){List<Map<String,Object>> rows=new ArrayList<>();for(var outer:e.blockers().entrySet())for(var x:outer.getValue().entries())rows.add(m("defender",entity(outer.getKey()),"attacker",s.view(x.getKey()),"value",s.view(x.getValue()),"unblocked_self_sentinel",x.getKey().getId()==x.getValue().getId()));return sorted(rows);}
        @Subscribe public void shuffle(GameEventShuffle e){emit("GameEventShuffle",()->m("player",s.seatView(e.player())));}
        @Subscribe public void mulligan(GameEventMulligan e){emit("GameEventMulligan",()->m("player",s.seatView(e.player())));}
        @Subscribe public void flip(GameEventFlipCoin e){emit("GameEventFlipCoin",()->m("flipper",s.seatView(e.flipper()),"heads",e.heads(),"startingToss",e.startingToss()));}
        @Subscribe public void started(GameEventGameStarted e){emit("GameEventGameStarted",()->{List<String> players=new ArrayList<>();for(PlayerView p:e.players())players.add(s.seatView(p));return m("gameType",e.gameType().name(),"firstTurn",s.seatView(e.firstTurn()),"players",players);});}
        @Subscribe public void began(GameEventTurnBegan e){emit("GameEventTurnBegan",()->m("turnOwner",s.seatView(e.turnOwner()),"turnNumber",e.turnNumber()));}
        @Subscribe public void phase(GameEventTurnPhase e){emit("GameEventTurnPhase",()->{s.require(!e.phaseDesc().equals("dev"),"dev_phase",m());return m("playerTurn",s.seatView(e.playerTurn()),"phase",e.phase().name(),"phaseDesc",e.phaseDesc());});}
        @Subscribe public void priority(GameEventPlayerPriority e){emit("GameEventPlayerPriority",()->m("turn",s.seatView(e.turn()),"phase",e.phase().name(),"priority",s.seatView(e.priority())));}
        @Subscribe public void land(GameEventLandPlayed e){emit("GameEventLandPlayed",()->m("player",s.seatView(e.player()),"land",s.view(e.land())));}
        @Subscribe public void zone(GameEventCardChangeZone e){emit("GameEventCardChangeZone",()->m("card",s.view(e.card()),"from",s.zone(e.from()),"to",s.zone(e.to())));}
        @Subscribe public void tapped(GameEventCardTapped e){emit("GameEventCardTapped",()->m("card",s.view(e.card()),"tapped",e.tapped()));}
        @Subscribe public void mana(GameEventManaPool e){emit("GameEventManaPool",()->m("player",s.seatView(e.player()),"mode",e.mode().name(),"colors",e.colors()==null?null:e.colors().stream().map(x->x.getShortName()).sorted().toList()));}
        @Subscribe public void cast(GameEventSpellAbilityCast e){emit("GameEventSpellAbilityCast",()->m("sa",s.view(e.sa()),"si",s.view(e.si()),"stackIndex",e.stackIndex(),"targetDescription",e.targetDescription()));}
        @Subscribe public void resolved(GameEventSpellResolved e){emit("GameEventSpellResolved",()->m("spell",s.view(e.spell()),"hasFizzled",e.hasFizzled(),"stackDescription",e.stackDescription()));}
        @Subscribe public void removed(GameEventSpellRemovedFromStack e){emit("GameEventSpellRemovedFromStack",()->m("sa",s.view(e.sa())));}
        @Subscribe public void attackers(GameEventAttackersDeclared e){emit("GameEventAttackersDeclared",()->m("player",s.seatView(e.player()),"attackersMap",attacks(e)));}
        @Subscribe public void blockers(GameEventBlockersDeclared e){emit("GameEventBlockersDeclared",()->m("defendingPlayer",s.seatView(e.defendingPlayer()),"blockers",blocks(e)));}
        @Subscribe public void cardDamage(GameEventCardDamaged e){emit("GameEventCardDamaged",()->m("card",s.view(e.card()),"source",s.view(e.source()),"amount",String.valueOf(e.amount()),"type",e.type().name()));}
        @Subscribe public void playerDamage(GameEventPlayerDamaged e){emit("GameEventPlayerDamaged",()->m("target",s.seatView(e.target()),"source",s.view(e.source()),"amount",String.valueOf(e.amount()),"combat",e.combat(),"infect",e.infect()));}
        @Subscribe public void lives(GameEventPlayerLivesChanged e){emit("GameEventPlayerLivesChanged",()->m("player",s.seatView(e.player()),"oldLives",String.valueOf(e.oldLives()),"newLives",String.valueOf(e.newLives())));}
        @Subscribe public void combatEnd(GameEventCombatEnded e){emit("GameEventCombatEnded",()->m("attackers",e.attackers()==null?null:e.attackers().stream().map(s::view).toList(),"blockers",e.blockers()==null?null:e.blockers().stream().map(s::view).toList()));}
        @Subscribe public void turnEnd(GameEventTurnEnded e){emit("GameEventTurnEnded",()->m());}
        @Subscribe public void control(GameEventPlayerControl e){emit("GameEventPlayerControl",()->m("player",s.seatView(e.player()),"newLobbyPlayerName",e.newLobbyPlayerName(),"newControllerIsHuman",e.newControllerIsHuman(),"actual_stack_tags",s.stackTags()));}
        @Subscribe public void outcome(GameEventGameOutcome e){emit("GameEventGameOutcome",()->m("lastTurnNumber",e.lastTurnNumber(),"outcomeStrings",e.outcomeStrings(),"winningPlayerName",e.winningPlayerName(),"matchSummary",e.matchSummary()));}
        @Subscribe public void finished(GameEventGameFinished e){emit("GameEventGameFinished",()->m());}
    }
}
