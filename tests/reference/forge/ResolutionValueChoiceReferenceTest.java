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
import forge.game.ability.AbilityUtils;
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

/** Strict finite S2 reference probes. No selected probe runs before the external J01 capture gate. */
public class ResolutionValueChoiceReferenceTest extends AITest {

    static final String PIN="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    static final String INPUT_SHA="2f563788ed85b26a0a09209f77cd7ef6b0d25872c3f7cf9ffd9fcebb79512f0a";
    static final List<String> COMMON=List.of("RF01-DM-Elf","RF02-DM-Time-Lord","RF03-Exclude-Wall-Elf","RF04-Controlled-Artifact-Live","RF04-Controlled-Artifact-Sacrificed","RF05-Double-Then-Shield","RF05-Shield-Then-Double","RF05-Late-Missing-Target","RF05-All-Targets-Illegal","RF06-Copy-Choice-Damage-Count","RF06-Copy-Choice-Shield-Damage-Count");
    static final List<String> DIAGNOSTIC=List.of("RF06-Two-Choices-One-Field","RF06-Two-Choices-Two-Fields");
    static final Gson JSON=new GsonBuilder().serializeNulls().disableHtmlEscaping().create();
    static Map<String,Object> m(Object... xs){Map<String,Object> r=new LinkedHashMap<>();for(int i=0;i<xs.length;i+=2)r.put((String)xs[i],xs[i+1]);return r;}
    static String s(JsonObject o,String k){return o.get(k).getAsString();}
    static List<JsonObject> objs(JsonArray a){List<JsonObject> r=new ArrayList<>();for(JsonElement x:a)r.add(x.getAsJsonObject());return r;}
    static List<String> strs(JsonArray a){List<String> r=new ArrayList<>();for(JsonElement x:a)r.add(x.getAsString());return r;}
    static List<Integer> ints(JsonArray a){List<Integer> r=new ArrayList<>();for(JsonElement x:a)r.add(x.getAsInt());return r;}
    static String sha(byte[] b)throws Exception{return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(b));}
    static JsonObject inputs()throws Exception{
        byte[] bytes=Files.readAllBytes(Path.of(Objects.requireNonNull(System.getProperty("spellwake.inputs.path"))));
        if(!sha(bytes).equals(INPUT_SHA))throw new AssertionError("Corrected sealed input bytes mismatch");
        JsonObject root=JsonParser.parseString(new String(bytes,StandardCharsets.UTF_8)).getAsJsonObject();
        if(root.get("schema_version").getAsInt()!=2||!s(root,"forge_commit").equals(PIN))throw new AssertionError("Input version/pin mismatch");
        List<String> ids=new ArrayList<>();for(JsonObject row:objs(root.getAsJsonArray("candidates")))ids.add(s(row,"id"));
        List<String> selected=new ArrayList<>(COMMON);selected.addAll(DIAGNOSTIC);
        if(!ids.equals(selected))throw new AssertionError("Exact thirteen reference roles required");
        return root;
    }
    @DataProvider(name="probes") public Object[][] probes()throws Exception{
        List<Object[]> rows=new ArrayList<>();for(JsonObject row:objs(inputs().getAsJsonArray("candidates")))rows.add(new Object[]{s(row,"id")});return rows.toArray(Object[][]::new);
    }
    @Test(dataProvider="probes") public void probe(String id)throws Exception{
        JsonObject root=inputs(),fixture=null;for(JsonObject row:objs(root.getAsJsonArray("candidates")))if(s(row,"id").equals(id))fixture=row;
        if(fixture==null)throw new AssertionError("Unknown reference role");
        Script x=new Script(root,fixture);Throwable failure=null;
        try{
            x.captureClasses();x.construct();x.game.subscribeToEvents(x);
            x.observe("operation","setupFirstTurn.entry",m("world",x.world()));
            x.game.getPhaseHandler().setupFirstTurn(x.seats.get("A"),null);x.healthy();
            x.observe("operation","setupFirstTurn.return",m("world",x.world()));
            try{
                for(int i=0;i<fixture.getAsJsonObject("budget").get("max_mainLoopStep_calls").getAsInt();i++){
                    x.healthy();x.require(!x.game.isGameOver(),"unexpected_game_over",m());
                    x.observe("loop","entry",m("index",i,"world",x.world()));
                    x.game.getPhaseHandler().mainLoopStep();x.healthy();
                    x.observe("loop","return",m("index",i,"world",x.world()));
                }
                throw x.bad("loop_budget_exhausted",m());
            }catch(ProbeStop stop){x.require(stop==x.stop,"foreign_stop_marker",m());x.healthy();x.stopped=true;x.observe("operation","probe_stop_caught",m("world",x.world()));}
            x.healthy();
        }catch(Throwable error){failure=error;x.failure("uncaught",error);}
        finally{
            if(x.sticky!=null&&failure==null)failure=x.sticky;
            Map<String,Object> result=m("schema_version",1,"kind","resolution-value-choice-reference-raw","reference_id",id,
                "role",COMMON.contains(id)?"matched-common-bounded":"reference-only-storage-diagnostic",
                "native_counterpart",COMMON.contains(id)?"S2NR-"+id:null,"forge_commit",PIN,"input_sha256",INPUT_SHA,
                "probe_stop",x.stopped,"natural_game_return",false,"ai_policy_used",false,"strict_first_controller",true,
                "direct_real_DM_resolution_used",false,"no_floating_mana_seed",true,
                "read_inputs",List.of(System.getProperty("spellwake.inputs.path")),"printing_and_definitions",x.definitions,
                "loaded_classes",x.classes,"immutable_admissions",x.admissionProofs(),"observations",x.records,
                "failure",failure==null?null:m("class",failure.getClass().getName(),"message",failure.getMessage()),
                "limits",List.of("Prepared registered source-bound thirteen finite roles; eleven common and two reference-only diagnostics.",
                    "Global Forge D differs from native target-T setup effect; only selected damage at T is common.",
                    "Derived current counts are not interpreter returns. No private failed-draw flag or full-state parity is claimed.",
                    "Actual offered domains remain raw; no injected Child, alias normalization or native-domain parity."));
            Files.writeString(Path.of(System.getProperty("spellwake.evidence.dir"),id+".json"),JSON.toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
        }
        if(failure!=null)throw new AssertionError("Recorded reference failure "+id,failure);
    }
    static final class ProbeStop extends RuntimeException{private static final long serialVersionUID=1L;ProbeStop(){super("selected nonterminal priority stop",null,false,false);}}
    static final class StrictLobby extends LobbyPlayer implements IGameEntitiesFactory{
        final Script script;final String seat;
        StrictLobby(String seat,Script script){super("resolution-choice-"+seat);this.seat=seat;this.script=script;}
        @Override public Player createIngamePlayer(Game game,int id){Player p=new Player(getName(),game,id);p.setFirstController(new StrictController(game,p,this,script));script.seats.put(seat,p);return p;}
        @Override public PlayerController createMindSlaveController(Player master,Player slave){throw script.bad("mind_slave_factory",m());}
        @Override public void hear(LobbyPlayer p,String message){throw script.bad("lobby_message",m("message",message));}
    }
    static final class Admission{
        final String alias;final SpellAbilityStackInstance instance;final SpellAbility root;final Player controller;
        final Card host;final int hostId;final long timestamp;final JsonElement source,tree;
        final SpellAbilityView admittedView;final SpellAbility sourceCopyAdmissionRoot;final boolean admittedCopiedSpell;
        boolean resolved,removed;int removalNotifications;
        Admission(String alias,SpellAbilityStackInstance instance,Script script){this.alias=alias;this.instance=instance;root=instance.getSpellAbility();controller=root.getActivatingPlayer();host=root.getHostCard();hostId=host.getId();timestamp=host.getGameTimestamp();source=JSON.toJsonTree(script.card(host));tree=JSON.toJsonTree(script.tree(root));admittedView=root.getView();sourceCopyAdmissionRoot=script.copyAwaitingAdmission==root?script.copyAwaitingAdmission:null;admittedCopiedSpell=root.isSpell()&&root.isCopied()&&host.isCopiedSpell();}
    }
    static final class TypeReceipt{
        final Admission entry;final SpellAbility ability;final String value;final boolean second;
        final JsonElement before;final String beforeFirst,beforeSecond;final List<Map<String,Object>> membership;final Set<Player> notified=Collections.newSetFromMap(new IdentityHashMap<>());
        TypeReceipt(Admission entry,SpellAbility ability,String value,Script x){this.entry=entry;this.ability=ability;this.value=value;second=ability.hasParam("ChooseType2");before=JSON.toJsonTree(x.card(ability.getHostCard()));beforeFirst=ability.getHostCard().getChosenType();beforeSecond=ability.getHostCard().getChosenType2();membership=x.membership(entry.controller,value);}
    }
    /** Immutable producing-source facts; Mana retains a distinct engine-created LKI Card. */
    static final class ManaProduction{
        final String sourceAlias;
        final Mana mana;
        final SpellAbility producer;
        final AbilityManaPart part;
        final Card liveSource,lkiSource;
        final int sourceId;
        final long sourceTimestamp;
        final Player sourceOwner,sourceController,player;
        final JsonElement sourceBefore,liveSourceAfter,lkiAtProduction;
        ManaProduction(String alias,Mana mana,SpellAbility producer,JsonElement before,Script script){
            sourceAlias=alias;this.mana=mana;this.producer=producer;part=producer.getManaPart();
            liveSource=producer.getHostCard();lkiSource=mana.getSourceCard();
            sourceId=liveSource.getId();sourceTimestamp=liveSource.getGameTimestamp();
            sourceOwner=liveSource.getOwner();sourceController=liveSource.getController();player=mana.getPlayer();
            sourceBefore=before.deepCopy();liveSourceAfter=JSON.toJsonTree(script.card(liveSource));
            lkiAtProduction=JSON.toJsonTree(script.card(lkiSource));
        }
    }
    static final class Script{
        final JsonObject root,fixture,seed;final String id;final ProbeStop stop=new ProbeStop();
        final Map<String,Player> seats=new LinkedHashMap<>();final Map<String,Card> seeded=new LinkedHashMap<>();
        final Map<Integer,String> aliases=new LinkedHashMap<>();final Map<String,JsonObject> inventory=new LinkedHashMap<>();
        final Map<String,PaperCard> papers=new LinkedHashMap<>();final Map<String,Object> definitions=new LinkedHashMap<>();
        final Map<String,Admission> entries=new LinkedHashMap<>();final IdentityHashMap<SpellAbilityStackInstance,Admission> byInstance=new IdentityHashMap<>();
        final IdentityHashMap<Object,String> references=new IdentityHashMap<>();final List<JsonElement> records=new ArrayList<>();
        final List<Map<String,Object>> classes=new ArrayList<>();final List<TypeReceipt> typeReceipts=new ArrayList<>();
        final List<JsonObject> actions,literals;final Deque<SpellAbility> contexts=new ArrayDeque<>();
        final Map<String,Set<Integer>> targetsUsed=new LinkedHashMap<>();final Set<String> derivedRecorded=new LinkedHashSet<>();
        final Map<String,Integer> replacementOrdinals=new LinkedHashMap<>();final Map<SpellAbility,JsonElement> targetsBeforeClear=new IdentityHashMap<>();
        final Map<String,ManaProduction> manaProductions=new LinkedHashMap<>();
        Game game;int actionCursor,literalCursor,callbacks;long sequence;boolean stopped,commonStart;
        Throwable sticky;SpellAbility selected,copyPending,copyAwaitingAdmission;JsonObject selectedAction;ReplacementEffect replacementPending;TypeReceipt currentType;
        Script(JsonObject root,JsonObject fixture){this.root=root;this.fixture=fixture;seed=fixture.getAsJsonObject("seed");id=s(fixture,"id");actions=objs(fixture.getAsJsonArray("priority_actions"));literals=objs(fixture.getAsJsonArray("literal_callbacks"));}
        void observe(String type,String kind,Map<String,Object> fields){Map<String,Object> row=m("sequence",++sequence,"type",type,"kind",kind);row.putAll(fields);records.add(JSON.toJsonTree(row));}
        void failure(String code,Throwable error){if(sticky==null)sticky=error;observe("failure",code,m("class",error.getClass().getName(),"message",error.getMessage()));}
        AssertionError bad(String code,Map<String,Object> info){AssertionError error=new AssertionError(code+": "+JSON.toJson(info));failure(code,error);return error;}
        void require(boolean condition,String code,Map<String,Object> info){if(!condition)throw bad(code,info);}
        void healthy(){if(sticky!=null)throw new AssertionError("Sticky callback/observer failure",sticky);}
        <T>T call(Player p,String method,Map<String,Object> args,Supplier<T> work){
            healthy();require(++callbacks<=fixture.getAsJsonObject("budget").get("max_total_controller_calls").getAsInt(),"callback_budget",m());strict(p);
            long ordinal=sequence+1;observe("callback","entry",m("callback_id",ordinal,"method",method,"actor",seat(p),"arguments",args,"world",world()));
            try{T result=work.get();healthy();observe("callback","return",m("callback_id",ordinal,"method",method,"reply",diag(result),"world",world()));return result;}
            catch(ProbeStop marker){require(marker==stop,"foreign_stop",m());healthy();observe("callback","nonterminal_stop",m("callback_id",ordinal));throw marker;}
            catch(Throwable error){failure("callback_"+method,error);throw error;}
        }
        AssertionError unexpected(Player p,String method,Object... args){return call(p,method,m("offered",diag(Arrays.asList(args))),()->{throw bad("unexpected_callback",m("method",method));});}
        String ref(Object object){return object==null?null:references.computeIfAbsent(object,key->"ref"+(references.size()+1));}
        String seat(Player p){if(p==null)return null;for(var entry:seats.entrySet())if(entry.getValue()==p)return entry.getKey();throw bad("unknown_player",m("id",p.getId()));}
        String alias(Card card){return card==null?null:aliases.get(card.getId());}
        Card current(String alias){Card origin=seeded.get(alias);require(origin!=null,"unknown_alias",m("alias",alias));Card card=game.getCardState(origin);require(card!=null,"missing_current_object",m("alias",alias));return card;}
        boolean same(Card a,Card b){return a!=null&&b!=null&&a.getId()==b.getId()&&a.getGameTimestamp()==b.getGameTimestamp();}
        Map<String,Object> card(Card c){if(c==null)return null;return m("alias",alias(c),"ref",ref(c),"id",String.valueOf(c.getId()),"timestamp",String.valueOf(c.getGameTimestamp()),"is_lki",c.isLKI(),"name",c.getName(),"owner",seat(c.getOwner()),"controller",seat(c.getController()),"zone",c.getZone()==null?null:c.getZone().getZoneType().name(),"type",c.getType().toString(),"creature_subtypes_raw_iteration",new ArrayList<>(c.getType().getCreatureTypes()),"tapped",c.isTapped(),"creature",c.isCreature(),"power",String.valueOf(c.getNetPower()),"toughness",String.valueOf(c.getNetToughness()),"damage",c.isInPlay()?String.valueOf(c.getDamage()):null,"raw_retained_card_damage_field",String.valueOf(c.getDamage()),"chosen_type",c.getChosenType(),"chosen_type2",c.getChosenType2(),"shield_amount_svar",c.getSVar("ShieldAmount"),"effect_source_ability_ref",ref(c.getEffectSourceAbility()));}
        Map<String,Object> ability(SpellAbility a){if(a==null)return null;return m("ref",ref(a),"id",String.valueOf(a.getId()),"class",a.getClass().getName(),"source",card(a.getHostCard()),"controller",seat(a.getActivatingPlayer()),"spell",a.isSpell(),"copied",a.isCopied(),"mana",a.isManaAbility(),"trigger",a.isTrigger(),"api",a.getApi()==null?null:a.getApi().name(),"params",new LinkedHashMap<>(a.getMapParams()),"svars",new LinkedHashMap<>(a.getSVars()),"cost",a.getPayCosts()==null?null:a.getPayCosts().toString(),"targets",diag(a.getTargets()),"original_ability_ref",ref(a.getOriginalAbility()),"replacing_objects",diag(a.getReplacingObjects()));}
        List<Map<String,Object>> tree(SpellAbility root){List<Map<String,Object>> nodes=new ArrayList<>();List<Integer> path=new ArrayList<>();for(SpellAbility a=root;a!=null;a=a.getSubAbility()){nodes.add(m("child_path",new ArrayList<>(path),"ability",ability(a)));path.add(0);}return nodes;}
        Object diag(Object o){if(o==null||o instanceof String||o instanceof Number||o instanceof Boolean)return o;if(o instanceof Card c)return card(c);if(o instanceof Player p)return m("seat",seat(p),"id",String.valueOf(p.getId()));if(o instanceof SpellAbility a)return m("ref",ref(a),"id",String.valueOf(a.getId()),"host_ref",ref(a.getHostCard()));if(o instanceof TargetChoices t)return m("cards",t.getTargetCards().stream().map(this::card).toList(),"players",Lists.newArrayList(t.getTargetPlayers()).stream().map(this::seat).toList(),"spells",Lists.newArrayList(t.getTargetSpells()).stream().map(this::diag).toList());if(o instanceof Mana mana)return m("ref",ref(mana),"source_lki",card(mana.getSourceCard()),"mana_ability_ref",ref(mana.getManaAbility()),"producer_live_source",mana.getManaAbility()==null?null:card(mana.getManaAbility().getSourceCard()),"player",seat(mana.getPlayer()),"color",String.valueOf(mana.getColor()));if(o instanceof ReplacementEffect replacement)return replacementRecord(replacement);if(o instanceof Map<?,?> map){List<Object> rows=new ArrayList<>();for(var entry:map.entrySet())rows.add(m("key",String.valueOf(entry.getKey()),"value",diag(entry.getValue())));return rows;}if(o instanceof Iterable<?> values){List<Object> rows=new ArrayList<>();for(Object item:values)rows.add(diag(item));return rows;}return m("class",o.getClass().getName(),"ref",ref(o),"text",o.toString());}
        List<Map<String,Object>> stack(){List<Map<String,Object>> rows=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())rows.add(m("id",String.valueOf(si.getId()),"ref",ref(si),"ability_tree",tree(si.getSpellAbility())));return rows;}
        Map<String,Object> world(){if(game==null)return m("stage","construction");Map<String,Object> ps=new LinkedHashMap<>();for(var entry:seats.entrySet()){Player p=entry.getValue();Map<String,Object> zones=new LinkedHashMap<>();for(ZoneType zone:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command)){List<Object> cards=new ArrayList<>();for(Card c:p.getCardsIn(zone))cards.add(card(c));zones.put(zone.name(),cards);}ps.put(entry.getKey(),m("life",String.valueOf(p.getLife()),"lost",p.hasLost(),"zones_raw_iteration",zones,"mana_actual_iteration",diag(p.getManaPool())));}return m("stage",game.getAge().name(),"turn",game.getPhaseHandler().getTurn(),"phase",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",seat(game.getPhaseHandler().getPlayerTurn()),"priority",seat(game.getPhaseHandler().getPriorityPlayer()),"stack_resolving",game.getStack().isResolving(),"action_cursor",actionCursor,"literal_cursor",literalCursor,"stack_top_to_bottom",stack(),"players",ps);}
        void strict(Player p){require(p.getController().getClass()==StrictController.class&&p.getController().getGame()==game&&p.getController().getMatch()==game.getMatch()&&p.getController().getPlayer()==p&&p.getController().getLobbyPlayer()==p.getLobbyPlayer()&&!p.getController().isAI()&&!p.getController().isGuiPlayer()&&p.getController().getFullControl().isEmpty()&&!p.getController().canPlayUnlimitedLands()&&!p.getController().isOrderedZone(),"strict_first_controller_flags",m("seat",seat(p)));}
        void construct()throws Exception{
            require(seed.getAsJsonArray("floating_mana").isEmpty(),"seed_mana_forbidden",m());Set<String> used=new LinkedHashSet<>();
            for(JsonObject row:objs(seed.getAsJsonArray("cards"))){require(inventory.put(s(row,"alias"),row)==null,"duplicate_alias",m());used.add(s(row,"definition"));}
            for(String key:used){JsonObject d=root.getAsJsonObject("definitions").getAsJsonObject(key);PaperCard paper;
                if(d.has("printing")){JsonObject printing=d.getAsJsonObject("printing");paper=FModel.getMagicDb().getCommonCards().getCard(s(printing,"name"),s(printing,"edition"),s(printing,"collector_number"));require(paper!=null&&paper.getName().equals(s(printing,"name"))&&paper.getEdition().equals(s(printing,"edition"))&&paper.getCollectorNumber().equals(s(printing,"collector_number")),"exact_printing",m("definition",key));}
                else{String text=s(d,"reader_utf8");List<String> lines=strs(d.getAsJsonArray("reader_lines"));require(text.equals(String.join("\n",lines)+"\n")&&sha(text.getBytes(StandardCharsets.UTF_8)).equals(s(d,"reader_utf8_sha256")),"exact_reader_bytes",m("definition",key));paper=new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common);}
                papers.put(key,paper);definitions.put(key,m("input",d,"name",paper.getName(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText(),"type",paper.getRules().getType().toString(),"cost",paper.getRules().getManaCost().toString()));
            }
            List<RegisteredPlayer> registered=new ArrayList<>();for(String name:strs(seed.getAsJsonArray("participants"))){Deck deck=new Deck("resolution-choice-"+id+"-"+name);for(JsonObject row:inventory.values())if(s(row,"owner").equals(name))deck.getMain().add(papers.get(s(row,"definition")),1);RegisteredPlayer p=new RegisteredPlayer(deck).setPlayer(new StrictLobby(name,this));p.setStartingLife(new BigInteger(s(seed.getAsJsonObject("life"),name)).intValueExact());p.setTeamNumber(registered.size()+1);registered.add(p);}
            GameRules rules=new GameRules(GameType.Constructed);rules.setOrderCombatants(false);game=new Game(registered,rules,new Match(rules,registered,"Resolution values "+id));game.setAge(GameStage.Play);
            for(JsonObject row:inventory.values()){Card c=Card.fromPaperCard(papers.get(s(row,"definition")),seats.get(s(row,"owner")));c.setGameTimestamp(game.getNextTimestamp());seeded.put(s(row,"alias"),c);aliases.put(c.getId(),s(row,"alias"));}
            for(JsonObject row:inventory.values())if(!s(row,"zone").equals("Library"))seedOne(row);
            for(String owner:seats.keySet())for(String alias:strs(seed.getAsJsonObject("library_order").getAsJsonArray(owner)))seedOne(inventory.get(alias));
            for(var participant:seats.entrySet()){strict(participant.getValue());List<String> actual=new ArrayList<>();for(Card card:participant.getValue().getCardsIn(ZoneType.Library))actual.add(alias(card));require(actual.equals(strs(seed.getAsJsonObject("library_order").getAsJsonArray(participant.getKey()))),"actual_prepared_library_order",m("actor",participant.getKey(),"actual",actual));}game.copyLastState();game.getAction().checkStaticAbilities();
            observe("operation","prepared_seed",m("scope","trusted prepared physical inventory only; actual gameplay operations follow","world",world()));
            for(String alias:seeded.keySet())observe("definition","actual_card_factory",m("alias",alias,"card",card(current(alias)),"nonmana_abilities",current(alias).getNonManaAbilities().stream().map(this::tree).toList(),"mana_abilities",current(alias).getManaAbilities().stream().map(this::tree).toList(),"replacement_effects",diag(current(alias).getReplacementEffects())));
        }
        void seedOne(JsonObject row){Card c=seeded.get(s(row,"alias"));ZoneType zone=ZoneType.valueOf(s(row,"zone"));Player owner=seats.get(s(row,"owner"));if(zone==ZoneType.Battlefield){Player controller=seats.get(s(row,"controller"));if(controller!=owner)c.setController(controller,game.getNextTimestamp());controller.getZone(zone).add(c);c.setTapped(row.get("tapped").getAsBoolean());}else owner.getZone(zone).add(c);}
        List<Integer> path(SpellAbility root,SpellAbility wanted){List<Integer> path=new ArrayList<>();for(SpellAbility a=root;a!=null;a=a.getSubAbility()){if(a==wanted)return path;path.add(0);}throw bad("ability_not_in_actual_tree",m("root",ref(root),"wanted",ref(wanted)));}
        SpellAbility child(SpellAbility root,List<Integer> path){SpellAbility a=root;for(int index:path){require(index==0&&a!=null,"invalid_child_path",m("path",path));a=a.getSubAbility();}require(a!=null,"missing_actual_child",m("path",path));return a;}
        Admission resolving(){require(game.getStack().isResolving(),"not_resolving",m());SpellAbilityStackInstance si=game.getStack().peek();Admission entry=byInstance.get(si);require(entry!=null&&entry.root==si.getSpellAbility()&&game.getStack().isResolving(entry.root.getHostCard()),"actual_resolving_entry_join",m("top",ref(si)));return entry;}
        JsonObject literal(Player p,String method){require(literalCursor<literals.size(),"literal_exhausted",m("method",method));JsonObject row=literals.get(literalCursor);require(s(row,"actor").equals(seat(p))&&s(row,"callback").startsWith(method+"("),"literal_callback_order",m("input",row,"method",method));literalCursor++;return row;}
        Map<String,Object> admissionProofs(){Map<String,Object> rows=new LinkedHashMap<>();for(Admission entry:entries.values())rows.put(entry.alias,m("instance_ref",ref(entry.instance),"entry_id",String.valueOf(entry.instance.getId()),"ability_ref",ref(entry.root),"controller",seat(entry.controller),"immutable_source",entry.source,"immutable_tree",entry.tree,"admitted_ability_view_ref",ref(entry.admittedView),"source_copy_admission_root_ref",ref(entry.sourceCopyAdmissionRoot),"admitted_copied_spell",entry.admittedCopiedSpell,"resolved_publication",entry.resolved,"removed_publication",entry.removed,"raw_removal_notification_count",entry.removalNotifications,"observer_logical_removal_transition_count",entry.removed?1:0));return rows;}
        Admission register(String alias,SpellAbilityStackInstance si){require(!entries.containsKey(alias)&&!byInstance.containsKey(si),"duplicate_entry_binding",m());Admission entry=new Admission(alias,si,this);entries.put(alias,entry);byInstance.put(si,entry);observe("association","actual_admission",m("entry_alias",alias,"entry_id",String.valueOf(si.getId()),"instance_ref",ref(si),"admitted_ability_view_ref",ref(entry.admittedView),"source_copy_admission_root_ref",ref(entry.sourceCopyAdmissionRoot),"admitted_copied_spell",entry.admittedCopiedSpell,"immutable_source",entry.source,"immutable_tree",entry.tree,"world",world()));return entry;}
        SpellAbilityStackInstance unique(SpellAbility selected,Set<SpellAbilityStackInstance> before){List<SpellAbilityStackInstance> matches=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack()){SpellAbility a=si.getSpellAbility();if(!before.contains(si)&&(a==selected||(selected.isActivatedAbility()&&a.getOriginalAbility()==selected&&a.getActivatingPlayer()==selected.getActivatingPlayer()&&same(a.getHostCard(),selected.getHostCard())&&a.getMapParams().equals(selected.getMapParams()))))matches.add(si);}require(matches.size()==1,"unique_actual_admission",m("matches",matches.size()));return matches.get(0);}
        void priorTypeNotifications(){if(currentType!=null)require(currentType.notified.size()==seats.size()-1,"missing_type_notification",m("entry",currentType.entry.alias));}
        List<SpellAbility> choose(Player p){return call(p,"chooseSpellAbilityToPlay",m(),()->{
            require(game.getPhaseHandler().getPriorityPlayer()==p,"priority_actor",m());priorTypeNotifications();
            if(!commonStart&&game.getPhaseHandler().getTurn()==1&&game.getPhaseHandler().getPhase()==PhaseType.MAIN1){require(p==seats.get("A")&&game.getStack().isEmpty(),"common_start_control",m());commonStart=true;observe("snapshot","common_start",m("world",world()));}
            if(actionCursor==actions.size()){JsonObject stopRow=fixture.getAsJsonObject("stop");require(commonStart&&literalCursor==literals.size()&&contexts.isEmpty()&&selected==null&&copyPending==null&&copyAwaitingAdmission==null&&replacementPending==null&&game.getStack().isEmpty()&&entries.values().stream().allMatch(e->e.resolved&&e.removed)&&seat(p).equals(s(stopRow,"actor"))&&game.getPhaseHandler().getTurn()==stopRow.get("turn").getAsInt()&&game.getPhaseHandler().getPhase().name().equals(s(stopRow,"phase")),"structural_stop_guard",m("admissions",admissionProofs()));observe("snapshot","common_final",m("world",world()));healthy();throw stop;}
            JsonObject row=actions.get(actionCursor),guard=row.getAsJsonObject("guard");require(s(row,"actor").equals(seat(p))&&game.getPhaseHandler().getTurn()==guard.get("turn").getAsInt()&&game.getPhaseHandler().getPhase().name().equals(s(guard,"forge_phase")),"literal_priority_guard",m("input",row));
            if(s(row,"op").equals("pass")){actionCursor++;return null;}
            require(selected==null,"unfinished_action",m());Card source=current(s(row,"source"));List<SpellAbility> domain=new ArrayList<>();
            switch(s(row,"op")){case "cast"->{require(source.isInZone(ZoneType.Hand)&&source.getOwner()==p,"cast_source",m());domain.add(source.getFirstSpellAbility());}case "activate_mana"->{require(source.isInPlay()&&source.getController()==p&&s(row,"source").matches("I[0-3]"),"island_source",m());domain.addAll(source.getManaAbilities());require(source.isLand(),"actual_Island_land",m());}case "activate_ability"->{require(source.isInPlay()&&source.getController()==p&&source.getOwner()==seats.get("B")&&s(row,"source").equals("S"),"artifact_source",m());for(SpellAbility a:source.getNonManaAbilities())if(a.isActivatedAbility())domain.add(a);}default->throw bad("unknown_action",m("input",row));}
            observe("choice","ordinary_ability_domain",m("input",row,"complete_offered",domain.stream().map(this::tree).toList(),"source_before_cost",card(source)));
            require(domain.size()==1&&domain.get(0)!=null,"exact_single_source_ability",m());selected=domain.get(0);selectedAction=row;selected.setActivatingPlayer(p);require(selected.getPayCosts()!=null&&selected.getPayCosts().getCostParts().size()==1,"exact_single_original_cost_part",m("cost",selected.getPayCosts()==null?null:selected.getPayCosts().toString()));if(s(row,"op").equals("activate_mana"))require(selected.isIntrinsic()&&selected.getApi()==ApiType.Mana&&"U".equals(selected.getParam("Produced")),"intrinsic_fixed_Island_blue",m());require(selected.canPlay(),"selected_can_play",m("ability",ability(selected)));return List.of(selected);
        });}
        boolean play(Player p,SpellAbility sa){return call(p,"playChosenSpellAbility",m("ability",ability(sa)),()->{
            require(sa==selected&&selectedAction==actions.get(actionCursor),"selected_action_identity",m());JsonObject row=selectedAction;Set<SpellAbilityStackInstance> before=Collections.newSetFromMap(new IdentityHashMap<>());for(SpellAbilityStackInstance si:game.getStack())before.add(si);
            JsonElement pre=JSON.toJsonTree(card(sa.getHostCard()));contexts.push(sa);try{require(PlaySpellAbility.playSpellAbility(p.getController(),p,sa),"actual_play_failed",m());}finally{contexts.pop();}healthy();
            if(row.has("bind_entry")){Admission entry=register(s(row,"bind_entry"),unique(sa,before));require(entry.controller==p,"admitted_controller",m());observe("operation","actual_cost_and_admission_return",m("input",row,"source_before_cost",pre,"callback_host",card(entry.root.getHostCard()),"current_physical_source",card(current(s(row,"source"))),"targets_before_clear",targetsBeforeClear.get(sa),"actual_tree",tree(entry.root)));}
            if(sa.isManaAbility())recordManaProduction(p,sa,row,pre);
            actionCursor++;selected=null;selectedAction=null;return true;
        });}
        int poolIdentityCount(Player player,Mana wanted){int count=0;for(Mana actual:player.getManaPool())if(actual==wanted)count++;return count;}
        boolean manaFromProduction(Mana mana,ManaProduction production){
            Card lki=mana.getSourceCard();return mana==production.mana&&mana.getColor()==ManaAtom.BLUE&&mana.getPlayer()==production.player&&mana.getManaAbility()==production.part&&lki==production.lkiSource&&lki!=production.liveSource&&lki.isLKI()&&lki.getId()==production.sourceId&&lki.getGameTimestamp()==production.sourceTimestamp&&lki.getOwner()==production.sourceOwner&&lki.getController()==production.sourceController;
        }
        Map<String,Object> manaProductionProof(ManaProduction production){return m("source_alias",production.sourceAlias,"mana_ref",ref(production.mana),"producer_ability_ref",ref(production.producer),"producer_mana_part_ref",ref(production.part),"source_id",String.valueOf(production.sourceId),"source_timestamp",String.valueOf(production.sourceTimestamp),"source_owner_at_production",seat(production.sourceOwner),"source_controller_at_production",seat(production.sourceController),"mana_player",seat(production.player),"source_before_action",production.sourceBefore,"live_source_at_production_return",production.liveSourceAfter,"mana_source_lki_at_production_return",production.lkiAtProduction,"lki_and_live_distinct",production.lkiSource!=production.liveSource);}
        void recordManaProduction(Player p,SpellAbility sa,JsonObject action,JsonElement before){
            Card live=sa.getHostCard();AbilityManaPart part=sa.getManaPart();
            require(s(action,"op").equals("activate_mana")&&sa.getApi()==ApiType.Mana&&live==current(s(action,"source"))&&live.isInPlay()&&!live.isLKI()&&live.getOwner()==p&&live.getController()==p&&part!=null&&part.getSourceCard()==live&&part.getLastManaProduced().size()==1,"actual_island_production_source",m("source",card(live)));
            Mana mana=part.getLastManaProduced().iterator().next();Card lki=mana.getSourceCard();
            require(mana.getColor()==ManaAtom.BLUE&&mana.getPlayer()==p&&mana.getManaAbility()==part&&lki!=null&&lki!=live&&lki.isLKI()&&same(lki,live)&&lki.getOwner()==live.getOwner()&&lki.getController()==live.getController()&&poolIdentityCount(p,mana)==1,"actual_island_output_lki_provenance",m("mana",diag(mana),"source_live",card(live)));
            ManaProduction production=new ManaProduction(s(action,"source"),mana,sa,before,this);
            require(manaProductions.putIfAbsent(production.sourceAlias,production)==null,"repeated_literal_mana_production",m("source_alias",production.sourceAlias));
            observe("operation","actual_mana_production",m("source",card(live),"ability",ability(sa),"last_mana_produced",diag(part.getLastManaProduced()),"production",manaProductionProof(production),"world",world()));
        }
        SpellAbility select(Player p,Card host,List<SpellAbility> offered,ITriggerEvent event){return call(p,"getAbilityToPlay",m("host",card(host),"complete_offered",offered.stream().map(this::ability).toList(),"event",diag(event)),()->{require(event==null&&!contexts.isEmpty()&&offered.size()==1&&offered.get(0)==contexts.peek()&&host==contexts.peek().getHostCard(),"singleton_original_cost_ability",m());return contexts.peek();});}
        CostDecisionMakerBase costs(Player p,Player payer,SpellAbility sa,boolean effect,String prompt){return call(p,"getCostDecisionMaker",m("payer",seat(payer),"ability",ability(sa),"effect",effect,"prompt",prompt),()->{require(p==payer&&sa.getActivatingPlayer()==p&&!effect&&!contexts.isEmpty()&&contexts.peek()==sa,"cost_context",m());return new StrictCosts(p,sa,this,false);});}
        PaymentDecision cost(Player p,SpellAbility sa,boolean effect,CostPart part){return call(p,"visit_"+part.getClass().getSimpleName(),m("part",diag(part),"ability",ability(sa),"effect",effect,"payment_stack",diag(game.costPaymentStack.peek())),()->{
            require(!effect&&sa.getActivatingPlayer()==p&&!contexts.isEmpty()&&contexts.peek()==sa&&sa==selected&&game.costPaymentStack.peek()!=null&&game.costPaymentStack.peek().cost()==part&&game.costPaymentStack.peek().payment().getAbility()==sa,"actual_cost_stack",m());
            if(part instanceof CostPartMana mana){String text=mana.getMana().toString();require(mana.getMana().isZero()||(s(selectedAction,"source").equals("DM")&&mana.getMana().getGenericCost()==3&&Lists.newArrayList(mana.getMana()).equals(List.of(ManaCostShard.BLUE))),"literal_mana_cost",m("actual",text));return PaymentDecision.number(0);}
            if(part instanceof CostTap tap){boolean island=s(selectedAction,"op").equals("activate_mana")&&s(selectedAction,"source").matches("I[0-3]")&&sa.isManaAbility();boolean artifact=s(selectedAction,"op").equals("activate_ability")&&s(selectedAction.getAsJsonObject("cost_literal"),"tag").equals("Tap")&&sa.getHostCard()==current("S");require((island||artifact)&&tap.canPay(sa,p,false),"literal_tap_cost",m());return PaymentDecision.number(0);}
            if(part instanceof CostSacrifice sacrifice){require(s(selectedAction,"op").equals("activate_ability")&&s(selectedAction.getAsJsonObject("cost_literal"),"tag").equals("Sacrifice")&&sa.getHostCard()==current("S")&&sacrifice.payCostFromSource()&&sacrifice.getAmount().equals("1")&&sacrifice.canPay(sa,p,false),"literal_sacrifice_cost",m());return PaymentDecision.card(sa.getHostCard());}
            throw bad("unselected_cost",m("part",part.getClass().getName()));
        });}
        boolean pay(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){return call(p,"payManaCost",m("cost",cost.toString(),"ability",ability(sa),"effect",effect),()->{require(!effect&&sa.getActivatingPlayer()==p&&sa==selected&&!contexts.isEmpty()&&contexts.peek()==sa,"mana_payment_context",m());return PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,false);});}
        boolean apply(Player p,ManaCostBeingPaid cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){return call(p,"applyManaToCost",m("cost",cost.toString(),"ability",ability(sa),"pool",diag(p.getManaPool())),()->{
            require(!effect&&sa.getActivatingPlayer()==p&&sa==selected&&s(selectedAction,"source").equals("DM")&&selectedAction.getAsJsonArray("payment").size()==4&&matrix==game.costPaymentStack.peek().payment(),"DM_actual_payment_only",m());
            List<Object> spent=new ArrayList<>();Set<Mana> used=Collections.newSetFromMap(new IdentityHashMap<>());p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);
            try{for(JsonObject row:objs(selectedAction.getAsJsonArray("payment"))){require(s(row,"color").equals("U")&&s(row,"amount").equals("1"),"literal_Blue_one",m());String alias=s(row,"mana_from_source");Card source=current(alias);ManaProduction production=manaProductions.get(alias);require(production!=null&&source.getId()==production.sourceId&&source.getGameTimestamp()==production.sourceTimestamp&&source.getOwner()==production.sourceOwner&&source.getController()==production.sourceController&&production.player==p,"literal_producing_source_receipt",m("source",card(source)));List<Mana> matches=new ArrayList<>();for(Mana mana:p.getManaPool())if(manaFromProduction(mana,production))matches.add(mana);require(matches.size()==1&&matches.get(0)==production.mana&&used.add(matches.get(0)),"unique_source_bound_actual_mana",m("source",card(source),"production",manaProductionProof(production)));Mana mana=matches.get(0);require(mana.meetsManaRestrictions(sa)&&sa.allowsPayingWithShard(mana.getSourceCard(),mana.getColor())&&p.getManaPool().tryPayCostWithMana(sa,cost,mana,false),"actual_tryPayCostWithMana",m());require(poolIdentityCount(p,mana)==0,"selected_actual_mana_was_removed",m("mana",diag(mana)));sa.getPayingMana().add(mana);spent.add(m("mana",diag(mana),"actual_production",manaProductionProof(production)));}require(cost.isPaid(),"incomplete_actual_payment",m());}
            finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}
            observe("operation","actual_payment",m("spent",spent,"paying_mana",diag(sa.getPayingMana()),"remaining_pool",diag(p.getManaPool())));return true;
        });}
        boolean targets(Player p,SpellAbility sa){return call(p,"chooseTargetsFor",m("ability",ability(sa)),()->{
            require(selected!=null&&selectedAction.has("targets")&&child(selected,path(selected,sa))==sa,"target_action_context",m());List<Integer> path=path(selected,sa);List<JsonObject> matches=new ArrayList<>();for(JsonObject row:objs(selectedAction.getAsJsonArray("targets")))if(ints(row.getAsJsonArray("child_path")).equals(path))matches.add(row);require(matches.size()==1&&targetsUsed.computeIfAbsent(String.valueOf(actionCursor),k->new LinkedHashSet<>()).add(path.size()),"literal_target_child",m("path",path));JsonObject row=matches.get(0);List<Object> domain=new ArrayList<>();sa.clearTargets();
            if(row.has("card")){for(Card c:game.getCardsIn(ZoneType.Battlefield))if(sa.canTarget(c))domain.add(card(c));Card wanted=current(s(row,"card"));require(sa.canTarget(wanted)&&sa.getTargets().add(wanted),"actual_card_target",m());}
            else{for(SpellAbilityStackInstance si:game.getStack())if(si.isSpell()&&sa.canTargetSpellAbility(si.getSpellAbility()))domain.add(m("instance_ref",ref(si),"entry_id",String.valueOf(si.getId()),"ability",ability(si.getSpellAbility())));Admission wanted=entries.get(s(row,"entry"));require(wanted!=null&&game.getStack().getInstanceMatchingSpellAbilityID(wanted.root)==wanted.instance&&sa.canTargetSpellAbility(wanted.root)&&sa.getTargets().add(wanted.root),"actual_spell_target",m());}
            require(sa.isTargetNumberValid(),"target_count",m());targetsBeforeClear.put(selected,JSON.toJsonTree(tree(selected)));observe("choice","target_domain_and_selection",m("input",row,"child_path",path,"complete_offered",domain,"actual_all_target_choices",diag(selected.getAllTargetChoices()),"tree",tree(selected)));return true;
        });}
        String type(Player p,String kind,SpellAbility sa,Collection<String> offered,boolean optional){return call(p,"chooseSomeType",m("kind",kind,"optional",optional,"collection_class",offered==null?null:offered.getClass().getName(),"raw_complete_offered",offered==null?null:new ArrayList<>(offered),"ability",ability(sa)),()->{
            priorTypeNotifications();Admission entry=resolving();List<Integer> path=path(entry.root,sa);JsonObject row=literal(p,"chooseSomeType");
            require(s(row,"entry").equals(entry.alias)&&ints(row.getAsJsonArray("ability_child_path")).equals(path)&&sa.getApi()==ApiType.ChooseType&&sa.getActivatingPlayer()==p&&entry.controller==p&&kind.equals("Creature")&&!optional&&s(row,"kind_literal").equals(kind)&&!row.get("isOptional_literal").getAsBoolean()&&"Creature".equals(sa.getParam("Type"))&&"You".equals(sa.getParam("Defined"))&&!sa.usesTargeting()&&!sa.hasParam("ValidTypes")&&!sa.hasParam("TypesFromDefined")&&!sa.hasParam("AtRandom")&&!sa.hasParam("MostPrevalentInDefinedZone")&&!sa.hasParam("Secretly"),"type_callback_context",m("input",row,"entry",entry.alias,"path",path));
            require(offered!=null,"null_type_domain",m());List<Object> raw=new ArrayList<>();int ordinal=0;for(String value:offered)raw.add(m("occurrence_index",ordinal++,"raw",value,"utf8_hex",value==null?null:HexFormat.of().formatHex(value.getBytes(StandardCharsets.UTF_8))));
            observe("choice","raw_type_domain_before_membership",m("entry",entry.alias,"ability_path",path,"collection_class",offered.getClass().getName(),"offered",raw,"Child_actual_membership",offered.contains("Child"),"input",row,"callback_host",card(sa.getHostCard()),"current_physical_source",card(game.getCardState(sa.getHostCard())),"world",world()));
            List<Object> mappedDiagnostics=new ArrayList<>();for(String rawValue:offered){String mapped=Objects.equals(rawValue,"C'tan")?"C’tan":Objects.equals(rawValue,"Shi'ar")?"Shi’ar":rawValue;mappedDiagnostics.add(m("raw",rawValue,"mapped_diagnostic_only",mapped));}observe("diagnostic","raw_domain_scoped_spelling_comparison",m("entry",entry.alias,"raw_to_mapped",mappedDiagnostics,"choice_domain_modified",false,"selected_reply_normalized",false,"domain_size_actual",offered.size()));
            String value=s(row,"return_literal");require(List.of("Elf","Goblin","Time Lord").contains(value)&&offered.contains(value),"unoffered_literal_type",m("literal",value));for(String excluded:strs(row.getAsJsonArray("excluded_raw_members")))require(!offered.contains(excluded),"excluded_type_offered",m("excluded",excluded));
            currentType=new TypeReceipt(entry,sa,value,this);typeReceipts.add(currentType);
            observe("association","type_return_before_host_write",m("entry",entry.alias,"actual_instance_ref",ref(entry.instance),"actual_root_ref",ref(entry.root),"ability_path",path,"literal",value,"immutable_callback_host_before",currentType.before,"chosen_member_world_cut",currentType.membership,"classification","callback return precedes notifyOfValue and host write"));return value;
        });}
        void notifyValue(Player p,SpellAbility sa,GameObject related,String value){call(p,"notifyOfValue",m("ability",ability(sa),"related",diag(related),"value",value),()->{
            require(currentType!=null&&sa==currentType.ability&&related==currentType.entry.controller&&p!=related&&currentType.value.equals(value)&&Objects.equals(currentType.beforeFirst,sa.getHostCard().getChosenType())&&Objects.equals(currentType.beforeSecond,sa.getHostCard().getChosenType2())&&currentType.notified.add(p)&&resolving()==currentType.entry,"type_notification_context",m());
            List<String> stack=StackWalker.getInstance().walk(frames->frames.map(frame->frame.getClassName()+"."+frame.getMethodName()).toList());require(stack.contains("forge.game.ability.effects.ChooseTypeEffect.resolve")&&stack.contains("forge.game.GameAction.notifyOfValue"),"type_notification_source",m());observe("association","type_notification_before_host_write",m("entry",currentType.entry.alias,"callback_host",card(sa.getHostCard()),"immutable_host_before",currentType.before,"source_path",stack));return null;
        });}
        List<Map<String,Object>> membership(Player controller,String label){List<Map<String,Object>> rows=new ArrayList<>();for(Card c:game.getCardsIn(ZoneType.Battlefield))if(c.getController()==controller&&c.getType().hasSubtype(label))rows.add(m("alias",alias(c),"id",String.valueOf(c.getId()),"timestamp",String.valueOf(c.getGameTimestamp()),"controller",seat(c.getController()),"subtypes",new ArrayList<>(c.getType().getCreatureTypes())));rows.sort(Comparator.comparing(row->(String)row.get("id")));return rows;}
        void derived(Admission entry,String cut){if(!derivedRecorded.add(entry.alias))return;if(DIAGNOSTIC.contains(id)){observe("diagnostic","reference_only_host_storage_after_body",m("entry",entry.alias,"callback_host",card(entry.root.getHostCard()),"current_world",world(),"no_individual_draw_child_return_claim",true));return;}List<Map<String,Object>> observations=new ArrayList<>();for(SpellAbility a=entry.root;a!=null;a=a.getSubAbility())if(a.getApi()==ApiType.Draw){String amount=a.getParam("NumCards");String svar=a.getSVar(amount);boolean second=svar.equals("Count$Valid Permanent.ChosenType2+YouCtrl");require(second||svar.equals("Count$Valid Permanent.ChosenType+YouCtrl"),"selected_count_source",m("svar",svar));String label=second?a.getHostCard().getChosenType2():a.getHostCard().getChosenType();List<Map<String,Object>> members=membership(entry.controller,label);TypeReceipt receipt=null;for(TypeReceipt candidate:typeReceipts)if(candidate.entry==entry&&candidate.second==second)receipt=candidate;require(receipt!=null&&receipt.value.equals(label)&&JSON.toJson(receipt.membership).equals(JSON.toJson(members)),"derived_count_member_stability",m("entry",entry.alias,"label",label,"members",members));observations.add(m("entry",entry.alias,"actual_controller",seat(entry.controller),"draw_child_path",path(entry.root,a),"actual_draw_ability",ability(a),"chosen_field",second?"chosenType2":"chosenType","actual_chosen_value",label,"current_matching_permanents",members,"derived_cardinality",String.valueOf(members.size()),"source_cut",cut,"membership_stable_since_type_callback",true,"interpreter_return_observed",false));}observe("derived","current_count_from_actual_battlefield_cut",m("classification","derived from actual battlefield/entry/host cut, never an original NumCards return","consumers",observations,"world",world()));}
        Map<String,Object> replacementRecord(ReplacementEffect re){return m("ref",ref(re),"class",re.getClass().getName(),"mode",re.getMode().name(),"params",new LinkedHashMap<>(re.getMapParams()),"host",card(re.getHostCard()),"overriding_ability",re.getOverridingAbility()==null?null:ability(re.getOverridingAbility()));}
        String replacementRole(ReplacementEffect re,Admission entry){Card host=re.getHostCard();if(seeded.containsKey("D")&&host==current("D")&&current("D").getReplacementEffects().size()==1&&current("D").getReplacementEffects().contains(re)&&re.getMode().name().equals("DamageDone"))return "doubler";SpellAbility shield=entry.root.getSubAbility();if(host.isInZone(ZoneType.Command)&&host.getEffectSourceAbility()==shield&&shield!=null&&shield.getApi()==ApiType.PreventDamage&&"NextN".equals(re.getParam("PreventionEffect"))&&re.getOverridingAbility()!=null&&re.getOverridingAbility().getApi()==ApiType.ReplaceDamage&&"ShieldAmount".equals(re.getOverridingAbility().getParam("Amount")))return "shield";throw bad("unselected_replacement_identity",m("replacement",replacementRecord(re),"entry",entry.alias));}
        ReplacementEffect replacement(Player p,List<ReplacementEffect> offered){return call(p,"chooseSingleReplacementEffect",m("complete_offered",offered.stream().map(this::replacementRecord).toList()),()->{
            priorTypeNotifications();Admission entry=resolving();JsonObject row=literal(p,"chooseSingleReplacementEffect");require(p==current("T").getController()&&s(row,"damage_entry").equals(entry.alias)&&replacementPending==null,"replacement_context",m());int ordinal=replacementOrdinals.getOrDefault(entry.alias,0);require(row.get("ordinal_within_damage").getAsInt()==ordinal,"replacement_ordinal",m());Map<String,ReplacementEffect> roles=new LinkedHashMap<>();for(ReplacementEffect re:offered)require(roles.put(replacementRole(re,entry),re)==null,"duplicate_replacement_role",m());require(roles.keySet().equals(new LinkedHashSet<>(strs(row.getAsJsonArray("offered_identity_set_roles")))),"replacement_complete_identity_domain",m("roles",roles.keySet()));ReplacementEffect chosen=roles.get(s(row,"return_literal_role"));require(chosen!=null&&offered.stream().anyMatch(item->item==chosen),"literal_replacement_membership",m());replacementPending=chosen;replacementOrdinals.put(entry.alias,ordinal+1);observe("choice","replacement_identity_selection",m("input",row,"actual_entry",entry.alias,"selected",replacementRecord(chosen),"callback_host",card(entry.root.getHostCard()),"world",world()));return chosen;
        });}
        void noStack(Player p,SpellAbility effect,boolean mayChooseTargets){call(p,"playSpellAbilityNoStack",m("ability",ability(effect),"mayChooseTargets",mayChooseTargets),()->{
            Admission entry=resolving();JsonObject row=literal(p,"playSpellAbilityNoStack");ReplacementEffect replacement=replacementPending;require(replacement!=null&&replacement.getOverridingAbility()==effect&&effect.getReplacementEffect()==replacement&&effect.getHostCard()==replacement.getHostCard()&&effect.getHostCard().getController()==p&&mayChooseTargets&&row.get("mayChooseTargets_literal").getAsBoolean()&&s(row,"damage_entry").equals(entry.alias)&&s(row,"return_literal_role").equals(replacementRole(replacement,entry))&&(effect.getApi()==ApiType.ReplaceEffect||effect.getApi()==ApiType.ReplaceDamage)&&effect.getSubAbility()==null,"actual_replacement_executor_context",m());
            Object rawParams=effect.getReplacingObject(AbilityKey.OriginalParams);require(rawParams instanceof Map<?,?>,"actual_replacement_original_params",m());Map<?,?> params=(Map<?,?>)rawParams;List<SpellAbility> damageChildren=new ArrayList<>();for(SpellAbility a=entry.root;a!=null;a=a.getSubAbility())if(a.getApi()==ApiType.DealDamage)damageChildren.add(a);require(damageChildren.size()==1&&params.get(AbilityKey.Cause)==damageChildren.get(0)&&params.get(AbilityKey.Affected) instanceof Card target&&same(target,current("T"))&&params.get(AbilityKey.DamageSource) instanceof Card source&&same(source,entry.root.getHostCard()),"actual_damage_source_target_cause_join",m("original_params",diag(params)));
            observe("operation","replacement_body_before",m("input",row,"actual_ability",ability(effect),"world",world()));contexts.push(effect);try{AbilityUtils.resolve(effect);healthy();}catch(Throwable error){observe("operation","replacement_body_exception",m("class",error.getClass().getName(),"message",error.getMessage(),"returned_value_present",false));throw error;}finally{contexts.pop();}observe("operation","replacement_body_normal_void_completion",m("returned_value_present",false,"actual_ability",ability(effect),"world",world()));replacementPending=null;return null;
        });}
        void copies(Player p,List<SpellAbility> offered){call(p,"orderAndPlaySimultaneousSa",m("complete_offered",offered.stream().map(this::tree).toList()),()->{
            priorTypeNotifications();Admission parent=resolving();JsonObject row=literal(p,"orderAndPlaySimultaneousSa");require(parent.alias.equals(s(row,"parent_entry"))&&offered.size()==row.get("count_literal").getAsInt()&&offered.size()==1&&p==seats.get("A")&&!entries.containsKey("C0"),"copy_cohort_context",m());SpellAbility copy=offered.get(0);Admission original=entries.get("O0");require(original!=null&&Lists.newArrayList(parent.root.getSubAbility().getTargets().getTargetSpells()).size()==1&&Lists.newArrayList(parent.root.getSubAbility().getTargets().getTargetSpells()).get(0)==original.root&&copy.isCopied()&&copy.isSpell()&&!copy.isTrigger()&&copy.getHostCard().isCopiedSpell()&&copy.getHostCard().getOwner()==p&&copy.getHostCard().getController()==p&&copy.getActivatingPlayer()==p&&copy.isMayChooseNewTargets()&&copy.getHostCard()!=original.host&&copy.getHostCard().getId()!=original.hostId,"actual_new_copy_host",m());
            List<SpellAbility> a=new ArrayList<>(),b=new ArrayList<>();for(SpellAbility x=copy;x!=null;x=x.getSubAbility())a.add(x);for(SpellAbility x=original.root;x!=null;x=x.getSubAbility())b.add(x);require(a.size()==b.size(),"copied_tree_length",m());for(int i=0;i<a.size();i++)require(a.get(i)!=b.get(i)&&a.get(i).getApi()==b.get(i).getApi()&&a.get(i).getMapParams().equals(b.get(i).getMapParams()),"copied_tree_source",m("index",i));
            aliases.put(copy.getHostCard().getId(),"C");seeded.put("C",copy.getHostCard());copyPending=copy;String parentChosen=parent.root.getHostCard().getChosenType();
            observe("association","copy_new_host_before_setup",m("parent_entry",parent.alias,"parent_choice",parentChosen,"original_immutable_source",original.source,"copy_tree",tree(copy),"world",world()));
            if(!copy.getHostCard().isInZone(ZoneType.Stack))copy.setHostCard(game.getAction().moveToStack(copy.getHostCard(),copy));else game.getStackZone().add(copy.getHostCard());
            seeded.put("C",copy.getHostCard());require(copy.setupNewTargets(p),"actual_copy_setupNewTargets",m());copyAwaitingAdmission=copy;game.getStack().add(copy);
            require(parentChosen.equals(parent.root.getHostCard().getChosenType()),"parent_choice_survives_copy_setup",m());copyPending=null;
            observe("operation","copy_admitted_and_parent_resumed",m("parent_entry",parent.alias,"parent_choice_after",parent.root.getHostCard().getChosenType(),"copy_entry_binding_status",entries.containsKey("C0")?"actual live admission observed":"frozen actual copy awaiting live admission publication","copy_tree",tree(copy),"world",world()));return null;
        });}
        List<Object> creatureTargetDomain(SpellAbility sa){require("Creature".equals(sa.getParam("ValidTgts")),"selected_creature_target_domain",m());List<Object> domain=new ArrayList<>();for(Card c:game.getCardsIn(ZoneType.Battlefield))if(sa.canTarget(c))domain.add(card(c));return domain;}
        TargetChoices newTargets(Player p,SpellAbility sa,Predicate<GameObject> filter,boolean optional){return call(p,"chooseNewTargetsFor",m("ability",ability(sa),"filter",diag(filter),"optional",optional),()->{
            JsonObject row=literal(p,"chooseNewTargetsFor");require(copyPending!=null&&s(row,"entry").equals("C0")&&child(copyPending,ints(row.getAsJsonArray("ability_child_path")))==sa&&filter==null&&optional&&row.get("filter_literal").isJsonNull()&&row.get("optional_literal").getAsBoolean()&&s(row,"return_literal").equals("retain-existing-targets")&&sa.getTargets().getTargetCards().size()==1&&same(sa.getTargets().getTargetCards().get(0),current("T"))&&sa.canTarget(current("T")),"explicit_copy_KEEP_target",m());observe("choice","copy_targets_explicit_KEEP",m("input",row,"complete_actual_legal_target_candidates",creatureTargetDomain(sa),"retained_actual_targets",diag(sa.getTargets()),"null_return_scope","only setupNewTargets source-defined KEEP protocol"));return null;
        });}
        Object playerView(PlayerView p){if(p==null)return null;for(var entry:seats.entrySet())if(entry.getValue().getId()==p.getId())return m("seat",entry.getKey(),"id",String.valueOf(p.getId()));throw bad("unknown_player_view",m());}
        Object view(CardView c){return c==null?null:m("alias",aliases.get(c.getId()),"id",String.valueOf(c.getId()),"ref",ref(c),"owner",playerView(c.getOwner()),"controller",playerView(c.getController()),"zone",c.getZone()==null?null:c.getZone().name());}
        Object view(SpellAbilityView a){return a==null?null:m("id",String.valueOf(a.getId()),"host",view(a.getHostCard()),"spell",a.isSpell(),"description",a.getDescription());}
        Object zoneView(ZoneView z){return z==null?null:m("player",playerView(z.player()),"zone",z.zoneType().name());}
        List<StackTraceElement> removalSourceFrames(){return StackWalker.getInstance().walk(frames->frames.filter(frame->frame.getClassName().equals("forge.game.zone.MagicStack")||frame.getClassName().equals("forge.game.GameAction")).limit(12).map(StackWalker.StackFrame::toStackTraceElement).toList());}
        Map<String,Object> removalPublication(GameEventSpellRemovedFromStack event){
            require(event.sa()!=null,"null_removed_entry",m());
            List<StackTraceElement> frames=removalSourceFrames();
            List<Map<String,Object>> frameRecords=frames.stream().map(frame->m("class",frame.getClassName(),"method",frame.getMethodName(),"file",frame.getFileName(),"line",frame.getLineNumber())).toList();
            List<String> sourcePath=frames.stream().map(frame->frame.getClassName()+"#"+frame.getMethodName()+":"+frame.getLineNumber()).toList();
            List<Admission> matches=new ArrayList<>();for(Admission candidate:entries.values())if(candidate.admittedView==event.sa())matches.add(candidate);
            require(matches.size()==1,"actual_removal_view_identity_join",m("actual_event_view_ref",ref(event.sa()),"raw_view",view(event.sa()),"matching_admissions",matches.size(),"actual_source_frames",frameRecords));
            Admission entry=matches.get(0);SpellAbility ability=entry.root;Card host=ability.getHostCard();
            int liveInstanceMatches=0,liveAbilityMatches=0,liveSourceIdMatches=0,stackZoneSourceIdMatches=0;
            for(SpellAbilityStackInstance item:game.getStack()){if(item==entry.instance)liveInstanceMatches++;if(item.getSpellAbility()==ability||item.getSpellAbility().getId()==ability.getId())liveAbilityMatches++;if(item.getSourceCard().getId()==entry.hostId)liveSourceIdMatches++;}
            for(Card card:game.getStackZone())if(card.getId()==entry.hostId)stackZoneSourceIdMatches++;
            boolean liveStackAbsent=liveInstanceMatches==0&&liveAbilityMatches==0&&liveSourceIdMatches==0&&game.getStack().getInstanceMatchingSpellAbilityID(ability)==null;
            boolean exactCopy=entry.sourceCopyAdmissionRoot==ability&&entry.admittedCopiedSpell&&ability.isSpell()&&ability.isCopied()&&!ability.isTrigger()&&host==entry.host&&!host.isLKI()&&host.isCopiedSpell()&&host.getId()==entry.hostId&&host.getGameTimestamp()==entry.timestamp&&ability.getActivatingPlayer()==entry.controller&&host.getOwner()==entry.controller&&host.getController()==entry.controller&&entry.instance.getSourceCard()==entry.host;
            boolean logicalTransition=!entry.removed;
            Map<String,Object> proof=m("actual_event_view_ref",ref(event.sa()),"retained_admission_view_ref",ref(entry.admittedView),"actual_event_view",view(event.sa()),"instance_ref",ref(entry.instance),"entry_id",String.valueOf(entry.instance.getId()),"root_ref",ref(ability),"source_copy_admission_root_ref",ref(entry.sourceCopyAdmissionRoot),"admitted_copied_spell",entry.admittedCopiedSpell,"source_at_admission",entry.source,"retained_callback_host",card(host),"live_instance_matches",liveInstanceMatches,"live_ability_matches",liveAbilityMatches,"live_source_id_matches",liveSourceIdMatches,"stack_zone_source_id_matches",stackZoneSourceIdMatches,"live_stack_lookup_absent",game.getStack().getInstanceMatchingSpellAbilityID(ability)==null,"source_still_resolving",game.getStack().isResolving(host),"actual_source_frames",frameRecords,"join_scope","event carries a SpellAbilityView; its exact identity joins the retained admitted instance/root, whose actual host supplies physical id/timestamp");
            require(byInstance.get(entry.instance)==entry&&entry.instance.getSpellAbility()==ability&&ability.getView()==entry.admittedView&&event.sa().getId()==ability.getId()&&event.sa().getHostCard()==host.getView()&&entry.resolved&&entry.removed==(entry.removalNotifications>0),"actual_removal_admission_join",proof);
            if(logicalTransition){
                require(entry.removalNotifications==0,"first_removal_notification_ordinal",proof);
                if(entry.admittedCopiedSpell||ability.isCopied()||host.isCopiedSpell())require(exactCopy&&liveStackAbsent&&game.getStack().isResolving(host)&&sourcePath.equals(List.of("forge.game.zone.MagicStack#remove:859","forge.game.zone.MagicStack#remove:865","forge.game.GameAction#ceaseToExist:972","forge.game.zone.MagicStack#removeCardFromStack:774","forge.game.zone.MagicStack#finishResolving:746","forge.game.zone.MagicStack#resolveStack:731")),"actual_copied_spell_first_removal_source",proof);
                entry.removed=true;
            }else{
                // Pinned finishResolving calls remove(si) again after ceaseToExist removed the copied spell.
                require(entry.removalNotifications==1&&exactCopy&&liveStackAbsent&&stackZoneSourceIdMatches==0&&host.getZone()!=null&&host.getZone().is(ZoneType.None)&&game.getStack().isResolving(host)&&sourcePath.equals(List.of("forge.game.zone.MagicStack#remove:859","forge.game.zone.MagicStack#finishResolving:749","forge.game.zone.MagicStack#resolveStack:731")),"source_adjudicated_copied_spell_second_removal",proof);
            }
            entry.removalNotifications++;
            return m("actual_entry",entry.alias,"retained_callback_host",card(host),"raw_removal_notification_ordinal",entry.removalNotifications,"logical_entry_removal_transition",logicalTransition,"observer_logical_removal_transition_count",1,"removal_notification_role",logicalTransition?"first actual removal publication":"source-adjudicated second copied-spell cleanup notification","actual_removal_join",proof);
        }
        Map<String,Object> eventPayload(GameEvent event){
            if(event instanceof GameEventTurnBegan e)return m("owner",playerView(e.turnOwner()),"turn",e.turnNumber());
            if(event instanceof GameEventTurnPhase e)return m("active",playerView(e.playerTurn()),"phase",e.phase().name());
            if(event instanceof GameEventPlayerPriority e)return m("active",playerView(e.turn()),"priority",playerView(e.priority()),"phase",e.phase().name());
            if(event instanceof GameEventZone e)return m("zone",e.zoneType().name(),"player",playerView(e.player()),"mode",e.mode().name(),"card",view(e.card()),"ability",view(e.sa()));
            if(event instanceof GameEventCardChangeZone e)return m("card",view(e.card()),"from",zoneView(e.from()),"to",zoneView(e.to()));
            if(event instanceof GameEventCardStatsChanged e)return m("cards",e.cards().stream().map(this::view).toList(),"transform",e.transform());
            if(event instanceof GameEventPlayerStatsChanged e)return m("players",e.players().stream().map(this::playerView).toList());
            if(event instanceof GameEventCardTapped e)return m("card",view(e.card()),"tapped",e.tapped());
            if(event instanceof GameEventManaPool e)return m("player",playerView(e.player()),"mode",e.mode().name(),"colors",e.colors()==null?null:e.colors().stream().map(x->x.getShortName()).toList());
            if(event instanceof GameEventSpellAbilityCast e)return m("ability",view(e.sa()),"stack_item_id",String.valueOf(e.si().getId()),"stack_index",e.stackIndex(),"targets",e.targetDescription());
            if(event instanceof GameEventSpellResolved e)return m("ability",view(e.spell()),"hasFizzled",e.hasFizzled(),"description",e.stackDescription());
            if(event instanceof GameEventSpellRemovedFromStack e)return m("ability",view(e.sa()));
            if(event instanceof GameEventCardSacrificed e)return m("card",view(e.card()));
            if(event instanceof GameEventCardDamaged e)return m("source",view(e.source()),"card",view(e.card()),"amount",String.valueOf(e.amount()),"damage_type",e.type().name());
            if(event instanceof GameEventCombatChanged)return m("record_components",List.of());
            if(event instanceof GameEventAddLog e)return m("log_type",e.type().name(),"message",e.message(),"source",view(e.sourceCard()));
            throw bad("unprojected_event_record",m("class",event.getClass().getName(),"raw",event.toString()));
        }
        @Subscribe public void event(GameEvent event){if(sticky!=null)return;try{
            if(event instanceof GameEventSpellAbilityCast cast&&copyAwaitingAdmission!=null&&cast.sa().getId()==copyAwaitingAdmission.getId()){SpellAbilityStackInstance actual=game.getStack().getInstanceMatchingSpellAbilityID(copyAwaitingAdmission);require(actual!=null&&actual.getSpellAbility()==copyAwaitingAdmission,"actual_unfrozen_copy_admission",m());register("C0",actual);copyAwaitingAdmission=null;}
            Map<String,Object> fields=m("class",event.getClass().getName(),"raw_record",event.toString(),"typed_payload",eventPayload(event),"world",world());
            if(event instanceof GameEventCardChangeZone move&&move.from()!=null&&move.to()!=null&&move.from().zoneType()==ZoneType.Library&&move.to().zoneType()==ZoneType.Hand){Admission entry=resolving();List<SpellAbility> draws=new ArrayList<>();for(SpellAbility a=entry.root;a!=null;a=a.getSubAbility())if(a.getApi()==ApiType.Draw)draws.add(a);require(!draws.isEmpty(),"draw_source_body",m());derived(entry,"first actual Library-to-Hand publication");fields.put("actual_draw_source_association",m("entry",entry.alias,"instance_ref",ref(entry.instance),"controller",seat(entry.controller),"single_draw_child_path",draws.size()==1?path(entry.root,draws.get(0)):null,"scope",draws.size()==1?"unique actual Draw child in resolving source body":"reference-only diagnostic: entry association, multiple Draw children not individually distinguished","card_alias",aliases.get(move.card().getId())));}
            if(event instanceof GameEventSpellResolved resolved){Admission entry=resolving();require(!entry.resolved&&resolved.spell().getId()==entry.root.getId(),"resolution_publication_join",m());priorTypeNotifications();entry.resolved=true;if(!resolved.hasFizzled())derived(entry,"actual SpellResolved publication after source body");else observe("association","all_illegal_no_reached_count_consumer",m("entry",entry.alias,"actual_hasFizzled",true,"count_return_observed",false));fields.put("actual_entry",entry.alias);}
            if(event instanceof GameEventSpellRemovedFromStack removed)fields.putAll(removalPublication(removed));
            observe("event","publication",fields);
        }catch(Throwable error){failure("observer",error);}}
        void captureClasses()throws Exception{
            List<String> names=new ArrayList<>(List.of("forge.ai.StrictRejectingController","forge.ai.StrictRejectingController$Costs","forge.ai.ResolutionValueChoiceReferenceTest","forge.ai.ResolutionValueChoiceReferenceTest$Script","forge.ai.ResolutionValueChoiceReferenceTest$StrictLobby","forge.ai.ResolutionValueChoiceReferenceTest$StrictController","forge.ai.ResolutionValueChoiceReferenceTest$StrictCosts","forge.ai.ResolutionValueChoiceReferenceTest$Admission","forge.ai.ResolutionValueChoiceReferenceTest$TypeReceipt","forge.ai.ResolutionValueChoiceReferenceTest$ManaProduction","forge.ai.ResolutionValueChoiceReferenceTest$ProbeStop","forge.ai.AITest","forge.game.Game","forge.game.GameAction","forge.game.GameActionUtil","forge.game.phase.PhaseHandler","forge.game.player.PlayerController","forge.game.player.Player","forge.game.player.PlaySpellAbility","forge.game.ability.SpellAbilityEffect","forge.game.ability.AbilityUtils","forge.game.ability.effects.ChooseTypeEffect","forge.game.ability.effects.DrawEffect","forge.game.ability.effects.ManaEffect","forge.game.ability.effects.DamageDealEffect","forge.game.ability.effects.DamagePreventEffect","forge.game.ability.effects.ReplaceDamageEffect","forge.game.ability.effects.ReplaceEffect","forge.game.ability.effects.CopySpellAbilityEffect","forge.game.ability.effects.ChangeZoneEffect","forge.game.card.Card","forge.game.card.CardFactory","forge.game.card.CardCopyService","forge.game.card.CardLists","forge.game.card.CardPredicates","forge.game.card.CardProperty","forge.game.card.CardStateProperty","forge.game.spellability.SpellAbility","forge.game.spellability.SpellAbilityStackInstance","forge.game.zone.MagicStack","forge.game.replacement.ReplacementHandler","forge.game.replacement.ReplacementEffect","forge.game.replacement.ReplaceDamage","forge.game.cost.CostPayment","forge.game.cost.CostAdjustment","forge.game.cost.Cost","forge.game.mana.ManaCostBeingPaid","forge.game.spellability.AbilityManaPart","forge.game.cost.CostPartMana","forge.game.cost.CostTap","forge.game.cost.CostSacrifice","forge.game.mana.ManaPool","forge.game.mana.Mana","forge.card.CardType","forge.card.CardEdition","forge.card.CardRules","forge.item.PaperCard","forge.StaticData","forge.model.FModel","forge.localinstance.properties.ForgeConstants","com.google.common.eventbus.EventBus","com.google.gson.Gson"));
            for(String name:names){Class<?> type=Class.forName(name);String resource="/"+name.replace('.','/')+".class";byte[] bytes;try(InputStream in=type.getResourceAsStream(resource)){if(in==null)throw new AssertionError("Missing loaded class bytes "+name);bytes=in.readAllBytes();}Path out=Path.of(System.getProperty("spellwake.evidence.dir"),"loader-classes",name.replace('.','/')+".class");Files.createDirectories(out.getParent());if(Files.exists(out))require(Arrays.equals(bytes,Files.readAllBytes(out)),"changed_loaded_class",m("class",name));else Files.write(out,bytes,StandardOpenOption.CREATE_NEW);classes.add(m("class",name,"code_source",type.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",type.getResource(resource).toString(),"sha256",sha(bytes)));}
            Files.writeString(Path.of(System.getProperty("spellwake.evidence.dir"),id+".before-game-classes.json"),JSON.toJson(classes)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }

    static final class StrictCosts extends StrictRejectingController.Costs {
        final Script script; final boolean actualEffect;
        @Override protected AssertionError reject(String method,Object... args){return script.unexpected(player,method,args);}
        StrictCosts(Player p, SpellAbility a, Script s, boolean effect){super(p,a,effect);script=s;actualEffect=effect;}
        @Override public boolean paysRightAfterDecision(){return true;}
        @Override public PaymentDecision visit(CostTap cost) { return script.cost(player,ability,actualEffect,cost); }
        @Override public PaymentDecision visit(CostPartMana cost) { return script.cost(player,ability,actualEffect,cost); }
        @Override public PaymentDecision visit(CostSacrifice cost) { return script.cost(player,ability,actualEffect,cost); }
    }

    static final class StrictController extends StrictRejectingController {
        final Script script;
        @Override protected AssertionError reject(String method,Object... args){return script.unexpected(getPlayer(),method,args);}
        StrictController(Game game,Player player,LobbyPlayer lobby,Script script){super(game,player,lobby);this.script=script;}
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(getPlayer(),hostCard,abilities,triggerEvent); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { script.noStack(getPlayer(),effectSA,mayChoseNewTargets); }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { script.copies(getPlayer(),activePlayerSAs); }
        @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) { return script.newTargets(getPlayer(),ability,filter,optional); }
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { return script.targets(getPlayer(),currentAbility); }
        @Override public void notifyOfValue(SpellAbility saSource, GameObject realtedTarget, String value) { script.notifyValue(getPlayer(),saSource,realtedTarget,value); }
        @Override public String chooseSomeType(String kindOfType, SpellAbility sa, Collection<String> validTypes, boolean isOptional) { return script.type(getPlayer(),kindOfType,sa,validTypes,isOptional); }
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { return script.choose(getPlayer()); }
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { return script.play(getPlayer(),sa); }
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { return script.replacement(getPlayer(),possibleReplacers); }
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.pay(getPlayer(),toPay,costPartMana,sa,prompt,matrix,effect); }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.apply(getPlayer(),toPay,ability,prompt,matrix,effect); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { return script.costs(getPlayer(),player,ability,effect,prompt); }
    }

}
