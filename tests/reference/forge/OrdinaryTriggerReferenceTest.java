package forge.ai;

import com.google.common.collect.*;
import com.google.common.eventbus.Subscribe;
import com.google.gson.*;
import forge.LobbyPlayer;
import forge.card.*;
import forge.card.mana.*;
import forge.deck.*;
import forge.game.*;
import forge.game.ability.AbilityKey;
import forge.game.ability.ApiType;
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
import forge.game.trigger.*;
import forge.game.zone.*;
import forge.item.*;
import forge.model.FModel;
import forge.util.*;
import forge.util.collect.FCollectionView;
import org.apache.commons.lang3.tuple.*;
import org.testng.annotations.DataProvider;
import org.testng.annotations.Test;
import java.io.*;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.function.*;

/** Input-only finite prepared probes of the pinned engine's ordinary trigger route. */
public class OrdinaryTriggerReferenceTest extends AITest {
    static final String PIN="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    static final String INPUT_SHA="263acc91dec6d869519af7ea296348365591bdcc383a039622db46fc92765c40";
    static final String PROTOCOL_SHA="104681f32771c55ab46610bf2469c142a4cd0d76cfec9e4e63ec8249a696a20c";
    static final Gson JSON=new GsonBuilder().serializeNulls().disableHtmlEscaping().create();
    static Map<String,Object> m(Object... xs){Map<String,Object> r=new LinkedHashMap<>();for(int i=0;i<xs.length;i+=2)r.put((String)xs[i],xs[i+1]);return r;}
    static String s(JsonObject o,String k){return o.get(k).getAsString();}
    static List<JsonObject> objs(JsonArray a){List<JsonObject> r=new ArrayList<>();for(JsonElement x:a)r.add(x.getAsJsonObject());return r;}
    static List<String> strs(JsonArray a){List<String> r=new ArrayList<>();for(JsonElement x:a)r.add(x.getAsString());return r;}
    static String sha(byte[] b)throws Exception{return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(b));}
    static JsonObject inputs()throws Exception{byte[] b=Files.readAllBytes(Path.of(Objects.requireNonNull(System.getProperty("spellwake.inputs.path"))));if(!sha(b).equals(INPUT_SHA))throw new AssertionError("Frozen input bytes mismatch");return JsonParser.parseString(new String(b,StandardCharsets.UTF_8)).getAsJsonObject();}
    @DataProvider(name="probes") public Object[][] probes()throws Exception{List<Object[]> r=new ArrayList<>();for(JsonObject f:objs(inputs().getAsJsonArray("fixtures")))if(f.getAsJsonObject("engine_recipes").getAsJsonObject("forge").get("selected").getAsBoolean())r.add(new Object[]{s(f,"id")});if(r.size()!=14)throw new AssertionError("Selected protocol count");return r.toArray(Object[][]::new);}
    @Test(dataProvider="probes") public void probe(String id)throws Exception{
        JsonObject root=inputs(),fixture=null;for(JsonObject f:objs(root.getAsJsonArray("fixtures")))if(s(f,"id").equals(id))fixture=f;
        if(fixture==null)throw new AssertionError("Unknown fixture");Script x=new Script(root,fixture);Throwable failure=null;
        try{x.captureClasses();x.construct();x.game.subscribeToEvents(x);x.observe("operation","setupFirstTurn.entry",m("hook",null,"world",x.world()));x.game.getPhaseHandler().setupFirstTurn(x.seats.get(s(fixture.getAsJsonObject("seed"),"active")),null);x.observe("operation","setupFirstTurn.return",m("world",x.world()));
            try{for(int i=0;i<x.recipe.getAsJsonObject("budget").get("max_mainLoopStep_calls").getAsInt();i++){x.healthy();x.require(!x.game.isGameOver(),"unexpected_game_over",m());x.observe("loop","entry",m("index",i,"world",x.world()));x.game.getPhaseHandler().mainLoopStep();x.observe("loop","return",m("index",i,"world",x.world()));}throw x.bad("loop_budget_exhausted",m());}
            catch(ProbeStop stop){x.require(stop==x.stop,"foreign_stop_marker",m());x.healthy();x.stopped=true;x.observe("operation","probe_stop_caught",m("natural_game_return",false,"world",x.world()));}
            x.localPermanentAssociationControls();
        }catch(Throwable t){failure=t;x.failure("uncaught",t);}
        finally{Path out=Path.of(System.getProperty("spellwake.evidence.dir"),x.probe+".json");Files.writeString(out,JSON.toJson(m("schema_version",1,"kind","ordinary-trigger-reference-raw","fixture_id",id,"probe_id",x.probe,"forge_commit",PIN,"input_sha256",INPUT_SHA,"protocol_sha256",root.getAsJsonObject("selected_forge_protocol").get("sha256"),"probe_stop",x.stopped,"natural_game_return",false,"ai_policy_used",false,"strict_first_controller",true,"private_waiting_inspected",false,"direct_resolution_used",false,"read_inputs",List.of(System.getProperty("spellwake.inputs.path")),"printing_and_definitions",x.definitions,"loaded_classes",x.classes,"immutable_permanent_receipts",x.permanentReceiptProofs(),"immutable_captured_bindings",x.bindingProofs(),"observations",x.records,"failure",failure==null?null:m("class",failure.getClass().getName(),"message",failure.getMessage()),"limits",List.of("Prepared finite selected protocol, no complete-card or natural full-game claim.","Public publications do not prove private waiting or native owned event cuts.","Native Pending, rollback, shared pre-SBA and pre-membership worlds are not reference parity claims.")))+"\n",StandardOpenOption.CREATE_NEW);}
        if(failure!=null)throw new AssertionError("Recorded reference failure "+id,failure);
    }
    static final class ProbeStop extends RuntimeException {private static final long serialVersionUID=1L;ProbeStop(){super("selected nonterminal priority stop",null,false,false);}}
    static final class StrictLobby extends LobbyPlayer implements IGameEntitiesFactory {
        final Script script;final String seat;StrictLobby(String seat,Script script){super("trigger-"+seat);this.seat=seat;this.script=script;}
        @Override public Player createIngamePlayer(Game g,int id){Player p=new Player(getName(),g,id);p.setFirstController(new StrictController(g,p,this,script));script.seats.put(seat,p);return p;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw script.bad("mind_slave_factory",m());}
        @Override public void hear(LobbyPlayer p,String message){throw script.bad("lobby_message",m("sender",p.getName(),"message",message));}
    }
    static final class Placed {final WrappedAbility wrapper;final SpellAbility body;final Player controller;final int entry;final String token;boolean resolved;Placed(WrappedAbility w,int entry,String token){wrapper=w;body=w.getWrappedAbility();controller=w.getActivatingPlayer();this.entry=entry;this.token=token;}}
    /** A captured key/snapshot never reads its identity back from the mutable Card. */
    static final class CapturedObject {
        final Card reference;
        final int id;
        final long timestamp;
        final JsonElement snapshot;
        CapturedObject(Card card, JsonElement snapshot) {
            reference=card;id=card.getId();timestamp=card.getGameTimestamp();
            this.snapshot=snapshot.deepCopy();
        }
        boolean sameIncarnation(Card card) {
            return card!=null&&id==card.getId()&&timestamp==card.getGameTimestamp();
        }
    }
    static final class Admission {
        final String alias;
        final SpellAbilityStackInstance instance;
        final SpellAbility ability;
        final CapturedObject source;
        final Player controller;
        final boolean permanent;
        Admission(String alias, SpellAbilityStackInstance instance, CapturedObject source) {
            this.alias=alias;this.instance=instance;this.source=source;
            ability=instance.getSpellAbility();controller=ability.getActivatingPlayer();
            permanent=ability.isSpell()&&(ability.getApi()==ApiType.PermanentCreature||ability.getApi()==ApiType.PermanentNoncreature);
        }
    }
    /** Actual public lifetime facts frozen at a Stack-to-Battlefield publication. */
    static final class PermanentPublication {
        final SpellAbilityStackInstance top;
        final SpellAbility ability;
        final CapturedObject hostAtPublication,after;
        final Player controller,afterController;
        final boolean resolving,sourceResolving,stackToBattlefield,afterInBattlefield;
        final int liveIdentityCount,publicationCardId;
        PermanentPublication(SpellAbilityStackInstance top, CapturedObject host, CapturedObject after,
                Player controller, Player afterController, boolean resolving, boolean sourceResolving,
                boolean stackToBattlefield, boolean afterInBattlefield, int liveCount, int cardId) {
            this.top=top;ability=top==null?null:top.getSpellAbility();hostAtPublication=host;this.after=after;
            this.controller=controller;this.afterController=afterController;this.resolving=resolving;
            this.sourceResolving=sourceResolving;this.stackToBattlefield=stackToBattlefield;
            this.afterInBattlefield=afterInBattlefield;liveIdentityCount=liveCount;publicationCardId=cardId;
        }
    }
    static final class PermanentReceipt {
        final Admission admission;
        final PermanentPublication publication;
        PermanentReceipt(Admission admission,PermanentPublication publication) {
            this.admission=admission;this.publication=publication;
        }
    }
    static final class Script {
        final JsonObject root,fixture,recipe,seed;final String probe;final ProbeStop stop=new ProbeStop();
        final Map<String,Player> seats=new LinkedHashMap<>();final Map<String,Card> seeded=new LinkedHashMap<>();final Map<String,CapturedObject> bindings=new LinkedHashMap<>();final Map<String,PermanentReceipt> permanentEntries=new LinkedHashMap<>();final Map<String,Admission> admissions=new LinkedHashMap<>();final IdentityHashMap<SpellAbilityStackInstance,Admission> admissionsByInstance=new IdentityHashMap<>();final Map<Integer,String> aliases=new LinkedHashMap<>();final Map<String,JsonObject> inventory=new LinkedHashMap<>();final Map<String,SpellAbilityStackInstance> entries=new LinkedHashMap<>();
        final Map<String,PaperCard> papers=new LinkedHashMap<>();final Map<String,Object> definitions=new LinkedHashMap<>();final List<JsonElement> records=new ArrayList<>();final List<Map<String,Object>> classes=new ArrayList<>();final IdentityHashMap<Object,String> references=new IdentityHashMap<>();final List<JsonObject> actions=new ArrayList<>(),choices,selectors,cohorts;final Set<Integer> choicesUsed=new LinkedHashSet<>(),assignmentUsed=new LinkedHashSet<>();final Set<String> selectorsUsed=new LinkedHashSet<>();final List<Placed> placed=new ArrayList<>();final Deque<SpellAbility> contexts=new ArrayDeque<>();final IdentityHashMap<SpellAbility,JsonElement> preclearTargets=new IdentityHashMap<>();final Set<Player> notified=new LinkedHashSet<>();
        Game game;int actionCursor,cohortCursor,callbacks,cleanupVisits;long sequence;boolean stopped;Throwable sticky;SpellAbility selected;JsonObject selectedAction;
        Script(JsonObject root,JsonObject fixture){this.root=root;this.fixture=fixture;if(!s(root.getAsJsonObject("selected_forge_protocol"),"sha256").equals(PROTOCOL_SHA))throw new AssertionError("Frozen protocol selection mismatch");seed=fixture.getAsJsonObject("seed");recipe=fixture.getAsJsonObject("engine_recipes").getAsJsonObject("forge");probe=s(recipe,"probe_id");choices=objs(recipe.getAsJsonArray("callback_choices"));selectors=objs(recipe.getAsJsonArray("trigger_selectors"));cohorts=objs(recipe.getAsJsonArray("trigger_placement_callbacks"));for(JsonObject a:objs(recipe.getAsJsonArray("priority_actions")))expand(a);}
        void expand(JsonObject a){String op=s(a,"op");if(op.equals("repeat_bounded")){for(int i=0;i<a.get("count").getAsInt();i++)for(JsonObject child:objs(a.getAsJsonArray("sequence")))expand(child);}else if(op.equals("pass_round")){for(String seat:strs(a.getAsJsonArray("actors"))){JsonObject p=a.deepCopy();p.addProperty("op","pass");p.addProperty("actor",seat);actions.add(p);}}else actions.add(a);}
        void observe(String type,String kind,Map<String,Object> fields){Map<String,Object> row=m("sequence",++sequence,"type",type,"kind",kind);row.putAll(fields);records.add(JSON.toJsonTree(row));}
        void failure(String code,Throwable t){if(sticky==null)sticky=t;observe("failure",code,m("exception",t.getClass().getName(),"message",t.getMessage()));}
        AssertionError bad(String code,Map<String,Object> info){AssertionError t=new AssertionError(code+": "+JSON.toJson(info));failure(code,t);return t;}
        void require(boolean ok,String code,Map<String,Object> info){if(!ok)throw bad(code,info);}
        void healthy(){if(sticky!=null)throw new AssertionError("Sticky protocol/observer failure",sticky);}
        <T>T call(Player p,String method,Map<String,Object> args,Supplier<T> work){healthy();require(++callbacks<=recipe.getAsJsonObject("budget").get("max_callbacks").getAsInt(),"callback_budget",m());strict(p);long id=sequence+1;observe("callback","entry",m("callback_id",id,"method",method,"actor",seat(p),"arguments",args,"frames",StackWalker.getInstance().walk(z->z.map(f->f.getClassName()+"."+f.getMethodName()).toList()),"world",world()));try{T r=work.get();healthy();observe("callback","return",m("callback_id",id,"method",method,"reply",diag(r),"world",world()));return r;}catch(ProbeStop t){observe("callback","nonterminal_stop",m("callback_id",id,"method",method,"world",world()));throw t;}catch(Throwable t){failure("callback_"+method,t);observe("callback","failure",m("callback_id",id,"method",method));throw t;}}
        AssertionError unexpected(Player p,String method,Object...args){return call(p,method,m("offered",diag(Arrays.asList(args))),()->{throw bad("unexpected_callback",m("method",method,"actor",seat(p)));});}
        void housekeeping(Player p,String method){call(p,method,m(),()->null);}
        String ref(Object o){if(o==null)return null;return references.computeIfAbsent(o,k->"ref"+(references.size()+1));}
        String seat(Player p){if(p==null)return null;for(var e:seats.entrySet())if(e.getValue()==p)return e.getKey();throw bad("unknown_player",m("id",p.getId()));}
        String alias(Card c){if(c==null)return null;String a=aliases.get(c.getId());require(a!=null,"unregistered_physical_card",m("id",c.getId(),"name",c.getName()));return a;}
        Card current(String a){Card original=seeded.get(a);require(original!=null,"unknown_alias",m("alias",a));Card c=game.getCardState(original);require(c!=null,"missing_current_object",m("alias",a));return c;}
        Card bound(JsonObject o){String k=o.has("captured_object")?s(o,"captured_object"):s(o,"captured_seed_object");CapturedObject c=bindings.get(k);require(c!=null,"missing_captured_binding",m("binding",k));require(c.sameIncarnation(c.reference),"captured_binding_incarnation_mutated",m("binding",k,"immutable_capture",captureProof(c)));return c.reference;}
        boolean sameObject(Card a,Card b){return a!=null&&b!=null&&a.getId()==b.getId()&&a.getGameTimestamp()==b.getGameTimestamp();}
        Map<String,Object> card(Card c){if(c==null)return null;return m("alias",alias(c),"ref",ref(c),"id",String.valueOf(c.getId()),"timestamp",String.valueOf(c.getGameTimestamp()),"name",c.getName(),"owner",seat(c.getOwner()),"controller",seat(c.getController()),"zone",c.getZone()==null?null:c.getZone().getZoneType().name(),"tapped",c.isTapped(),"creature",c.isCreature(),"power",String.valueOf(c.getNetPower()),"toughness",String.valueOf(c.getNetToughness()),"damage",String.valueOf(c.getDamage()),"counters",diag(c.getCounters()));}
        Map<String,Object> ability(SpellAbility a){if(a==null)return null;return m("ref",ref(a),"id",String.valueOf(a.getId()),"source",card(a.getHostCard()),"controller",seat(a.getActivatingPlayer()),"spell",a.isSpell(),"mana",a.isManaAbility(),"trigger",a.isTrigger(),"wrapper",a.isWrapper(),"api",a.getApi()==null?null:a.getApi().name(),"params",a.getMapParams(),"cost",a.getPayCosts()==null?null:a.getPayCosts().toString(),"targets",diag(a.getTargets()),"trigger_id",a.isTrigger()?String.valueOf(a.getTrigger().getId()):null,"trigger_mode",a.isTrigger()?a.getTrigger().getMode().name():null,"trigger_params",a.isTrigger()?a.getTrigger().getMapParams():null,"triggering",diag(a.getTriggeringObjects()),"original_ability",diag(a.getOriginalAbility()));}
        Object diag(Object o){if(o==null||o instanceof String||o instanceof Number||o instanceof Boolean)return o;if(o instanceof Card c)return card(c);if(o instanceof Player p)return m("seat",seat(p),"id",p.getId());if(o instanceof SpellAbility a)return m("ref",ref(a),"id",String.valueOf(a.getId()),"source_alias",alias(a.getHostCard()));if(o instanceof Mana mana)return m("ref",ref(mana),"source",card(mana.getSourceCard()),"player",seat(mana.getPlayer()),"color",String.valueOf(mana.getColor()));if(o instanceof Map<?,?> map){List<Object> r=new ArrayList<>();for(var e:map.entrySet())r.add(m("key",String.valueOf(e.getKey()),"value",diag(e.getValue())));return r;}if(o instanceof Iterable<?> list){List<Object> r=new ArrayList<>();for(Object x:list)r.add(diag(x));return r;}return m("class",o.getClass().getName(),"ref",ref(o),"text",o.toString());}
        List<Map<String,Object>> stack(){List<Map<String,Object>> r=new ArrayList<>();for(SpellAbilityStackInstance x:game.getStack())r.add(m("id",String.valueOf(x.getId()),"ref",ref(x),"ability",ability(x.getSpellAbility())));return r;}
        Map<String,Object> world(){if(game==null)return m("stage","construction");Map<String,Object> ps=new LinkedHashMap<>();for(var e:seats.entrySet()){Player p=e.getValue();Map<String,Object> zs=new LinkedHashMap<>();for(ZoneType z:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile)){List<Object> cs=new ArrayList<>();for(Card c:p.getCardsIn(z))cs.add(card(c));zs.put(z.name(),cs);}List<Object> mana=new ArrayList<>();for(Mana n:p.getManaPool())mana.add(diag(n));ps.put(e.getKey(),m("life",String.valueOf(p.getLife()),"lost",p.hasLost(),"zones_raw_iteration",zs,"mana_actual_iteration",mana));}return m("stage",game.getAge().name(),"turn",game.getPhaseHandler().getTurn(),"phase",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",seat(game.getPhaseHandler().getPlayerTurn()),"priority",seat(game.getPhaseHandler().getPriorityPlayer()),"cleanup_visit",cleanupVisits,"action_cursor",actionCursor,"choice_used",new ArrayList<>(choicesUsed),"cohort_cursor",cohortCursor,"stack_top_to_bottom",stack(),"players",ps);}
        void strict(Player p){require(p.getController().getClass()==StrictController.class&&!p.getController().isAI()&&!p.getController().isGuiPlayer()&&p.getController().getFullControl().isEmpty()&&!p.getController().canPlayUnlimitedLands()&&!p.getController().isOrderedZone(),"strict_controller",m("actor",seat(p)));}
        void construct()throws Exception{
            require(seed.getAsJsonArray("floating_mana").isEmpty(),"unexpected_seed_mana",m());Set<String> used=new LinkedHashSet<>();for(JsonObject c:objs(seed.getAsJsonArray("cards"))){inventory.put(s(c,"alias"),c);used.add(s(c,"definition"));}
            for(String key:used){JsonObject d=root.getAsJsonObject("definitions").getAsJsonObject(key);PaperCard p;
                if(!d.get("printing").isJsonNull()){JsonObject print=d.getAsJsonObject("printing");p=FModel.getMagicDb().getCommonCards().getCard(s(print,"name"),s(print,"edition"),s(print,"collector_number"));require(p!=null&&p.getName().equals(s(print,"name"))&&p.getEdition().equalsIgnoreCase(s(print,"edition"))&&p.getCollectorNumber().equals(s(print,"collector_number")),"printing_realization",m("definition",key));}
                else{require(d.has("forge_reader_utf8"),"unselected_synthetic",m("definition",key));String text=s(d,"forge_reader_utf8");require(sha(text.getBytes(StandardCharsets.UTF_8)).equals(s(d,"forge_reader_sha256"))&&text.equals(String.join("\n",strs(d.getAsJsonArray("forge_synthetic_card_lines")))+"\n"),"reader_bytes",m("definition",key));p=new PaperCard(new CardRules.Reader().readCard(strs(d.getAsJsonArray("forge_synthetic_card_lines"))),"???",CardRarity.Common);}
                papers.put(key,p);CardRules r=p.getRules();definitions.put(key,m("input_definition",d,"realized_name",p.getName(),"edition",p.getEdition(),"collector_number",p.getCollectorNumber(),"foil",p.isFoil(),"art_index",p.getArtIndex(),"oracle",r.getOracleText(),"mana_cost",r.getManaCost().toString(),"type",r.getType().toString(),"power",r.getPower(),"toughness",r.getToughness()));}
            List<RegisteredPlayer> registered=new ArrayList<>();for(String a:strs(seed.getAsJsonArray("participants"))){Deck d=new Deck("trigger-"+probe+"-"+a);for(JsonObject c:inventory.values())if(s(c,"owner").equals(a))d.getMain().add(papers.get(s(c,"definition")),1);RegisteredPlayer p=new RegisteredPlayer(d).setPlayer(new StrictLobby(a,this));p.setStartingLife(new BigInteger(s(seed.getAsJsonObject("life"),a)).intValueExact());p.setTeamNumber(registered.size()+1);registered.add(p);}
            GameRules rules=new GameRules(GameType.Constructed);rules.setOrderCombatants(false);game=new Game(registered,rules,new Match(rules,registered,"Ordinary triggers "+probe));game.setAge(GameStage.Play);
            for(JsonObject c:inventory.values()){String a=s(c,"alias");Card actual=Card.fromPaperCard(papers.get(s(c,"definition")),seats.get(s(c,"owner")));actual.setGameTimestamp(game.getNextTimestamp());seeded.put(a,actual);aliases.put(actual.getId(),a);}
            for(JsonObject c:inventory.values())if(!s(c,"zone").equals("Library"))seedOne(c);
            for(String owner:seats.keySet())for(String a:strs(seed.getAsJsonObject("library_order").getAsJsonArray(owner)))seedOne(inventory.get(a));
            for(var seedEntry:seeded.entrySet())bindings.put(seedEntry.getKey()+"_seed_object",capture(seedEntry.getValue()));for(Player p:seats.values())strict(p);game.copyLastState();observe("operation","prepared_seed",m("scope","trusted setup; no real entry/tap/discard or pool credit","world",world()));
            for(String a:seeded.keySet())observe("definition","actual_card_factory",m("alias",a,"card",card(current(a)),"abilities",current(a).getNonManaAbilities().stream().map(this::ability).toList(),"mana_abilities",current(a).getManaAbilities().stream().map(this::ability).toList(),"triggers",diag(current(a).getTriggers())));
        }
        void seedOne(JsonObject c){Card x=seeded.get(s(c,"alias"));ZoneType z=ZoneType.valueOf(s(c,"zone"));Player owner=seats.get(s(c,"owner"));if(z==ZoneType.Battlefield){Player controller=seats.get(s(c,"controller"));if(controller!=owner)x.setController(controller,game.getNextTimestamp());controller.getZone(z).add(x);x.setTapped(c.get("tapped").getAsBoolean());}else owner.getZone(z).add(x);}
        void guard(JsonObject a,Player p){JsonObject g=a.getAsJsonObject("guard");require(g!=null&&game.getPhaseHandler().getTurn()==g.get("turn").getAsInt()&&game.getPhaseHandler().getPhase().name().equals(s(g,"forge_phase"))&&seat(p).equals(s(a,"actor")),"priority_guard",m("action",a));}
        boolean allConsumed(){return actionCursor==actions.size()&&choicesUsed.size()==choices.size()&&cohortCursor==cohorts.size()&&selectorsUsed.size()==selectors.size()&&assignmentUsed.size()==(recipe.has("combat_assignment_inputs")?recipe.getAsJsonArray("combat_assignment_inputs").size():0)&&notified.size()==(actions.stream().anyMatch(a->a.has("synchronous_choice"))?seats.size()-1:0);}
        List<SpellAbility> choose(Player p){return call(p,"chooseSpellAbilityToPlay",m(),()->{require(game.getPhaseHandler().getPriorityPlayer()==p,"priority_actor",m());if(actionCursor==actions.size()){JsonObject st=recipe.getAsJsonObject("stop");require(allConsumed()&&seat(p).equals(s(st,"actor"))&&game.getPhaseHandler().getPhase().name().equals(s(st,"phase"))&&game.getPhaseHandler().getTurn()==st.get("turn").getAsInt()&&(st.get("cleanup_visit_ordinal").isJsonNull()||cleanupVisits==st.get("cleanup_visit_ordinal").getAsInt())&&contexts.isEmpty(),"stop_guard",m("stop",st));healthy();throw stop;}
            JsonObject a=actions.get(actionCursor);guard(a,p);if(s(a,"op").equals("pass")){actionCursor++;return null;}require(selected==null,"unfinished_action",m());String src=s(a.getAsJsonObject("source"),a.getAsJsonObject("source").has("current_owned_action_alias")?"current_owned_action_alias":"captured_object");Card c=a.getAsJsonObject("source").has("current_owned_action_alias")?current(src):bound(a.getAsJsonObject("source"));List<SpellAbility> domain=new ArrayList<>();switch(s(a,"op")){case "cast"-> {require(c.isInZone(ZoneType.Hand)&&c.getOwner()==p,"cast_source",m());domain.add(c.getFirstSpellAbility());}case "activate_mana"->{require(c.isInPlay()&&c.getController()==p,"mana_source",m());domain.addAll(c.getManaAbilities());}case "activate_ability"->{require(c.isInPlay()&&c.getController()==p,"activation_source",m());for(SpellAbility sa:c.getNonManaAbilities())if(sa.isActivatedAbility())domain.add(sa);}default->throw bad("unknown_action",m("action",a));}observe("choice","ordinary_ability_domain",m("input",a,"domain",domain.stream().map(this::ability).toList()));require(domain.size()==1&&domain.get(0)!=null,"ordinary_ability_domain",m("size",domain.size()));selected=domain.get(0);selectedAction=a;selected.setActivatingPlayer(p);require(selected.canPlay(),"can_play",m("ability",ability(selected)));return List.of(selected);});}
        boolean play(Player p,SpellAbility sa){return call(p,"playChosenSpellAbility",m("ability",ability(sa)),()->{require(sa==selected&&selectedAction==actions.get(actionCursor),"selected_action",m());JsonObject a=selectedAction;Set<SpellAbilityStackInstance> before=Collections.newSetFromMap(new IdentityHashMap<>());for(SpellAbilityStackInstance item:game.getStack())before.add(item);contexts.push(sa);try{require(PlaySpellAbility.playSpellAbility(p.getController(),p,sa),"play_failed",m());}finally{contexts.pop();}if(a.has("bind_entry")){SpellAbilityStackInstance si=uniqueEntry(sa,before);Admission admitted=registerEntry(s(a,"bind_entry"),si);observe("association","ordinary_entry",m("input_alias",s(a,"bind_entry"),"id",String.valueOf(si.getId()),"immutable_admission_source",captureProof(admitted.source),"selected_ability",ability(sa),"actual_instance_ability",ability(si.getSpellAbility()),"preclear_selected_targets",preclearTargets.get(sa),"relation",si.getSpellAbility()==sa?"same_ability":"actual_activated_ability_copy_with_original_link"));}if(sa.isManaAbility())observe("operation","actual_mana_production",m("source",card(sa.getHostCard()),"ability",ability(sa),"last_mana",diag(sa.getManaPart().getLastManaProduced()),"world",world()));actionCursor++;selected=null;selectedAction=null;return true;});}
        SpellAbilityStackInstance uniqueEntry(SpellAbility sa){return uniqueEntry(sa,Set.of());}
        SpellAbilityStackInstance uniqueEntry(SpellAbility sa,Set<SpellAbilityStackInstance> before){List<SpellAbilityStackInstance> found=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack()){SpellAbility actual=si.getSpellAbility();boolean exact=actual==sa&&actual.getId()==sa.getId();boolean activationCopy=sa.isActivatedAbility()&&!sa.isManaAbility()&&!sa.isTrigger()&&actual!=sa&&actual.getOriginalAbility()==sa&&sameObject(actual.getHostCard(),sa.getHostCard())&&actual.getActivatingPlayer()==sa.getActivatingPlayer()&&actual.getApi()==sa.getApi()&&actual.getMapParams().equals(sa.getMapParams());if(!before.contains(si)&&(exact||activationCopy))found.add(si);}require(found.size()==1&&game.getStack().getInstanceMatchingSpellAbilityID(found.get(0).getSpellAbility())==found.get(0),"actual_entry_join",m("ability",ability(sa),"matches",found.size()));SpellAbility actual=found.get(0).getSpellAbility();if(actual!=sa){require(selectedAction!=null&&selectedAction.has("target")&&selectedAction.getAsJsonObject("target").has("captured_object"),"activation_copy_target_recipe",m());Card target=bound(selectedAction.getAsJsonObject("target"));require(preclearTargets.containsKey(sa)&&actual.getTargets().getTargetCards().size()==1&&sameObject(actual.getTargets().getTargetCards().get(0),target)&&actual.isTargetNumberValid(),"activation_copy_target_join",m());}return found.get(0);}
        SpellAbility select(Player p,Card host,List<SpellAbility> options,ITriggerEvent event){return call(p,"getAbilityToPlay",m("host",card(host),"complete_offered",options.stream().map(this::ability).toList(),"event",diag(event)),()->{require(event==null&&!contexts.isEmpty()&&options.size()==1&&options.get(0)==contexts.peek()&&host.getId()==contexts.peek().getHostCard().getId(),"getAbilityToPlay_domain",m());return options.get(0);});}
        CostDecisionMakerBase costs(Player p,Player payer,SpellAbility sa,boolean effect,String prompt){return call(p,"getCostDecisionMaker",m("payer",seat(payer),"ability",ability(sa),"effect",effect,"prompt",prompt),()->{require(p==payer&&!contexts.isEmpty()&&contexts.peek()==sa&&effect==sa.isTrigger(),"cost_context",m());return new StrictCosts(p,sa,this,effect);});}
        PaymentDecision cost(Player p,SpellAbility sa,boolean effect,CostPart part){return call(p,"visit_"+part.getClass().getSimpleName(),m("ability",ability(sa),"effect",effect,"part",diag(part),"cost_stack",diag(game.costPaymentStack.peek())),()->{require(!contexts.isEmpty()&&contexts.peek()==sa&&effect==sa.isTrigger()&&game.costPaymentStack.peek()!=null&&game.costPaymentStack.peek().cost()==part&&game.costPaymentStack.peek().payment().getAbility()==sa,"cost_part_context",m());if(part instanceof CostPartMana)return PaymentDecision.number(0);if(part instanceof CostTap tap){require(sa.isManaAbility()&&tap.canPay(sa,p,effect),"tap_cost",m());return PaymentDecision.number(0);}if(part instanceof CostSacrifice sac){require(sa==selected&&s(selectedAction,"op").equals("activate_ability")&&sac.payCostFromSource()&&sac.getAmount().equals("1")&&sac.canPay(sa,p,effect),"sacrifice_cost",m());return PaymentDecision.card(sa.getHostCard());}throw bad("unselected_cost",m("class",part.getClass().getName()));});}
        List<CostPart> orderCosts(Player p,List<CostPart> parts){return call(p,"orderCosts",m("complete_offered",diag(parts)),()->{require(!contexts.isEmpty(),"cost_order_context",m());for(CostPart part:parts)require(part instanceof CostPartMana||part instanceof CostTap||part instanceof CostSacrifice,"unselected_cost_part",m());return new ArrayList<>(parts);});}
        boolean pay(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){return call(p,"payManaCost",m("cost",cost.toString(),"part",diag(part),"ability",ability(sa),"effect",effect,"matrix",ref(matrix)),()->{require(!contexts.isEmpty()&&contexts.peek()==sa&&effect==sa.isTrigger(),"pay_context",m());return PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,effect);});}
        boolean apply(Player p,ManaCostBeingPaid cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){return call(p,"applyManaToCost",m("cost",cost.toString(),"ability",ability(sa),"effect",effect,"pool",diag(p.getManaPool())),()->{require(sa==selected&&selectedAction!=null&&!effect&&matrix==game.costPaymentStack.peek().payment(),"payment_context",m());List<Object> spent=new ArrayList<>();p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);try{for(JsonObject spend:objs(selectedAction.getAsJsonArray("payment"))){byte color=ManaAtom.fromName(s(spend,"type").equals("Green")?"G":s(spend,"type").equals("Blue")?"U":s(spend,"type"));int n=new BigInteger(s(spend,"amount")).intValueExact();for(int i=0;i<n;i++){Mana chosen=null;for(Mana mana:p.getManaPool())if(mana.getColor()==color){chosen=mana;break;}require(chosen!=null,"missing_actual_mana",m("spend",spend));require(chosen.meetsManaRestrictions(sa)&&sa.allowsPayingWithShard(chosen.getSourceCard(),chosen.getColor())&&p.getManaPool().tryPayCostWithMana(sa,cost,chosen,false),"actual_mana_payment",m("mana",diag(chosen)));sa.getPayingMana().add(chosen);spent.add(diag(chosen));}}require(cost.isPaid(),"incomplete_payment",m());}finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}observe("operation","actual_payment",m("spent",spent,"paying_mana",diag(sa.getPayingMana()),"pool_after",diag(p.getManaPool())));return true;});}
        boolean targets(Player p,SpellAbility sa){return call(p,"chooseTargetsFor",m("ability",ability(sa)),()->{require(sa==selected&&selectedAction.has("target")&&!selectedAction.get("target").isJsonNull(),"target_context",m());JsonObject target=selectedAction.getAsJsonObject("target");List<Object> domain=new ArrayList<>();if(target.has("entry_ref")){for(SpellAbilityStackInstance si:game.getStack())if(si.isSpell()&&sa.canTargetSpellAbility(si.getSpellAbility()))domain.add(m("entry_id",String.valueOf(si.getId()),"ability",ability(si.getSpellAbility())));SpellAbilityStackInstance wanted=entries.get(s(target,"entry_ref"));require(wanted!=null&&sa.canTargetSpellAbility(wanted.getSpellAbility())&&game.getStack().getInstanceMatchingSpellAbilityID(wanted.getSpellAbility())==wanted,"spell_target",m());sa.getTargets().add(wanted.getSpellAbility());}else{for(Card c:game.getCardsIn(ZoneType.Battlefield))if(c.isCreature()&&sa.canTarget(c))domain.add(card(c));Card wanted=bound(target);require(sa.canTarget(wanted)&&sameObject(wanted,current(alias(wanted))),"creature_target",m());sa.getTargets().add(wanted);}require(sa.isTargetNumberValid(),"target_count",m());preclearTargets.put(sa,JSON.toJsonTree(diag(sa.getTargets())));observe("choice","target_domain_and_selection",m("complete_offered",domain,"input",target,"actual_targets",diag(sa.getTargets())));return true;});}
        byte color(Player p,String message,SpellAbility sa,ColorSet colors){return call(p,"chooseColor",m("ability",ability(sa),"offered_mask",colors.getColor(),"offered_text",colors.toString(),"message",message),()->{require(sa==selected&&selectedAction.has("synchronous_choice"),"color_context",m());JsonObject choice=selectedAction.getAsJsonObject("synchronous_choice");byte mask=0;for(String c:strs(choice.getAsJsonArray("allowed_color_set")))mask|=ManaAtom.fromName(c.equals("Green")?"G":"U");require(colors.getColor()==mask&&s(choice,"return").equals("Blue"),"color_domain",m());return (byte)ManaAtom.BLUE;});}
        void notifyValue(Player p,SpellAbility sa,GameObject related,String value){call(p,"notifyOfValue",m("ability",ability(sa),"related",diag(related),"value",value,"classification","read-only source notification, no choice"),()->{require(sa==selected&&selectedAction!=null&&selectedAction.has("synchronous_choice")&&s(selectedAction,"op").equals("activate_mana")&&sa.isManaAbility()&&sa.getActivatingPlayer()==related&&contexts.peek()==sa&&related==seats.get(s(selectedAction,"actor"))&&p!=related&&notified.add(p),"mana_notification_context",m());byte chosen=ManaAtom.fromName(s(selectedAction.getAsJsonObject("synchronous_choice"),"return").equals("Blue")?"U":"G");require(value.equals(MagicColor.toSymbol(chosen)),"mana_notification_value",m());List<String> path=StackWalker.getInstance().walk(z->z.map(f->f.getClassName()+"."+f.getMethodName()).toList());require(path.contains("forge.game.GameAction.notifyOfValue")&&path.contains("forge.game.ability.effects.ManaEffect.resolve"),"mana_notification_source_path",m());return null;});}
        JsonObject choice(Player p,String kind){for(int i=0;i<choices.size();i++){JsonObject c=choices.get(i);if(!choicesUsed.contains(i)&&s(c,"input_kind").equals(kind)&&s(c,"actor").equals(seat(p))){choicesUsed.add(i);return c;}}throw bad("unlisted_choice",m("kind",kind,"actor",seat(p)));}
        CardCollection cards(JsonArray ids){CardCollection out=new CardCollection();for(JsonObject id:objs(ids))out.add(bound(id));return out;}
        void attackers(Player p,Player actor,Combat combat){call(p,"declareAttackers",m("actor",seat(actor),"combat",ref(combat)),()->{require(p==actor,"attacker_actor",m());JsonObject c=choice(p,"DeclareAttackers");List<Object> domain=new ArrayList<>();for(Card x:p.getCreaturesInPlay())for(GameEntity d:combat.getDefenders())if(CombatUtil.canAttack(x,d))domain.add(m("attacker",card(x),"defender",diag(d)));observe("choice","attack_domain",m("complete_offered",domain,"input",c));for(JsonObject pair:objs(c.getAsJsonArray("items"))){Card attacker=bound(pair.getAsJsonObject("attacker"));Player defender=seats.get(s(pair,"defender"));require(CombatUtil.canAttack(attacker,defender),"illegal_attacker",m());combat.addAttacker(attacker,defender);}require(CombatUtil.validateAttackers(combat),"invalid_attackers",m());return null;});}
        void blockers(Player p,Player defender,Combat combat){call(p,"declareBlockers",m("defender",seat(defender),"combat",ref(combat)),()->{require(p==defender,"blocker_actor",m());JsonObject c=choice(p,"DeclareBlockers");List<Object> domain=new ArrayList<>();for(Card b:p.getCreaturesInPlay())for(Card a:combat.getAttackers())if(CombatUtil.canBlock(a,b,combat))domain.add(m("blocker",card(b),"attacker",card(a)));observe("choice","block_domain",m("complete_offered",domain,"input",c));for(JsonObject pair:objs(c.getAsJsonArray("items"))){Card b=bound(pair.getAsJsonObject("blocker")),a=bound(pair.getAsJsonObject("attacker"));require(CombatUtil.canBlock(a,b,combat),"illegal_blocker",m());combat.addBlocker(a,b);}require(CombatUtil.validateBlocks(combat,defender)==null,"invalid_blockers",m());return null;});}
        Map<Card,Integer> assign(Player p,Card source,CardCollectionView recipients,CardCollectionView remaining,int amount,GameEntity defender,boolean override){return call(p,"assignCombatDamage",m("source",card(source),"complete_recipients",diag(recipients),"remaining",diag(remaining),"amount",String.valueOf(amount),"defender",diag(defender),"overrideOrder",override),()->{List<JsonObject> rows=recipe.has("combat_assignment_inputs")?objs(recipe.getAsJsonArray("combat_assignment_inputs")):List.of();for(int i=0;i<rows.size();i++){JsonObject r=rows.get(i);if(!assignmentUsed.contains(i)&&s(r,"source_alias").equals(alias(source))){require(s(r,"actor").equals(seat(p))&&new BigInteger(s(r,"amount")).intValueExact()==amount&&r.get("overrideOrder").getAsBoolean()==override,"assignment_context",m());Set<String> offered=new LinkedHashSet<>();for(Card c:recipients)offered.add(alias(c));require(offered.equals(new LinkedHashSet<>(strs(r.getAsJsonArray("recipients_membership"))))&&(r.get("defender").isJsonNull()?defender==null:defender==seats.get(s(r,"defender"))),"assignment_domain",m());Map<Card,Integer> out=new LinkedHashMap<>();int sum=0;for(JsonObject part:objs(r.getAsJsonArray("reply"))){Card target=current(s(part,"card_alias"));int n=new BigInteger(s(part,"amount")).intValueExact();require(recipients.contains(target)&&out.put(target,n)==null,"assignment_reply",m());sum+=n;}require(sum==amount,"assignment_total",m());assignmentUsed.add(i);return out;}}throw bad("unlisted_assignment",m("source",alias(source)));});}
        CardCollectionView order(Player p,CardCollectionView offered,ZoneType dest,SpellAbility cause){return call(p,"orderMoveToZoneList",m("complete_offered",diag(offered),"destination",dest.name(),"cause",diag(cause)),()->{JsonObject c=choice(p,"OrderGraveyard");require(dest==ZoneType.Graveyard&&cause==null&&offered.size()>1,"graveyard_context",m());CardCollection result=cards(c.getAsJsonArray("forge_raw_move_order"));require(exactCards(offered,result),"graveyard_permutation",m());return result;});}
        boolean exactCards(CardCollectionView offered,CardCollectionView selected){if(offered.size()!=selected.size())return false;Set<String> a=new LinkedHashSet<>(),b=new LinkedHashSet<>();for(Card c:offered)a.add(c.getId()+":"+c.getGameTimestamp());for(Card c:selected)b.add(c.getId()+":"+c.getGameTimestamp());return a.size()==offered.size()&&b.size()==selected.size()&&a.equals(b);}
        CardCollectionView discard(Player p,int n){return call(p,"chooseCardsToDiscardToMaximumHandSize",m("complete_hand",diag(p.getCardsIn(ZoneType.Hand)),"required",n),()->{JsonObject c=choice(p,"Discard");CardCollection out=cards(c.getAsJsonArray("items"));require(game.getPhaseHandler().getPhase()==PhaseType.CLEANUP&&out.size()==n,"discard_count",m());for(Card x:out)require(x.isInZone(ZoneType.Hand)&&x.getOwner()==p,"discard_domain",m());return out;});}
        Map<String,String> params(String text){Map<String,String> r=new LinkedHashMap<>();if(text.startsWith("T:"))text=text.substring(2);for(String part:text.split("\\|")){int i=part.indexOf('$');require(i>=0,"trigger_input_record",m());r.put(part.substring(0,i).trim(),part.substring(i+1).trim());}return r;}
        boolean selector(JsonObject sel,WrappedAbility w){if(!s(sel,"source_alias").equals(alias(w.getHostCard())))return false;JsonObject d=root.getAsJsonObject("definitions").getAsJsonObject(s(sel,"definition")).getAsJsonObject("forge_selected_trigger");if(!w.getTrigger().getMapParams().equals(params(s(d,"record"))))return false;JsonObject source=sel.getAsJsonObject("source_incarnation");CapturedObject old=source.has("captured_seed_object")?bindings.get(s(source,"captured_seed_object")):permanentEntries.containsKey(s(source,"actual_permanent_entry_receipt"))?permanentEntries.get(s(source,"actual_permanent_entry_receipt")).publication.after:null;if(old==null||!old.sameIncarnation(w.getHostCard()))return false;JsonObject ctx=sel.getAsJsonObject("triggering_context");if(s(ctx,"kind").equals("Upkeep")&&game.getPhaseHandler().getPhase()!=PhaseType.UPKEEP)return false;if(s(ctx,"kind").equals("Cleanup")&&game.getPhaseHandler().getPhase()!=PhaseType.CLEANUP)return false;if(ctx.get("turn").getAsInt()!=game.getPhaseHandler().getTurn())return false;if(ctx.has("active")&&!s(ctx,"active").equals(seat(game.getPhaseHandler().getPlayerTurn())))return false;if(ctx.has("cleanup_visit_ordinal")&&ctx.get("cleanup_visit_ordinal").getAsInt()!=cleanupVisits)return false;if(ctx.has("related_object")){Card related=bound(ctx.getAsJsonObject("related_object"));Object actual=w.getTriggeringObject(AbilityKey.Card);if(!(actual instanceof Card c)||!sameObject(related,c))return false;}return true;}
        void triggers(Player p,List<SpellAbility> offered){call(p,"orderAndPlaySimultaneousSa",m("complete_offered",offered.stream().map(this::ability).toList(),"stack_before",stack()),()->{require(cohortCursor<cohorts.size()&&!offered.isEmpty(),"unlisted_trigger_cohort",m());JsonObject cohort=cohorts.get(cohortCursor);require(s(cohort,"actor").equals(seat(p)),"trigger_owner_order",m());Map<String,WrappedAbility> matches=new LinkedHashMap<>();for(SpellAbility sa:offered){require(sa instanceof WrappedAbility&&!sa.isOptionalTrigger()&&!sa.usesTargeting()&&sa.getActivatingPlayer()==p&&sa.getTrigger().getMode()!=TriggerType.AbilityTriggered&&!sa.getTrigger().isStatic(),"ordinary_wrapper_domain",m("ability",ability(sa)));WrappedAbility w=(WrappedAbility)sa;List<JsonObject> found=new ArrayList<>();for(JsonObject sel:selectors)if(!selectorsUsed.contains(s(sel,"input_token"))&&selector(sel,w))found.add(sel);require(found.size()==1,"trigger_selector_join",m("ability",ability(sa),"matches",found.size()));String token=s(found.get(0),"input_token");require(matches.put(token,w)==null,"duplicate_trigger_token",m());}
            List<String> order=strs(cohort.getAsJsonArray("bottom_to_top"));require(order.size()==offered.size()&&new LinkedHashSet<>(order).equals(matches.keySet()),"trigger_permutation",m());if(order.size()>1){JsonObject input=choice(p,"OrderTriggers");require(order.equals(strs(input.getAsJsonArray("items"))),"native_reference_trigger_reply_projection",m());}for(String token:order){WrappedAbility w=matches.get(token);require(w.getWrappedAbility().getApi()==ApiType.Draw&&!w.getWrappedAbility().usesTargeting()&&w.getWrappedAbility().getSubAbility()==null,"mandatory_draw_body",m());List<Map<String,Object>> before=stack();contexts.push(w);try{require(PlaySpellAbility.playSpellAbility(p.getController(),p,w),"trigger_admission",m());}finally{contexts.pop();}SpellAbilityStackInstance si=uniqueEntry(w);placed.add(new Placed(w,si.getId(),token));selectorsUsed.add(token);observe("association","trigger_entry",m("input_token",token,"wrapper",ability(w),"body",ability(w.getWrappedAbility()),"entry_id",String.valueOf(si.getId()),"captured_controller",seat(p),"stack_before",before,"stack_after",stack()));}cohortCursor++;return null;});}
        void noStack(Player p,SpellAbility body,boolean setup){call(p,"playSpellAbilityNoStack",m("body",ability(body),"canSetupTargets",setup,"isResolving",game.getStack().isResolving()),()->{List<Placed> matches=new ArrayList<>();for(Placed x:placed)if(!x.resolved&&x.body==body&&x.wrapper.getWrappedAbility()==body&&x.controller==p)matches.add(x);require(matches.size()==1&&!setup&&game.getStack().isResolving()&&body.getApi()==ApiType.Draw&&!body.usesTargeting(),"mandatory_body_join",m());Placed x=matches.get(0);observe("association","body_entry",m("input_token",x.token,"entry_id",String.valueOf(x.entry),"wrapper",ability(x.wrapper),"body",ability(body),"world",world()));contexts.push(body);try{require(PlaySpellAbility.playSpellAbilityNoStack(p.getController(),p,body,true),"mandatory_body_delegate",m());}finally{contexts.pop();}x.resolved=true;observe("association","body_return",m("input_token",x.token,"entry_id",String.valueOf(x.entry),"world",world()));return null;});}
        Object playerView(PlayerView p){if(p==null)return null;for(var e:seats.entrySet())if(e.getValue().getId()==p.getId())return m("seat",e.getKey(),"id",String.valueOf(p.getId()),"ref",ref(p));throw bad("unknown_player_view",m("id",p.getId()));}
        Object view(CardView c){if(c==null)return null;String a=aliases.get(c.getId());require(a!=null,"unknown_card_view",m("id",c.getId()));return m("alias",a,"id",String.valueOf(c.getId()),"ref",ref(c),"owner",playerView(c.getOwner()),"controller",playerView(c.getController()),"zone",c.getZone()==null?null:c.getZone().name());}
        Object view(SpellAbilityView a){return a==null?null:m("id",String.valueOf(a.getId()),"ref",ref(a),"host",view(a.getHostCard()),"spell",a.isSpell(),"description",a.getDescription());}
        Object view(StackItemView a){return a==null?null:m("id",String.valueOf(a.getId()),"ref",ref(a),"source",view(a.getSourceCard()),"activator",playerView(a.getActivatingPlayer()),"target_cards",a.getTargetCards()==null?null:a.getTargetCards().stream().map(this::view).toList(),"target_players",a.getTargetPlayers()==null?null:a.getTargetPlayers().stream().map(this::playerView).toList(),"target_stack_ids",a.getTargetStackItems()==null?null:a.getTargetStackItems().stream().map(x->String.valueOf(x.getId())).toList());}
        Object zoneView(ZoneView z){return z==null?null:m("player",playerView(z.player()),"zoneType",z.zoneType().name());}
        Object entityView(GameEntityView e){if(e instanceof PlayerView p)return playerView(p);if(e instanceof CardView c)return view(c);throw bad("unknown_event_entity",m("class",e.getClass().getName()));}
        Object eventPayload(GameEvent event){
            if(event instanceof GameEventTurnBegan e)return m("turnOwner",playerView(e.turnOwner()),"turnNumber",e.turnNumber());
            if(event instanceof GameEventTurnPhase e)return m("playerTurn",playerView(e.playerTurn()),"phase",e.phase().name(),"phaseDesc",e.phaseDesc());
            if(event instanceof GameEventPlayerPriority e)return m("turn",playerView(e.turn()),"phase",e.phase().name(),"priority",playerView(e.priority()));
            if(event instanceof GameEventZone e)return m("zoneType",e.zoneType().name(),"player",playerView(e.player()),"mode",e.mode().name(),"card",view(e.card()),"sa",view(e.sa()));
            if(event instanceof GameEventCardChangeZone e)return m("card",view(e.card()),"from",zoneView(e.from()),"to",zoneView(e.to()));
            if(event instanceof GameEventCardTapped e)return m("card",view(e.card()),"tapped",e.tapped());
            if(event instanceof GameEventManaPool e)return m("player",playerView(e.player()),"mode",e.mode().name(),"colors_actual_iteration",e.colors()==null?null:e.colors().stream().map(x->x.getShortName()).toList());
            if(event instanceof GameEventSpellAbilityCast e)return m("sa",view(e.sa()),"si",view(e.si()),"stackIndex",e.stackIndex(),"targetDescription",e.targetDescription());
            if(event instanceof GameEventSpellResolved e)return m("spell",view(e.spell()),"hasFizzled",e.hasFizzled(),"stackDescription",e.stackDescription());
            if(event instanceof GameEventSpellRemovedFromStack e)return m("sa",view(e.sa()));
            if(event instanceof GameEventAttackersDeclared e){List<Object> out=new ArrayList<>();for(var row:e.attackersMap().entries())out.add(m("defender",entityView(row.getKey()),"attacker",view(row.getValue())));return m("player",playerView(e.player()),"attackersMap_actual_iteration",out);}
            if(event instanceof GameEventBlockersDeclared e){List<Object> out=new ArrayList<>();for(var outer:e.blockers().entrySet())for(var row:outer.getValue().entries())out.add(m("defendingEntity",entityView(outer.getKey()),"attacker",view(row.getKey()),"value",view(row.getValue())));return m("defendingPlayer",playerView(e.defendingPlayer()),"blockers_actual_iteration",out);}
            if(event instanceof GameEventCombatEnded e)return m("attackers",e.attackers()==null?null:e.attackers().stream().map(this::view).toList(),"blockers",e.blockers()==null?null:e.blockers().stream().map(this::view).toList());
            if(event instanceof GameEventAddLog e)return m("type",e.type().name(),"message",e.message(),"sourceCard",view(e.sourceCard()));
            if(event instanceof GameEventCardSacrificed e)return m("card",view(e.card()));
            if(event instanceof GameEventCardDamaged e)return m("card",view(e.card()),"source",view(e.source()),"amount",String.valueOf(e.amount()),"type",e.type().name());
            if(event instanceof GameEventCombatChanged||event instanceof GameEventCardDestroyed||event instanceof GameEventTurnEnded)return m("record_components",List.of());
            throw bad("unprojected_publication_record",m("class",event.getClass().getName(),"raw",event.toString()));
        }
        CapturedObject capture(Card object) {
            return new CapturedObject(object,JSON.toJsonTree(card(object)));
        }
        Map<String,Object> captureProof(CapturedObject object) {
            return m("reference",ref(object.reference),"id",String.valueOf(object.id),
                    "timestamp",String.valueOf(object.timestamp),"snapshot",object.snapshot);
        }
        Admission registerEntry(String alias,SpellAbilityStackInstance instance) {
            require(!entries.containsKey(alias)&&!admissions.containsKey(alias)
                    &&!admissionsByInstance.containsKey(instance),"duplicate_admission_alias_or_instance",m("alias",alias));
            Admission admission=new Admission(alias,instance,capture(instance.getSourceCard()));
            entries.put(alias,instance);admissions.put(alias,admission);admissionsByInstance.put(instance,admission);
            return admission;
        }
        /** The production observer and local controls use this same mutation-free validator. */
        static String permanentAssociationRejection(Admission admission,PermanentPublication publication,boolean receiptExists) {
            if(admission==null)return "missing_exact_admission";
            if(admission.instance!=publication.top||admission.ability!=publication.ability)
                return "not_current_resolving_entry";
            if(receiptExists)return "receipt_already_recorded";
            if(!publication.resolving||!publication.sourceResolving||publication.liveIdentityCount!=1)
                return "entry_not_unique_live_resolution";
            if(!admission.permanent||!publication.stackToBattlefield||!publication.afterInBattlefield)
                return "not_selected_permanent_completion";
            if(publication.hostAtPublication==null||publication.after==null
                    ||publication.hostAtPublication.reference!=admission.source.reference
                    ||publication.after.reference!=admission.source.reference)
                return "admitted_host_reference_mismatch";
            if(admission.source.id!=publication.publicationCardId||admission.source.id!=publication.after.id
                    ||admission.source.id!=publication.hostAtPublication.id)
                return "physical_source_mismatch";
            if(publication.controller!=admission.controller||publication.afterController!=admission.controller)
                return "controller_mismatch";
            if(publication.after.timestamp==admission.source.timestamp
                    ||publication.hostAtPublication.timestamp!=publication.after.timestamp)
                return "unexpected_incarnation_transition";
            return null;
        }
        void associatePermanentPublication(GameEventCardChangeZone event,Card after) {
            SpellAbilityStackInstance top=game.getStack().peek();
            int liveCount=0;
            for(SpellAbilityStackInstance instance:game.getStack())if(instance==top)liveCount++;
            Card host=top==null?null:top.getSourceCard();
            PermanentPublication publication=new PermanentPublication(top,host==null?null:capture(host),capture(after),
                    top==null?null:top.getSpellAbility().getActivatingPlayer(),after.getController(),
                    game.getStack().isResolving(),host!=null&&game.getStack().isResolving(host),
                    event.from()!=null&&event.from().zoneType()==ZoneType.Stack
                        &&event.to()!=null&&event.to().zoneType()==ZoneType.Battlefield,
                    after.isInPlay(),liveCount,event.card().getId());
            Admission admission=admissionsByInstance.get(top);
            String rejection=permanentAssociationRejection(admission,publication,
                    admission!=null&&permanentEntries.containsKey(admission.alias));
            require(rejection==null,"permanent_publication_association_rejected",
                    m("reason",rejection,"actual_top_id",top==null?null:String.valueOf(top.getId()),
                        "actual_after",captureProof(publication.after)));
            require(game.getStack().getInstanceMatchingSpellAbilityID(publication.ability)==top,
                    "permanent_live_instance_lookup",m());
            List<String> bindingNames=new ArrayList<>();
            for(JsonObject binding:objs(fixture.getAsJsonArray("bindings"))) {
                JsonObject join=binding.getAsJsonObject("join");
                if(join.has("actual_permanent_entry_of")&&s(join,"actual_permanent_entry_of").equals(admission.alias)) {
                    require(s(join,"field").equals("after.id")&&join.get("require_unique").getAsBoolean(),
                            "unselected_permanent_binding_shape",m("binding",binding));
                    String name=s(binding,"name");
                    require(!bindings.containsKey(name)&&!bindingNames.contains(name),
                            "duplicate_captured_permanent_binding",m("binding",name));
                    bindingNames.add(name);
                }
            }
            PermanentReceipt receipt=new PermanentReceipt(admission,publication);
            require(permanentEntries.putIfAbsent(admission.alias,receipt)==null,
                    "duplicate_permanent_receipt",m("entry_alias",admission.alias));
            for(String name:bindingNames) {
                require(bindings.putIfAbsent(name,publication.after)==null,
                        "duplicate_captured_permanent_binding",m("binding",name));
            }
            observe("association","actual_permanent_entry_publication",m("entry_alias",admission.alias,
                    "entry_id",String.valueOf(top.getId()),"actual_after",publication.after.snapshot,
                    "immutable_admission_source",captureProof(admission.source),
                    "immutable_after",captureProof(publication.after),
                    "host_at_publication",captureProof(publication.hostAtPublication),
                    "actual_top_instance",ref(top),"actual_ability",ref(publication.ability),
                    "from",zoneView(event.from()),"to",zoneView(event.to()),
                    "source_reference_matches_admission",true,"unique_live_instance_count",liveCount,
                    "public_isResolving",publication.resolving,"public_isResolving_host",publication.sourceResolving,
                    "captured_bindings_written",bindingNames));
        }
        Map<String,Object> permanentReceiptProofs() {
            Map<String,Object> proofs=new LinkedHashMap<>();
            for(var entry:permanentEntries.entrySet()) {
                PermanentReceipt receipt=entry.getValue();
                proofs.put(entry.getKey(),m("entry_id",String.valueOf(receipt.admission.instance.getId()),
                        "admitted_ability_reference",ref(receipt.admission.ability),
                        "admitted_source",captureProof(receipt.admission.source),
                        "after",captureProof(receipt.publication.after),"controller",seat(receipt.admission.controller)));
            }
            return proofs;
        }
        Map<String,Object> bindingProofs() {
            Map<String,Object> proofs=new LinkedHashMap<>();
            for(var entry:bindings.entrySet())proofs.put(entry.getKey(),captureProof(entry.getValue()));
            return proofs;
        }
        /** Replay the actual validator on actual captured contexts; no new game/action/event. */
        void localPermanentAssociationControls() throws IOException {
            PermanentReceipt earlier=null,later=null;
            for(PermanentReceipt candidate:permanentEntries.values()) {
                for(PermanentReceipt historical:permanentEntries.values()) {
                    if(candidate!=historical&&candidate.admission.source.id==historical.admission.source.id
                            &&candidate.publication.after.timestamp>historical.publication.after.timestamp) {
                        require(later==null,"ambiguous_local_recast_control",m());
                        earlier=historical;later=candidate;
                    }
                }
            }
            if(later==null)return;
            String before=JSON.toJson(m("receipts",permanentReceiptProofs(),"bindings",bindingProofs(),"world",world()));
            String history=permanentAssociationRejection(earlier.admission,later.publication,
                    permanentEntries.containsKey(earlier.admission.alias));
            String duplicate=permanentAssociationRejection(later.admission,later.publication,
                    permanentEntries.containsKey(later.admission.alias));
            require("not_current_resolving_entry".equals(history),"historical_entry_control_not_rejected",m("reason",history));
            require("receipt_already_recorded".equals(duplicate),"duplicate_receipt_control_not_rejected",m("reason",duplicate));
            String after=JSON.toJson(m("receipts",permanentReceiptProofs(),"bindings",bindingProofs(),"world",world()));
            require(before.equals(after),"local_association_control_mutated_source",m());
            Files.writeString(Path.of(System.getProperty("spellwake.evidence.dir"),probe+".producer-controls.json"),
                    JSON.toJson(m("kind","non-game-producer-association-controls","probe",probe,
                        "additional_games_started",0,"engine_action_or_publication_performed",false,
                        "validator","same permanentAssociationRejection used by actual observer",
                        "historical_same_physical_card",m("old_entry",earlier.admission.alias,"old_id",String.valueOf(earlier.admission.instance.getId()),
                            "current_entry",later.admission.alias,"current_id",String.valueOf(later.admission.instance.getId()),"rejection",history),
                        "duplicate_receipt",m("entry",later.admission.alias,"rejection",duplicate),
                        "source_and_receipts_unchanged",before.equals(after),"immutable_receipts",permanentReceiptProofs(),
                        "immutable_bindings",bindingProofs()))+"\n",StandardOpenOption.CREATE_NEW);
        }
        @Subscribe public void event(GameEvent event){if(sticky!=null)return;try{if(event instanceof GameEventTurnPhase e&&e.phase()==PhaseType.CLEANUP)cleanupVisits++;if(event instanceof GameEventCardChangeZone e){String a=aliases.get(e.card().getId());require(a!=null,"unregistered_zone_publication",m("id",e.card().getId()));Card actual=current(a);if(e.to()!=null&&e.to().zoneType()==ZoneType.Battlefield)associatePermanentPublication(e,actual);}Map<String,Object> fields=m("class",event.getClass().getName(),"raw_record",event.toString(),"typed_payload",eventPayload(event),"world",world());if(event instanceof GameEventCardChangeZone e)fields.put("zone_operand",m("id",String.valueOf(e.card().getId()),"from",e.from()==null?null:e.from().zoneType().name(),"to",e.to()==null?null:e.to().zoneType().name()));observe("event","publication",fields);}catch(Throwable t){failure("observer",t);}}
        void captureClasses()throws Exception{for(String name:List.of("forge.ai.OrdinaryTriggerReferenceTest","forge.ai.OrdinaryTriggerReferenceTest$Script","forge.ai.OrdinaryTriggerReferenceTest$StrictLobby","forge.ai.OrdinaryTriggerReferenceTest$StrictController","forge.ai.OrdinaryTriggerReferenceTest$StrictCosts","forge.ai.OrdinaryTriggerReferenceTest$CapturedObject","forge.ai.OrdinaryTriggerReferenceTest$Admission","forge.ai.OrdinaryTriggerReferenceTest$PermanentPublication","forge.ai.OrdinaryTriggerReferenceTest$PermanentReceipt","forge.ai.AITest","forge.game.Game","forge.game.GameAction","forge.game.GameActionUtil","forge.game.phase.PhaseHandler","forge.game.player.PlaySpellAbility","forge.game.player.PlayerController","forge.game.player.Player","forge.game.trigger.TriggerHandler","forge.game.trigger.TriggerWaiting","forge.game.trigger.WrappedAbility","forge.game.trigger.TriggerChangesZone","forge.game.trigger.TriggerTaps","forge.game.trigger.TriggerDiscarded","forge.game.trigger.TriggerPhase","forge.game.zone.MagicStack","forge.game.spellability.SpellAbilityStackInstance","forge.game.spellability.SpellAbility","forge.game.card.Card","forge.game.card.CardFactory","forge.game.cost.CostPayment","forge.game.cost.CostTap","forge.game.cost.CostSacrifice","forge.game.cost.CostPartMana","forge.game.ability.effects.DrawEffect","forge.game.ability.effects.ManaEffect","forge.game.combat.Combat","forge.game.mana.ManaPool","forge.card.CardRules","forge.item.PaperCard","com.google.common.eventbus.EventBus","com.google.gson.Gson")){Class<?> c=Class.forName(name);String resource="/"+name.replace('.','/')+".class";byte[] bytes;try(InputStream in=c.getResourceAsStream(resource)){if(in==null)throw new AssertionError("Missing class bytes "+name);bytes=in.readAllBytes();}Path p=Path.of(System.getProperty("spellwake.evidence.dir"),"loader-classes",name.replace('.','/')+".class");Files.createDirectories(p.getParent());if(Files.exists(p)){if(!Arrays.equals(bytes,Files.readAllBytes(p)))throw new AssertionError("Class changed");}else Files.write(p,bytes,StandardOpenOption.CREATE_NEW);classes.add(m("class",name,"code_source",c.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",c.getResource(resource).toString(),"sha256",sha(bytes)));}Files.writeString(Path.of(System.getProperty("spellwake.evidence.dir"),probe+".before-game-classes.json"),JSON.toJson(classes)+"\n",StandardOpenOption.CREATE_NEW);}
    }
    static final class StrictCosts extends CostDecisionMakerBase {
        final Script script; final boolean actualEffect;
        StrictCosts(Player p, SpellAbility a, Script s, boolean effect){super(p,effect,a,a.getHostCard());script=s;actualEffect=effect;}
        @Override public boolean paysRightAfterDecision(){return true;}
        @Override public PaymentDecision visit(CostTap cost) { return script.cost(player,ability,actualEffect,cost); }
        @Override public PaymentDecision visit(CostPartMana cost) { return script.cost(player,ability,actualEffect,cost); }
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
        @Override public PaymentDecision visit(CostSacrifice cost) { return script.cost(player,ability,actualEffect,cost); }
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
        @Override public CardCollectionView cheatShuffle(CardCollectionView list){ throw script.unexpected(getPlayer(),"cheatShuffle",list); }
        @Override public Map<DeckSection,List<? extends PaperCard>> complainCardsCantPlayWell(Deck deck){ throw script.unexpected(getPlayer(),"complainCardsCantPlayWell",deck); }
        @Override public void resetAtEndOfTurn(){script.housekeeping(getPlayer(),"resetAtEndOfTurn");}
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(getPlayer(),hostCard,abilities,triggerEvent); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { script.noStack(getPlayer(),effectSA,mayChoseNewTargets); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { throw script.unexpected(getPlayer(),"orderSimultaneousSa",activePlayerSAs); }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { script.triggers(getPlayer(),activePlayerSAs); }
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
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { return script.targets(getPlayer(),currentAbility); }
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
        @Override public void notifyOfValue(SpellAbility saSource, GameObject realtedTarget, String value) { script.notifyValue(getPlayer(),saSource,realtedTarget,value); }
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
        @Override public Player chooseStartingPlayer(boolean isFirstGame) { throw script.unexpected(getPlayer(),"chooseStartingPlayer",isFirstGame); }
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
        @Override public boolean mulliganKeepHand(Player player, int cardsToReturn) { throw script.unexpected(getPlayer(),"mulliganKeepHand",player,cardsToReturn); }
        @Override public CardCollectionView tuckCardsViaMulligan(CardCollectionView hand, int cardsToReturn) { throw script.unexpected(getPlayer(),"tuckCardsViaMulligan",hand,cardsToReturn); }
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { return script.choose(getPlayer()); }
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { return script.play(getPlayer(),sa); }
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { throw script.unexpected(getPlayer(),"chooseModeForAbility",sa,possible,min,num,allowRepeat); }
        @Override public int chooseNumberForCostReduction(final SpellAbility sa, final int min, final int max) { throw script.unexpected(getPlayer(),"chooseNumberForCostReduction",sa,min,max); }
        @Override public int chooseNumberForKeywordCost(SpellAbility sa, Cost cost, KeywordInterface keyword, String prompt, int max) { throw script.unexpected(getPlayer(),"chooseNumberForKeywordCost",sa,cost,keyword,prompt,max); }
        @Override public int chooseNumber(SpellAbility sa, String title, int min, int max) { throw script.unexpected(getPlayer(),"chooseNumber",sa,title,min,max); }
        @Override public int chooseNumber(SpellAbility sa, String title, List<Integer> values, Player relatedPlayer) { throw script.unexpected(getPlayer(),"chooseNumber",sa,title,values,relatedPlayer); }
        @Override public boolean chooseBinary(SpellAbility sa, String question, BinaryChoiceType kindOfChoice, Boolean defaultChoice) { throw script.unexpected(getPlayer(),"chooseBinary",sa,question,kindOfChoice,defaultChoice); }
        @Override public boolean chooseFlipResult(SpellAbility sa, Player flipper, boolean call) { throw script.unexpected(getPlayer(),"chooseFlipResult",sa,flipper,call); }
        @Override public byte chooseColor(String message, SpellAbility sa, ColorSet colors) { return script.color(getPlayer(),message,sa,colors); }
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
        @Override public void revealUnsupported(Map<Player, List<PaperCard>> unsupported) { throw script.unexpected(getPlayer(),"revealUnsupported",unsupported); }
        @Override public List<OptionalCostValue> chooseOptionalCosts(SpellAbility choosen, List<OptionalCostValue> optionalCostValues) { throw script.unexpected(getPlayer(),"chooseOptionalCosts",choosen,optionalCostValues); }
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { return script.orderCosts(getPlayer(),costs); }
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

}
