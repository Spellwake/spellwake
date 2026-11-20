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
import forge.game.ability.AbilityKey;
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
public class SpellwakeBlueInteractionReferenceTest extends AITest {
    static final String PIN = "3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    static String INPUT_SHA;
    static final Gson JSON = new GsonBuilder().serializeNulls().disableHtmlEscaping().setObjectToNumberStrategy(ToNumberPolicy.LONG_OR_DOUBLE).create();
    @Test public void bi01() throws Exception { run("BI01"); }
    @Test public void bi02() throws Exception { run("BI02"); }
    @Test public void if01() throws Exception { run("IF01"); }
    @Test public void if02() throws Exception { run("IF02"); }
    @Test public void if03() throws Exception { run("IF03"); }
    @Test public void if04() throws Exception { run("IF04"); }
    @Test public void if05() throws Exception { run("IF05"); }
    @Test public void if06() throws Exception { run("IF06"); }
    @Test public void if07() throws Exception { run("IF07"); }
    @Test public void if08() throws Exception { run("IF08"); }
    @Test public void if09() throws Exception { run("IF09"); }
    @Test public void if10() throws Exception { run("IF10"); }
    @Test public void if11() throws Exception { run("IF11"); }
    @Test public void if12() throws Exception { run("IF12"); }
    @Test public void if15() throws Exception { run("IF15"); }

    @Test public void mandatory() throws Exception {
        JsonObject input=loadInputs();
        if(!input.has("profile")||!str(input,"profile").equals("mandatory-trigger-v1"))throw new AssertionError("Mandatory profile required");
        Throwable failure=null;for(String id:Objects.requireNonNull(System.getProperty("spellwake.selected.cases")).split(",")){try{run(id);}catch(Throwable error){if(failure==null)failure=error;}}if(failure!=null)throw new AssertionError("Mandatory cases failed; inspect every retained case",failure);
    }

    @Test public void optional() throws Exception {
        JsonObject input=loadInputs();
        if(!input.has("profile")||!str(input,"profile").equals("optional-payment-v1"))throw new AssertionError("Optional profile required");
        List<String> wanted=Arrays.asList(Objects.requireNonNull(System.getProperty("spellwake.selected.cases")).split(","));
        if(wanted.isEmpty()||new HashSet<>(wanted).size()!=wanted.size())throw new AssertionError("Duplicate/empty optional selection");
        Throwable failure=null;for(String id:wanted){try{run(id);}catch(Throwable error){if(failure==null)failure=error;}}
        if(failure!=null)throw new AssertionError("Optional cases failed; inspect every retained case",failure);
    }

    static List<Map<String,Object>> optionalRuntimeClasses() throws Exception {
        List<Map<String,Object>> out=new ArrayList<>();
        for(String name:List.of("forge.ai.SpellwakeBlueInteractionReferenceTest$ResolutionCosts","forge.ai.SpellwakeBlueInteractionReferenceTest$ResolutionPayment","forge.ai.SpellwakeBlueInteractionReferenceTest$Call","forge.ai.SpellwakeBlueInteractionReferenceTest$Produced","forge.game.card.CardCopyService","forge.game.ability.AbilityUtils","forge.game.ability.effects.ManaEffect","forge.game.ability.effects.CopySpellAbilityEffect","forge.game.trigger.TriggerSpellAbilityCastOrCopy","forge.game.cost.CostAdjustment","forge.game.cost.CostDecisionMakerBase","forge.game.zone.CostPaymentStack","forge.game.mana.Mana","forge.game.spellability.AbilitySub")){
            Class<?> type=Class.forName(name);String resource="/"+name.replace('.','/')+".class";byte[] bytes;
            try(InputStream in=type.getResourceAsStream(resource)){if(in==null)throw new AssertionError("Missing class bytes "+name);bytes=in.readAllBytes();}
            Path file=Path.of(System.getProperty("spellwake.evidence.dir"),"loader-classes",name.replace('.','/')+".class");Files.createDirectories(file.getParent());
            if(Files.exists(file)){if(!Arrays.equals(bytes,Files.readAllBytes(file)))throw new AssertionError("Changed loaded class "+name);}else Files.write(file,bytes,StandardOpenOption.CREATE_NEW);
            out.add(m("class",name,"code_source",type.getProtectionDomain().getCodeSource()==null?null:type.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",type.getResource(resource).toString(),"sha256",sha(bytes)));
        }return out;
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
        INPUT_SHA=Objects.requireNonNull(System.getProperty("spellwake.inputs.sha256"));
        if(!sha(bytes).equals(INPUT_SHA))throw new AssertionError("Frozen input bytes mismatch");
        return JsonParser.parseString(new String(bytes,StandardCharsets.UTF_8)).getAsJsonObject();
    }
    synchronized void run(String id) throws Exception {
        JsonObject root=loadInputs();JsonObject fixture=null;
        for(JsonObject c:objects(root.getAsJsonArray("cases")))if(str(c,"case_id").equals(id))fixture=c;
        if(fixture==null)throw new AssertionError("Missing frozen case");
        Script s=new Script(root,fixture);Random oldRandom=MyRandom.getRandom();
        MulliganDefs.MulliganRule oldRule=StaticData.instance().getMulliganRule();boolean oldFilter=StaticData.instance().getFilteredHandsEnabled();Throwable thrown=null;
        try {
            s.loaderClasses=runtimeClasses(s.causalProfile());if(s.optional())s.loaderClasses.addAll(optionalRuntimeClasses());
            s.construct();StaticData.instance().setMulliganRule(MulliganDefs.MulliganRule.London);StaticData.instance().setFilteredHandsEnabled(false);
            s.entropy.installedCallCount=MyRandom.callCount();MyRandom.setRandom(s.entropy);
            if(s.prepared())s.prepare();
            else {
                s.game.subscribeToEvents(new Events(s));s.add("operations","start_game_entry",m("hook",null,"registered_inventory",s.fixture.get("inventory")));
                s.match.startGame(s.game,null);s.naturalReturn=true;
                long seq=s.add("operations","start_game_return",m("context",s.header()));s.observe("terminal.return",seq,true,m("kind","natural_return"));
            }
            s.finish();
        }catch(BoundedStop stop){s.require(s.boundedStop&&s.callStack.isEmpty(),"invalid_bounded_exit",m());s.finish();}
        catch(Throwable t){thrown=t;if(s.failures.isEmpty())s.fatal("uncaught",m("class",t.getClass().getName(),"message",String.valueOf(t.getMessage())));s.failCallbacks(t);}
        finally {
            MyRandom.setRandom(oldRandom);StaticData.instance().setMulliganRule(oldRule);StaticData.instance().setFilteredHandsEnabled(oldFilter);
            Path dir=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));Files.writeString(dir.resolve(id+".json"),JSON.toJson(s.output())+"\n",StandardOpenOption.CREATE_NEW);
        }
        if(thrown!=null)throw new AssertionError("Recorded interaction failure "+id,thrown);
    }

    static List<Map<String,Object>> runtimeClasses(boolean mandatory) throws Exception {
        List<Map<String,Object>> out=new ArrayList<>();
        String[] names={"forge.ai.SpellwakeBlueInteractionReferenceTest","forge.ai.SpellwakeBlueInteractionReferenceTest$Script","forge.ai.SpellwakeBlueInteractionReferenceTest$StrictLobby","forge.ai.SpellwakeBlueInteractionReferenceTest$StrictController","forge.ai.SpellwakeBlueInteractionReferenceTest$StrictCosts","forge.ai.SpellwakeBlueInteractionReferenceTest$RecordedEntropy","forge.ai.SpellwakeBlueInteractionReferenceTest$Events","forge.ai.SpellwakeBlueInteractionReferenceTest$BoundedStop","forge.game.ability.effects.CounterEffect","forge.game.ability.effects.ChangeZoneEffect","forge.game.ability.effects.DrawEffect","forge.game.spellability.TargetChoices","forge.game.spellability.SpellAbility","forge.game.spellability.SpellAbilityStackInstance","forge.ai.AITest","forge.card.CardRules","forge.card.CardType","forge.item.PaperCard","forge.game.GameEntity","forge.game.GameOutcome","forge.game.player.PlayerOutcome","forge.game.card.CounterEnumType","forge.game.Game","forge.game.Match","forge.game.GameAction","forge.game.GameActionUtil","forge.game.phase.PhaseHandler","forge.game.player.Player","forge.game.player.PlayerController","forge.game.player.PlaySpellAbility","forge.game.card.Card","forge.game.card.CardFactory","forge.game.card.CardState","forge.game.zone.Zone","forge.game.zone.PlayerZone","forge.game.zone.MagicStack","forge.game.cost.Cost","forge.game.cost.CostPayment","forge.game.cost.CostTap","forge.game.cost.CostPartMana","forge.game.mana.ManaPool","forge.game.mana.ManaCostBeingPaid","forge.game.spellability.SpellPermanent","forge.game.spellability.AbilityManaPart","forge.game.combat.Combat","forge.game.combat.CombatUtil","forge.util.MyRandom","forge.util.MyRandom$CountingRandom","forge.util.Aggregates","java.util.Random","java.util.Collections","java.lang.StackWalker","com.google.common.eventbus.EventBus","com.google.gson.Gson"};
        for(String name:mandatory?java.util.stream.Stream.concat(Arrays.stream(names),java.util.stream.Stream.of("forge.game.trigger.TriggerHandler","forge.game.trigger.TriggerChangesZone","forge.game.trigger.WrappedAbility","forge.game.card.CardZoneTable","forge.game.ability.effects.TokenEffect","forge.game.ability.effects.TokenEffectBase","forge.game.ability.effects.LifeLoseEffect","forge.game.ability.effects.LifeGainEffect","forge.game.card.token.TokenInfo","forge.token.TokenDb","forge.item.PaperToken")).toList():Arrays.asList(names)){Class<?> type=Class.forName(name);String resource="/"+name.replace('.','/')+".class";byte[] bytes;try(InputStream in=type.getResourceAsStream(resource)){if(in==null)throw new AssertionError("Missing class bytes "+name);bytes=in.readAllBytes();}Path file=Path.of(System.getProperty("spellwake.evidence.dir"),"loader-classes",name.replace('.','/')+".class");Files.createDirectories(file.getParent());if(Files.exists(file)){if(!Arrays.equals(bytes,Files.readAllBytes(file)))throw new AssertionError("Changed loaded class "+name);}else Files.write(file,bytes,StandardOpenOption.CREATE_NEW);out.add(m("class",name,"code_source",type.getProtectionDomain().getCodeSource()==null?null:type.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",type.getResource(resource).toString(),"sha256",sha(bytes)));}
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

    static final class ResolutionPayment {
        final JsonObject plan;final SpellAbility ability;final Player payer;final Cost quote;
        CostPayment payment;final List<Object> spent=new ArrayList<>();
        ResolutionPayment(JsonObject plan,SpellAbility ability,Player payer,Cost quote){this.plan=plan;this.ability=ability;this.payer=payer;this.quote=quote;}
    }

    static final class BoundedStop extends RuntimeException { private static final long serialVersionUID=1L; }

    static final class Script {
        final JsonObject root,fixture; final String id;
        boolean mandatory(){return root.has("profile")&&str(root,"profile").equals("mandatory-trigger-v1");}
        boolean optional(){return root.has("profile")&&str(root,"profile").equals("optional-payment-v1");}
        boolean causalProfile(){return mandatory()||optional();}
        List<String> roles(){return causalProfile()?List.of("P0","P1"):List.of("U","G");}
        String other(String role){require(roles().contains(role),"role",m("role",role));return roles().get(role.equals(roles().get(0))?1:0);}
        final Map<Integer,String> triggerNames=new LinkedHashMap<>();
        final Set<String> usedTriggers=new LinkedHashSet<>(),usedPlayerChoices=new LinkedHashSet<>();
        final Map<String,List<String>> tokenCohorts=new LinkedHashMap<>();
        final Map<String,Card> savedObjects=new LinkedHashMap<>();
        final List<Map<String,Object>> triggerReceipts=new ArrayList<>(),tokenReceipts=new ArrayList<>();
        final Map<String,List<Map<String,Object>>> causalEvents=new LinkedHashMap<>();
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
        boolean naturalReturn,finishedEvent,boundedStop,naturalPreparedReturn;
        final Map<String,SpellAbility> castAbilities=new LinkedHashMap<>();
        final Map<Integer,String> castActions=new LinkedHashMap<>();
        final Map<String,String> settlements=new LinkedHashMap<>();
        final List<Map<String,Object>> castOperations=new ArrayList<>();
        final List<Map<String,Object>> settlementEvents=new ArrayList<>();
        final Set<String> settlementCuts=new LinkedHashSet<>();
        final Map<String,Integer> declarationActions=new LinkedHashMap<>();
        List<Map<String,Object>> loaderClasses=List.of();
        SpellAbility selected; JsonObject selectedAction; String ordinaryOperation;
        final List<String> decisionPlanConsumption=new ArrayList<>();
        final List<Map<String,Object>> optionalChoices=new ArrayList<>(),resolutionPayments=new ArrayList<>(),manaActivations=new ArrayList<>(),spellCopies=new ArrayList<>(),rawStackAdditions=new ArrayList<>();
        final List<Map<String,Object>> plannedInputRetirements=new ArrayList<>();
        final Map<Integer,String> copyNames=new LinkedHashMap<>();
        final Map<Integer,Map<String,Object>> pendingCopyReceipts=new LinkedHashMap<>();
        final Set<String> fundingNotifications=new LinkedHashSet<>();
        boolean copyTargetChoiceConsumed;Map<String,Object> fundingPoolAtEntry;Call fundingCostCallback;Long fundingCostReturnSequence;
        final Map<String,SpellAbility> copyAbilities=new LinkedHashMap<>();
        final Map<Integer,String> syntheticCopies=new LinkedHashMap<>();
        final Map<String,Boolean> paymentResults=new LinkedHashMap<>();
        ResolutionPayment resolutionPayment;JsonObject fundingInstruction,copyPlan;SpellAbility copyingAbility;
        Long fundingColorCallback;int decisionPlanCursor;

        Script(JsonObject root,JsonObject fixture){
            this.root=root;this.fixture=fixture;id=str(fixture,"case_id");
            actions=objects(fixture.getAsJsonArray("actions"));decisions=objects(fixture.getAsJsonArray("decisions"));opening=objects(fixture.getAsJsonArray("opening_policy"));
            for(String k:List.of("callbacks","events","random","operations","failures"))streams.put(k,new ArrayList<>());
            failures=streams.get("failures");
            for(JsonObject row:objects(fixture.getAsJsonArray("inventory")))inventory.put(str(row,"alias"),row);
            entropy=new RecordedEntropy(this);
        }
        void construct(){
            require(str(root,"schema").equals("interaction-game-inputs-v1"),"input_schema",m());
            for(String key:root.getAsJsonObject("definitions").keySet()){
                JsonObject d=root.getAsJsonObject("definitions").getAsJsonObject(key);
                PaperCard p=FModel.getMagicDb().getCommonCards().getCard(str(d,"name"),str(d,"set").toUpperCase(Locale.ROOT),str(d,"collector_number"));
                require(p!=null,"printing_missing",m("definition",key));
                CardRules rules=p.getRules();CardType type=rules.getType();
                paperCards.put(key,m("name",p.getName(),"set",p.getEdition(),"collector_number",p.getCollectorNumber(),"foil",p.isFoil(),"art_index",p.getArtIndex(),"functional_variant",p.getFunctionalVariant(),"oracle_text",rules.getOracleText(),"mana_cost",rules.getManaCost().toString(),"printed_power",rules.getPower(),"printed_toughness",rules.getToughness(),"type",type.toString(),"core_types",type.getCoreTypes().stream().map(Enum::name).sorted().toList(),"supertypes",type.getSupertypes().stream().map(Enum::name).sorted().toList(),"subtypes",type.getSubtypes().stream().sorted().toList(),"basic_land",type.isBasicLand(),"creature",type.isCreature()));
                require(p.getName().equals(str(d,"name"))&&p.getEdition().equalsIgnoreCase(str(d,"set"))&&p.getCollectorNumber().equals(str(d,"collector_number"))&&!p.isFoil(),"printing_identity",m("definition",key));
                JsonObject face=d.getAsJsonObject("face"); boolean land=type.isBasicLand();
                require(rules.getOracleText().equals(str(face,"oracle_text")),"printing_text",m("definition",key,"observed",rules.getOracleText()));
                require(rules.getManaCost().toString().equals(land?"no cost":str(face,"mana_cost")),"printing_cost",m("definition",key));
                require(type.toString().equals(str(face,"type_line").replace("\u2014","-")),"printing_type",m("definition",key,"observed",type.toString()));
                require(Objects.equals(rules.getPower(),face.get("power").isJsonNull()?null:str(face,"power"))&&Objects.equals(rules.getToughness(),face.get("toughness").isJsonNull()?null:str(face,"toughness")),"printing_pt",m("definition",key));
                require(rules.getColor().getColor()==number(d,"color_mask"),"printing_colors",m("definition",key));
                @SuppressWarnings("unchecked") Map<String,Object> receipt=(Map<String,Object>)paperCards.get(key);
                receipt.put("color_mask",rules.getColor().getColor());receipt.put("oracle_id",str(d,"oracle_id"));receipt.put("source_receipt",d.get("source_receipt"));
                papers.put(key,p);
            }
            List<RegisteredPlayer> registered=new ArrayList<>();
            for(String seat:roles()){
                Deck d=new Deck("ordinary-"+id+"-"+seat);
                for(JsonObject row:inventory.values())if(str(row,"owner").equals(seat))d.getMain().add(papers.get(str(row,"definition_ref")),1);
                require(d.getMain().countAll()==(causalProfile()&&prepared()?(int)inventory.values().stream().filter(r->str(r,"owner").equals(seat)).count():60),"deck_count",m("seat",seat));
                RegisteredPlayer rp=new RegisteredPlayer(d).setPlayer(new StrictLobby(seat,this));
                rp.setStartingLife(startingLife(seat));rp.setStartingHand(7);rp.setTeamNumber(seat.equals(roles().get(0))?1:2);rp.setRandomFoil(false);registered.add(rp);
            }
            GameRules rules=new GameRules(GameType.Constructed);rules.setOrderCombatants(false);
            match=new Match(rules,registered,"Ordinary strict reference "+id);game=match.createGame();
            for(Player p:seats.values())strict(p);
        }
        int startingLife(String role){if(!causalProfile())return 20;JsonObject init=fixture.getAsJsonObject("initialization");return number((prepared()?init.getAsJsonObject("setup"):init).getAsJsonObject("life"),role);}
        boolean prepared(){return str(fixture.getAsJsonObject("initialization"),"kind").equals("scheduled");}
        void prepare(){
            JsonObject setup=fixture.getAsJsonObject("initialization").getAsJsonObject("setup");
            game.setAge(GameStage.Play);Map<String,Card> all=new LinkedHashMap<>();
            for(JsonObject row:inventory.values()){
                String a=str(row,"alias");Player owner=seats.get(str(row,"owner"));Card card=Card.fromPaperCard(papers.get(str(row,"definition_ref")),owner);
                card.setGameTimestamp(game.getNextTimestamp());require(!card.isStartsGameInPlay(),"prepared_card_start_flag",m("alias",a));all.put(a,card);aliases.put(card.getId(),a);physical.put(a,card.getId());
            }
            Set<String> placed=new LinkedHashSet<>();
            for(String a:strings(setup.getAsJsonArray("initial_battlefield_order"))){Card card=all.get(a);require(card!=null&&placed.add(a),"prepared_field_order",m("alias",a));card.setGameTimestamp(game.getNextTimestamp());card.getOwner().getZone(ZoneType.Battlefield).add(card);}
            for(String owner:roles())for(String key:causalProfile()?List.of("hand","library_top_first","graveyard_top_first","exile"):List.of("hand","library_top_first","graveyard_top_first")){
                ZoneType zone=key.equals("hand")?ZoneType.Hand:key.equals("library_top_first")?ZoneType.Library:key.equals("exile")?ZoneType.Exile:ZoneType.Graveyard;
                List<String> order=new ArrayList<>(strings(setup.getAsJsonObject(key).getAsJsonArray(owner)));if(zone==ZoneType.Graveyard)Collections.reverse(order);
                for(String a:order){Card card=all.get(a);require(card!=null&&seat(card.getOwner()).equals(owner)&&placed.add(a),"prepared_partition",m("alias",a,"zone",key));card.getOwner().getZone(zone).add(card);}
            }
            require(placed.size()==inventory.size(),"prepared_inventory",m());Player active=seats.get(str(setup,"seed_active_role"));int n=number(setup,"individual_turn");
            boolean actualTurn=str(setup,"step").equals("untap");
            require(game.getPhaseHandler().getPhase()==null&&game.getStack().isEmpty(),"prepared_prephase",m());
            game.getPhaseHandler().devModeSet(actualTurn?null:PhaseType.MAIN1,active,false,actualTurn?n-1:n);
            add("operations","prepared_numeric_counter",m("phase_assignment",actualTurn?null:"MAIN1","counter",actualTurn?n-1:n,"active",seat(active),"purpose",actualTurn?"declared predecessor counter with phase remaining null before actual turn begin":"declared scheduled main-phase prefix","no_gameplay_resolution",true));
            if(causalProfile())for(String role:roles())seats.get(role).setLife(number(setup.getAsJsonObject("life"),role),null);
            game.subscribeToEvents(new Events(this));
            if(actualTurn){game.getPhaseHandler().setupFirstTurn(active,null);}
            else {game.getPhaseHandler().onStackResolved();add("operations","prepared_priority_enable",m("method","PhaseHandler.onStackResolved","purpose","prepared givePriorityToPlayer flag only","resolved_spell_event_claimed",false));}
            long seq=add("operations","prepared_initialized",m("global_battlefield_order",setup.get("initial_battlefield_order"),"source_setup",setup,"context",header()));observe("prepared.initialized",seq,true,m());
            game.getPhaseHandler().mainGameLoop();naturalPreparedReturn=true;
            long end=add("operations","prepared_loop_return",m("context",header()));observe("terminal.return",end,true,m("kind","natural_prepared_loop_return"));
        }
        void settlementCuts(){
            for(var entry:castAbilities.entrySet())if(settlements.containsKey(entry.getKey())&&!settlementCuts.contains(entry.getKey())&&game.getStack().getInstanceMatchingSpellAbilityID(entry.getValue())==null){
                settlementCuts.add(entry.getKey());long seq=add("operations","settled_ready",m("action_id",entry.getKey(),"disposition",settlements.get(entry.getKey()),"context",header()));observe("settle."+entry.getKey(),seq,true,m("disposition",settlements.get(entry.getKey())));
            }
        }
        List<String> stackAliases(){List<String> out=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())out.add(optional()?entryName(si.getSpellAbility()):causalProfile()&&si.getSpellAbility().isTrigger()?triggerNames.get(si.getSpellAbility().getId()):alias(si.getSourceCard()));Collections.reverse(out);return out;}
        boolean settled(String action){return settlements.containsKey(action)&&castAbilities.containsKey(action)&&game.getStack().getInstanceMatchingSpellAbilityID(castAbilities.get(action))==null;}
        boolean stopReady(Player p){
            JsonObject stop=fixture.getAsJsonObject("stop");if(!(str(stop,"kind").equals("ready_priority")||optional()&&str(stop,"kind").equals("at_stack"))||actionCursor!=actions.size()||optional()&&decisionPlanCursor!=fixture.getAsJsonArray("decision_plans").size())return false;
            if(turn()!=number(stop,"individual_turn")||!step().equals(str(stop,"step"))||!seat(p).equals(str(stop,"priority_holder"))||!seat(game.getPhaseHandler().getPlayerTurn()).equals(str(stop,"active_role"))||(optional()? !stackAliases().equals(strings(stop.getAsJsonArray("expected_stack_bottom_first"))):!game.getStack().isEmpty()))return false;
            for(String action:strings(stop.getAsJsonArray("required_settlements")))require(settled(action),"unsettled_bounded_action",m("action_id",action));return true;
        }
        boolean chooseTargets(Player p,SpellAbility sa){
            if(causalProfile()&&sa.isTrigger()){
                String name=triggerNames.get(sa.getId());require(name!=null,"unnamed_trigger_target",m());
                List<JsonObject> choices=objects(fixture.getAsJsonArray("player_choices")).stream().filter(row->str(row,"trigger").equals(name)).toList();
                Call c=enter(p,"chooseTargetsFor",m("ability",ability(sa),"actual_player_domain",seats.values().stream().filter(sa::canTarget).map(this::seat).toList(),"trigger",name));
                require(choices.size()==1&&str(choices.get(0),"chooser").equals(seat(p))&&usedPlayerChoices.add(name),"trigger_player_choice",m());JsonObject choice=choices.get(0);
                require(new HashSet<>(strings(choice.getAsJsonArray("domain_roles"))).equals(new HashSet<>(seats.values().stream().filter(sa::canTarget).map(this::seat).toList())),"trigger_player_domain",m());
                Player target=seats.get(str(choice,"target"));require(target!=null&&sa.canTarget(target),"trigger_player_target",m());sa.getTargets().add(target);require(sa.isTargetNumberValid(),"trigger_target_count",m());leave(c,m("input_policy",choice,"actual_ability",ability(sa),"result",true));return true;
            }
            Call call=enter(p,"chooseTargetsFor",m("ability",ability(sa),"policy",selectedAction));require(sa==selected&&p==sa.getActivatingPlayer()&&selectedAction.has("target")&&!selectedAction.get("target").isJsonNull(),"target_context",m());
            JsonObject target=selectedAction.getAsJsonObject("target");
            if(str(target,"kind").equals("object")){Card card=card(str(target,"card"));require(card.isInPlay()&&sa.canTarget(card),"target_object_domain",m("target",cardState(card)));sa.getTargets().add(card);}
            else if(str(target,"kind").equals("stack")){
                List<SpellAbilityStackInstance> matches=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())if((optional()?entryName(si.getSpellAbility()):alias(si.getSourceCard())).equals(str(target,"spell")))matches.add(si);
                require(matches.size()==1&&sa.canTarget(matches.get(0).getSpellAbility()),"target_stack_domain",m("matches",matches.size()));sa.getTargets().add(matches.get(0).getSpellAbility());
            }else if(causalProfile()&&str(target,"kind").equals("player")){Player player=seats.get(str(target,"player"));require(player!=null&&sa.canTarget(player),"target_player_domain",m());sa.getTargets().add(player);}
            else throw bad("target_kind",m());
            require(sa.isTargetNumberValid(),"target_count",m());leave(call,m("result",true,"actual_ability",ability(sa)));return true;
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
            for(var e:seats.entrySet()){Player p=e.getValue();Map<String,Object> counts=new LinkedHashMap<>();for(ZoneType z:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command))counts.put(z.name().toLowerCase(Locale.ROOT),p.getCardsIn(z).size());players.put(e.getKey(),m("player_id",p.getId(),"life",String.valueOf(p.getLife()),"mana",pool(p),"counts",counts,"lost",p.hasLost()));}
            return m("age",game.getAge().name(),"individual_turn",turn(),"personal_turn",personal(),"step",step(),"active",seat(game.getPhaseHandler().getPlayerTurn()),"raw_priority",seat(game.getPhaseHandler().getPriorityPlayer()),"stack_size",game.getStack().size(),"game_over",game.isGameOver(),"materialized_aliases",aliases.size(),"action_cursor",actionCursor,"decision_cursor",decisionCursor,"players",players);
        }
        Call enter(Player p,String method,Map<String,Object> arguments){
            healthy();strict(p);
            require(++choiceCursor<=fixture.getAsJsonObject("watchdog").get("maximum_callbacks").getAsInt(),"callback_watchdog",m());
            long cid=++callIdentity;List<Map<String,Object>> frames=stackFrames();List<String> tags=tagsFromFrames(frames);
            long seq=add("callbacks","entry",m("callback_id",cid,"parent_callback_id",callStack.isEmpty()?null:callStack.peek().id,"method",method,"callback_kind",method.startsWith("visit_")?"cost_visitor":"controller","actor",seat(p),"actor_player_id",p.getId(),"controller_class",p.getController().getClass().getName(),"arguments",arguments,"context",header(),"operation_id",ordinaryOperation,"actual_stack_tags",tags,"actual_stack_frames",frames));
            Call call=new Call(cid,seq,method,p,tags);callStack.push(call);return call;
        }
        void callerPath(Call call,String... expected){
            String anchor="forge.ai.SpellwakeBlueInteractionReferenceTest$StrictController."+call.method;int offset=call.stackTags.indexOf(anchor);
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
        String alias(Card c){String a=aliases.get(c.getId());if(a==null&&causalProfile()&&c.isToken())a=registerToken(c);if(a==null)throw bad("unknown_card",m("physical_id",c.getId(),"name",c.getName()));return a;}
        List<Card> actualCards(){List<Card> out=new ArrayList<>();for(Player p:seats.values())for(ZoneType z:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command))for(Card c:p.getCardsIn(z))out.add(c);for(Card c:game.getStackZone().getCards())out.add(c);return out;}
        Card card(String alias){Integer id=physical.get(alias);if(id==null&&causalProfile())for(var e:aliases.entrySet())if(e.getValue().equals(alias))id=e.getKey();require(id!=null,"unmaterialized_card",m("alias",alias));Card found=null;for(Card c:actualCards())if(c.getId()==id){require(found==null,"duplicate_physical_membership",m("alias",alias));found=c;}require(found!=null,"missing_physical",m("alias",alias));return found;}
        List<String> sequence(Iterable<Card> cards){List<String> out=new ArrayList<>();for(Card c:cards)out.add(alias(c));return out;}
        List<String> domain(Iterable<Card> cards){List<String> out=sequence(cards);Collections.sort(out);return out;}
        Map<String,Object> cardState(Card c){boolean bf=c.isInPlay(),creature=(causalProfile()||bf)&&c.isCreature();Map<String,Object> state=m("physical_id",String.valueOf(c.getId()),"timestamp",String.valueOf(c.getGameTimestamp()),"owner",seat(c.getOwner()),"owner_player_id",c.getOwner().getId(),"controller",bf||c.isInZone(ZoneType.Stack)?seat(c.getController()):null,"zone",c.getZone().getZoneType().name().toLowerCase(Locale.ROOT),"tapped",bf?c.isTapped():null,"power",creature?String.valueOf(c.getNetPower()):null,"toughness",creature?String.valueOf(c.getNetToughness()):null,"marked_damage",creature?String.valueOf(c.getDamage()):null,"sick",creature?c.isSick():null,"plus_one_counters",creature?String.valueOf(c.getCounters(CounterEnumType.P1P1)):null,"minus_one_counters",creature?String.valueOf(c.getCounters(CounterEnumType.M1M1)):null);if(causalProfile()){state.put("token",c.isToken());state.put("name",c.getName());state.put("counter_inventory",c.getCounters().entrySet().stream().map(e->m("kind",e.getElement().toString(),"amount",String.valueOf(e.getCount()))).sorted(Comparator.comparing(JSON::toJson)).toList());}return state;}
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
            require(causalProfile()?inventory.keySet().stream().allMatch(cards::containsKey):cards.size()==aliases.size(),"world_inventory",m("cards",cards.size(),"aliases",aliases.size()));
            for(var e:seats.entrySet()){Player p=e.getValue();List<String> gy=sequence(p.getCardsIn(ZoneType.Graveyard));Collections.reverse(gy);players.put(e.getKey(),m("life",String.valueOf(p.getLife()),"mana",pool(p),"library",sequence(p.getCardsIn(ZoneType.Library)),"hand",domain(p.getCardsIn(ZoneType.Hand)),"battlefield",domain(p.getCardsIn(ZoneType.Battlefield)),"graveyard",gy,"exile",domain(p.getCardsIn(ZoneType.Exile)),"won",p.getOutcome()==null?null:p.getOutcome().hasWon(),"lost",p.hasLost(),"loss_reason",p.getOutcome()==null||p.getOutcome().lossState==null?null:p.getOutcome().lossState.name(),"lands_played",p.getLandsPlayedThisTurn()));}
            List<Object> stack=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())stack.add(stackEntry(si));Collections.reverse(stack);
            List<Object> combat=new ArrayList<>();if(game.getCombat()!=null)for(Card attacker:game.getCombat().getAttackers())combat.add(m("attacker",alias(attacker),"blocked",game.getCombat().isBlocked(attacker),"blockers",sequence(game.getCombat().getBlockers(attacker))));return m("header",header(),"cards",cards,"players",players,"stack",stack,"combat",combat);
        }
        Map<String,Object> stackEntry(SpellAbilityStackInstance si){Map<String,Object> row=m("stack_id",si.getId(),"ability",ability(si.getSpellAbility()),"controller",seat(si.getActivatingPlayer()));if(causalProfile())row.put("resolving",(optional()?actualResolving(si):game.getStack().isResolving(si.getSourceCard())));return row;}
        Map<String,Object> registeredPlayers(){Map<String,Object> out=new LinkedHashMap<>();for(var entry:seats.entrySet())out.put(entry.getKey(),m("player_id",entry.getValue().getId()));return out;}
        Map<String,Object> otherKnownWorld(String owner){
            String other=other(owner);Player player=seats.get(other);
            Map<String,Object> cards=new TreeMap<>(),zones=new LinkedHashMap<>();
            for(Card card:actualCards())if(card.getOwner()==player)require(cards.put(alias(card),cardState(card))==null,"other_world_duplicate",m("card",card.getId()));
            long known=physical.keySet().stream().filter(a->str(inventory.get(a),"owner").equals(other)).count();
            require(cards.size()==known,"other_world_known_inventory",m("seat",other,"cards",cards.size(),"known",known));
            for(ZoneType zone:List.of(ZoneType.Library,ZoneType.Hand,ZoneType.Battlefield,ZoneType.Graveyard,ZoneType.Exile,ZoneType.Command))zones.put(zone.name().toLowerCase(Locale.ROOT),sequence(player.getCardsIn(zone)));
            List<Card> stack=new ArrayList<>();for(Card card:game.getStackZone().getCards())if(card.getOwner()==player)stack.add(card);zones.put("stack",sequence(stack));
            return m("seat",other,"player_id",player.getId(),"materialized_aliases",cards.size(),"life",String.valueOf(player.getLife()),"mana",pool(player),"lands_played",player.getLandsPlayedThisTurn(),"cards",cards,"actual_zone_order",zones);
        }
        Map<String,Object> ability(SpellAbility a){
            List<Object> targets=new ArrayList<>();for(Card card:a.getTargets().getTargetCards())targets.add(m("kind","object","alias",alias(card),"physical_id",String.valueOf(card.getId()),"timestamp",String.valueOf(card.getGameTimestamp()),"zone",card.getZone()==null?null:card.getZone().getZoneType().name().toLowerCase(Locale.ROOT)));
            for(SpellAbility target:a.getTargets().getTargetSpells()){SpellAbilityStackInstance si=game.getStack().getInstanceMatchingSpellAbilityID(target);targets.add(m("kind","stack","source",alias(target.getHostCard()),"ability_id",target.getId(),"stack_id",si==null?null:si.getId(),"source_timestamp",String.valueOf(target.getHostCard().getGameTimestamp())));}
            Map<String,Object> projection=m("ability_id",a.getId(),"source",alias(a.getHostCard()),"source_physical_id",String.valueOf(a.getHostCard().getId()),"source_timestamp",String.valueOf(a.getHostCard().getGameTimestamp()),"spell",a.isSpell(),"mana_ability",a.isManaAbility(),"land_ability",a.isLandAbility(),"targets_cards",sequence(a.getTargets().getTargetCards()),"targets",targets,"target_players",java.util.stream.StreamSupport.stream(a.getTargets().getTargetPlayers().spliterator(),false).map(this::seat).toList());if(optional()){projection.put("copied",a.isCopied());projection.put("trigger",a.isTrigger());projection.put("entry_label",entryName(a));projection.put("synthetic_copy_host",syntheticCopies.containsKey(a.getHostCard().getId()));}return projection;
        }
        Map<String,Object> view(CardView c){if(c==null)return null;String a=aliases.get(c.getId());if(a==null&&causalProfile()){Card actual=game.findByView(c);require(actual!=null,"unknown_token_view",m("id",c.getId()));a=alias(actual);}require(a!=null,"unknown_card_view",m("id",c.getId()));return m("physical_id",String.valueOf(c.getId()),"alias",a,"owner",seatView(c.getOwner()),"controller",seatView(c.getController()),"zone",c.getZone()==null?null:c.getZone().name().toLowerCase(Locale.ROOT));}
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
        boolean keep(Player p,Player first,int charge){Call c=enter(p,"mulliganKeepHand",m("first",seat(first),"charged",charge,"hand",sequence(p.getCardsIn(ZoneType.Hand))));require(first==seats.get(str(fixture,"starting_role"))&&game.getAge()==GameStage.Mulligan,"keep_context",m());JsonObject d=opening(p,"declaration");require(charge==number(d,"taken"),"mulligan_charge",m("actual",charge,"policy",d));boolean keep=str(d,"reply").equals("keep");require(keep||str(d,"reply").equals("mulligan"),"keep_reply",m());leave(c,m("keep",keep,"input_policy",d));return keep;}
        CardCollectionView bottom(Player p,CardCollectionView hand,int count){Call c=enter(p,"tuckCardsViaMulligan",m("domain",sequence(hand),"required",count));JsonObject d=opening(p,"bottom");List<String> wanted=strings(d.getAsJsonArray("aliases"));require(count==wanted.size()&&hand.size()==7&&seat(p).equals(str(fixture,"starting_role"))&&game.getAge()==GameStage.Mulligan,"bottom_context",m());observe("mulligan.bottom.before",c.sequence,true,m("policy",d));CardCollection out=new CardCollection();for(String a:wanted){require(hand.contains(card(a)),"bottom_domain",m("alias",a));out.add(card(a));}leave(c,m("selected",sequence(out),"input_policy",d));return out;}
        List<SpellAbility> choose(Player p){
            if(optional())require(resolutionPayment==null,"priority_inside_resolution_payment",m());
            Call c=enter(p,"chooseSpellAbilityToPlay",m("offered_domain","actual_priority_context"));return chooseAtCurrent(c,p);
        }
        void continueMandatory(Call c,Player p){require(callStack.peek()==c&&p==c.player,"annotation_callback",m());}
        List<SpellAbility> chooseAtCurrent(Call c,Player p){
            require(p==game.getPhaseHandler().getPriorityPlayer()&&game.getAge()==GameStage.Play&&turn()<=number(fixture.getAsJsonObject("watchdog"),"maximum_individual_turn"),"priority_context",m());priorityCount++;
            require(strings(fixture.getAsJsonObject("pass_policy").getAsJsonArray("allowed_steps")).contains(step()),"priority_step",m());
            once(prepared()?"prepared.first_ready":"opening.complete",c.sequence,true,m());once("priority."+turn()+"."+step(),c.sequence,true,m());settlementCuts();
            if(stopReady(p)){boundedStop=true;long seq=leave(c,m("kind","bounded_ready_exit","boundary_ref",fixture.getAsJsonObject("stop").get("boundary_ref")));observe("bounded.stop",seq,true,m("stop",fixture.get("stop")));throw new BoundedStop();}
            if(actionCursor<actions.size()){
                JsonObject a=actions.get(actionCursor);int at=number(a,"individual_turn");require(turn()<=at,"missed_action_turn",m("action",a));
                boolean stack=stackAliases().equals(strings(a.getAsJsonArray("expected_stack_bottom_first")));
                boolean context=turn()==at&&seat(p).equals(str(a,"seat"))&&step().equals(str(a,"step"))&&stack;
                if(context&&a.has("after_settled_action"))context=settled(str(a,"after_settled_action"));
                String kind=str(a,"kind");
                if(context&&causalProfile()&&List.of("seek","save_object","probe_mana").contains(kind)){
                    if(kind.equals("save_object")){String binding=str(a.getAsJsonObject("input_instruction"),"binding");require(savedObjects.putIfAbsent(binding,card(str(a,"alias")))==null,"duplicate_saved_object",m());}
                    if(kind.equals("probe_mana")){JsonObject instruction=a.getAsJsonObject("input_instruction");String key=str(a,"alias");Card source=key.startsWith("saved:")?savedObjects.get(key.substring(6)):card(key);require(source!=null,"probe_source",m());FCollectionView<SpellAbility> options=source.getManaAbilities();require(options.size()==1,"probe_mana_domain",m());SpellAbility mana=options.get(0);mana.setActivatingPlayer(seats.get(str(instruction.getAsJsonObject("point"),"actor")));boolean legal=mana.canPlay();add("operations","mana_legality_probe",m("policy",instruction,"ability",ability(mana),"can_play",legal,"current_source",cardState(source),"native_only_typed_rejection",true));}
                    actionCursor++;add("operations","input_annotation",m("policy",a,"context",header()));continueMandatory(c,p);return chooseAtCurrent(c,p);
                }
                if(context&&!kind.startsWith("declare_")){
                    if(kind.equals("pass")){actionCursor++;long seq=leave(c,m("kind","scripted_pass","policy",a));observe("action."+(actionCursor-1),seq,true,m("action",a));return null;}
                    selectedAction=a;Card source=card(str(a,"alias"));
                    if(kind.equals("play_land")){require(source.isInZone(ZoneType.Hand),"land_source",m());selected=new LandAbility(source,source.getCurrentState());}
                    else if(kind.equals("activate_mana")){List<SpellAbility> opts=new ArrayList<>();for(SpellAbility sa:source.getManaAbilities())if(sa.isManaAbility()&&sa.getManaPart().getOrigProduced().equals(causalProfile()?str(a,"mana_color"):seat(p).equals("U")?"U":"G"))opts.add(sa);require(opts.size()==1,"fixed_mana_domain",m());selected=opts.get(0);}
                    else if(kind.equals("cast")){require(source.isInZone(ZoneType.Hand)&&!source.isLand(),"spell_source",m());selected=source.getFirstSpellAbility();}
                    else throw bad("action_kind",m("kind",kind));
                    selected.setActivatingPlayer(p);require(selected.canPlay(),"chosen_action_legality",m("action",a));leave(c,m("kind","chosen","policy",a,"ability",ability(selected)));return List.of(selected);
                }
                if(turn()==at&&p==game.getPhaseHandler().getPlayerTurn()&&game.getStack().isEmpty()&&!kind.startsWith("declare_"))require(phaseRank(step())<=phaseRank(str(a,"step")),"missed_action_step",m("action",a));
            }
            for(SpellAbilityStackInstance si:game.getStack())require(si.getSpellAbility().isSpell()&&(castActions.containsKey(si.getSpellAbility().getId())||optional()&&copyNames.containsKey(si.getSpellAbility().getId()))||causalProfile()&&si.getSpellAbility().isTrigger()&&triggerNames.containsKey(si.getSpellAbility().getId()),"unplanned_stack",m());leave(c,m("kind","pass"));return null;
        }
        int phaseRank(String s){return List.of("untap","upkeep","draw","precombat_main","beginning_combat","declare_attackers","declare_blockers","first_strike_damage","combat_damage","end_combat","postcombat_main","end","cleanup").indexOf(s);}
        boolean play(Player p,SpellAbility sa){
            Call c=enter(p,"playChosenSpellAbility",m("ability",ability(sa),"policy",selectedAction));require(sa==selected&&selectedAction==actions.get(actionCursor),"selected_identity",m());
            ordinaryOperation=id+".ordinary."+(++ordinaryIdentity);JsonObject a=selectedAction;String kind=str(a,"kind");int stackBefore=game.getStack().size(),landsBefore=p.getLandsPlayedThisTurn();
            add("operations","ordinary_entry",m("operation_id",ordinaryOperation,"policy",a,"ability",ability(sa),"context",header(),"stack_before",world().get("stack")));
            boolean result=PlaySpellAbility.playSpellAbility(p.getController(),p,sa);require(result,"play_false",m("action",a));Card source=card(str(a,"alias"));
            if(kind.equals("play_land"))require(source.isInPlay()&&!source.isTapped()&&p.getLandsPlayedThisTurn()==landsBefore+1&&game.getStack().size()==stackBefore,"land_admission",m());
            else if(kind.equals("activate_mana")){
                require(source.isTapped()&&game.getStack().size()==stackBefore,"mana_admission",m());Collection<Mana> actual=sa.getManaPart().getLastManaProduced();require(actual.size()==1,"mana_produced_domain",m());Mana mana=actual.iterator().next();require(mana.getSourceCard().getId()==source.getId()&&mana.getColor()==(causalProfile()?ManaAtom.fromName(str(a,"mana_color")):seat(p).equals("U")?ManaAtom.BLUE:ManaAtom.GREEN)&&mana.getPlayer()==p&&!mana.isSnow(),"mana_source",m());produced.put(str(a,"alias"),new Produced(mana,str(a,"alias"),ordinaryOperation,source.getGameTimestamp()));
            }else {
                SpellAbilityStackInstance si=game.getStack().getInstanceMatchingSpellAbilityID(sa);require(source.isInZone(ZoneType.Stack)&&game.getStack().size()==stackBefore+1&&si!=null,"cast_admission",m());String action=str(a,"policy_id");require(!castAbilities.containsKey(action),"duplicate_cast_operation",m());castAbilities.put(action,sa);castActions.put(sa.getId(),action);
                castOperations.add(m("action_id",action,"operation_id",ordinaryOperation,"ability_id",sa.getId(),"stack_id",si.getId(),"source",str(a,"alias"),"source_physical_id",String.valueOf(source.getId()),"source_timestamp",String.valueOf(source.getGameTimestamp()),"payer",seat(p),"payer_player_id",p.getId(),"required_cost",str(a,"required_cost"),"paid_sources",a.get("paid_sources"),"actual_ability",ability(sa),"stack_bottom_first",stackAliases(),"context",header()));
            }
            actionCursor++;long seq=leave(c,m("result",result,"policy",a,"actual_ability",ability(sa)));add("operations","ordinary_return",m("operation_id",ordinaryOperation,"callback_return_sequence",seq,"policy",a,"context",header()));observe("action."+(actionCursor-1),seq,true,m("action",a));selected=null;selectedAction=null;ordinaryOperation=null;return result;
        }
        SpellAbility select(Player p,Card host,List<SpellAbility> options,ITriggerEvent event){Call c=enter(p,"getAbilityToPlay",m("host",alias(host),"domain",options.stream().map(this::ability).toList(),"trigger_event",event==null?null:event.getClass().getName()));callerPath(c,"forge.game.player.PlayerController.getAbilityToPlay","forge.game.player.PlaySpellAbility.chooseOptionalAdditionalCosts");require(event==null&&options.size()==1&&options.get(0)==selected&&host.getId()==selected.getHostCard().getId(),"ability_selection_domain",m());leave(c,ability(selected));return selected;}
        CostDecisionMakerBase costs(Player p,Player payer,SpellAbility sa,boolean effect,String prompt){Call c=enter(p,"getCostDecisionMaker",m("payer",seat(payer),"ability",ability(sa),"effect",effect,"prompt",prompt,"original_cost_parts",diagnostic(sa.getPayCosts().getCostParts(),0)));callerPath(c,"forge.game.player.PlayerController.getCostDecisionMaker","forge.game.player.PlaySpellAbility.playAbility");require(p==payer&&sa==selected&&(causalProfile()&&sa.isTrigger()?effect&&sa.getPayCosts().getCostParts().size()==1&&sa.getPayCosts().getCostParts().get(0) instanceof CostPartMana zero&&zero.getManaCostFor(sa).isZero():!effect&&sa.getPayCosts().getCostParts().size()==1),"cost_context",m());StrictCosts out=new StrictCosts(p,sa,this);long returnSequence=leave(c,m("visitor",objectRef(out),"payer",seat(payer),"effect",effect,"ability",ability(sa),"source",alias(sa.getHostCard())));if(optional()&&fundingInstruction!=null){require(fundingCostCallback==null,"one_funding_cost_callback",m());fundingCostCallback=c;fundingCostReturnSequence=returnSequence;}return out;}
        Map<String,Object> costScope(CostPart part,SpellAbility sa){
            CostPaymentStack.Entry entry=game.costPaymentStack.peek();List<CostPart> original=entry==null?List.of():entry.payment().getCost().getCostParts();String origin="unrecognized";
            if(original.size()==1){CostPart first=original.get(0);if(first instanceof CostTap&&part instanceof CostTap)origin="original";else if(first instanceof CostTap&&part instanceof CostPartMana mana&&mana.getManaCostFor(sa).isZero())origin="implicit_zero_mana";else if(first instanceof CostPartMana&&part instanceof CostPartMana)origin="original";}
            return m("part_origin",origin,"actual_part",objectRef(part),"payment",entry==null?null:objectRef(entry.payment()),"stack_part_is_actual_part",entry!=null&&entry.cost()==part,"payment_ability_is_selected",entry!=null&&entry.payment().getAbility()==sa&&sa==selected,"original_cost_parts",diagnostic(original,0),"original_part_identity_match",original.contains(part));
        }
        void validateCostScope(CostPart part,SpellAbility sa,String origin){CostPaymentStack.Entry entry=game.costPaymentStack.peek();Map<String,Object> scope=costScope(part,sa);require(entry!=null&&entry.cost()==part&&entry.payment().getAbility()==sa&&sa==selected&&origin.equals(scope.get("part_origin")),"cost_part_scope",scope);}
        boolean pay(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){if(optional()&&resolutionPayment!=null&&sa==resolutionPayment.ability)return payResolutionMana(p,cost,part,sa,prompt,matrix,effect);Call c=enter(p,"payManaCost",m("cost",cost.toString(),"ability",ability(sa),"payer",seat(p),"effect",effect,"prompt",prompt,"part",part.getClass().getName(),"scope",costScope(part,sa),"matrix",objectRef(matrix),"matrix_is_current_payment",game.costPaymentStack.peek()!=null&&matrix==game.costPaymentStack.peek().payment()));callerPath(c,"forge.game.player.PlayerController.payManaCost","forge.game.cost.CostPartMana.payAsDecided");validateCostScope(part,sa,sa.isManaAbility()?"implicit_zero_mana":"original");require(sa==selected&&!effect&&matrix==game.costPaymentStack.peek().payment()&&(sa.isManaAbility()?cost.isZero():cost.toString().equals(str(selectedAction,"required_cost"))),"mana_cost_context",m());boolean result=PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,effect);require(result,"mana_cost_failed",m());leave(c,m("result",result,"pool",pool(p),"scope",costScope(part,sa)));return result;}
        Map<String,Object> token(Produced t){return m("production_operation_id",t.operation,"output_index",0,"source",t.source,"source_timestamp",String.valueOf(t.timestamp),"color",causalProfile()?switch(t.mana.getColor()){case ManaAtom.WHITE->"white";case ManaAtom.BLUE->"blue";case ManaAtom.BLACK->"black";case ManaAtom.RED->"red";case ManaAtom.GREEN->"green";default->throw bad("mana_color",m());}:t.mana.getColor()==ManaAtom.BLUE?"blue":"green");}
        boolean apply(Player p,ManaCostBeingPaid cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){
            if(optional()&&resolutionPayment!=null&&sa==resolutionPayment.ability)return applyResolutionMana(p,cost,sa,prompt,matrix,effect);
            Call c=enter(p,"applyManaToCost",m("ability",ability(sa),"payer",seat(p),"effect",effect,"prompt",prompt,"unpaid",cost.toString(),"pool",pool(p),"matrix",objectRef(matrix)));
            callerPath(c,"forge.game.player.PlaySpellAbility.payManaCost");
            require(sa==selected&&sa.isSpell()&&!effect&&game.costPaymentStack.peek()!=null&&matrix==game.costPaymentStack.peek().payment()&&cost.toManaCost().toString().equals(str(selectedAction,"required_cost")),"apply_context",m());
            int amount=number(selectedAction,"mana_units_paid");List<Produced> tokens=new ArrayList<>();
            for(String alias:strings(selectedAction.getAsJsonArray("paid_sources"))){Produced t=produced.get(alias);require(t!=null&&t.mana.getPlayer()==p,"named_token_domain",m("source",alias));tokens.add(t);}
            require(tokens.size()==amount,"paid_source_count",m());List<Object> spent=new ArrayList<>();p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);
            try{for(Produced t:tokens){require(t.mana.meetsManaRestrictions(sa)&&sa.allowsPayingWithShard(t.mana.getSourceCard(),t.mana.getColor()),"token_restriction",m());String before=cost.toString();require(p.getManaPool().tryPayCostWithMana(sa,cost,t.mana,false),"named_token_debit",token(t));sa.getPayingMana().add(t.mana);spent.add(m("token",token(t),"unpaid_before",before,"unpaid_after",cost.toString(),"pool_after",pool(p)));}require(cost.isPaid()&&sa.getPayingMana().size()==amount,"paid_completion",m());}
            finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}
            leave(c,m("result",true,"spent",spent,"paid",cost.isPaid(),"paying_mana",tokens.stream().map(this::token).toList()));return true;
        }
        boolean actualResolving(SpellAbilityStackInstance si){return game.getStack().isResolving()&&!game.getStack().isEmpty()&&game.getStack().peekAbility().getId()==si.getSpellAbility().getId()&&game.getStack().isResolving(si.getSourceCard());}
        String entryRef(JsonObject ref){return switch(str(ref,"kind")){case "cast_entry"->str(ref,"operation");case "trigger_entry"->str(ref,"trigger");case "copy_entry"->str(ref,"copy");default->throw bad("optional_entry_reference",m("reference",ref));};}
        String entryName(SpellAbility sa){
            String name=copyNames.get(sa.getId());if(name==null)name=triggerNames.get(sa.getId());if(name==null)name=castActions.get(sa.getId());
            if(name==null&&selected==sa&&selectedAction!=null&&selectedAction.has("policy_id"))name=str(selectedAction,"policy_id");return name;
        }
        SpellAbility entryAbility(String label){
            SpellAbility sa=castAbilities.get(label);if(sa==null)sa=copyAbilities.get(label);
            if(sa==null)for(SpellAbilityStackInstance si:game.getStack())if(label.equals(entryName(si.getSpellAbility()))){require(sa==null,"duplicate_entry_label",m("label",label));sa=si.getSpellAbility();}
            require(sa!=null,"missing_entry_ability",m("label",label));return sa;
        }
        JsonObject consumePlan(String kind,String entry,Player actor){
            require(optional()&&entry!=null,"optional_plan_context",m("kind",kind,"entry",entry));List<JsonObject> matches=new ArrayList<>();
            for(JsonObject plan:objects(fixture.getAsJsonArray("decision_plans")))if(!decisionPlanConsumption.contains(str(plan,"id"))&&kind.equals(str(plan,"kind"))&&entry.equals(entryRef(plan.getAsJsonObject("entry")))&&seat(actor).equals(str(plan,kind.equals("payment")?"payer":"chooser")))matches.add(plan);
            require(matches.size()==1,"optional_unique_plan",m("kind",kind,"entry",entry,"actor",seat(actor),"matches",matches.size()));JsonObject plan=matches.get(0);
            require(new HashSet<>(decisionPlanConsumption).size()==decisionPlanConsumption.size(),"duplicate_optional_plan",m());decisionPlanConsumption.add(str(plan,"id"));decisionPlanCursor++;
            add("operations","decision_plan_consumed",m("plan_id",str(plan,"id"),"plan_kind",kind,"entry_label",entry,"actor",seat(actor),"callback_id",callStack.isEmpty()?null:callStack.peek().id));return plan;
        }
        Map<String,Object> choiceReceipt(JsonObject plan,String entry,SpellAbility sa,Player actor,Call call){return m("plan_id",str(plan,"id"),"entry_label",entry,"actual_ability",ability(sa),"actor",seat(actor),"callback_id",call.id,"entry_sequence",call.sequence);}
        boolean optionalTrigger(Player p,WrappedAbility sa){
            Call call=enter(p,"confirmTrigger",m("ability",ability(sa),"wrapped_ability",ability(sa.getWrappedAbility()),"decider",seat(sa.getDecider()),"triggering_objects",diagnostic(sa.getTriggeringObjects(),0)));
            callerPath(call,"forge.game.trigger.WrappedAbility.resolve","forge.game.ability.AbilityUtils.resolveApiAbility","forge.game.ability.AbilityUtils.resolve","forge.game.zone.MagicStack.resolveStack");
            String name=entryName(sa);require(name!=null&&name.equals(resolvingEntry())&&sa.isOptionalTrigger()&&sa.getDecider()==p&&p==sa.getActivatingPlayer()&&game.getStack().isResolving(sa.getHostCard())&&List.of("Soul's Attendant","Oculus").contains(sa.getHostCard().getName()),"optional_trigger_owner",m());
            JsonObject plan=consumePlan("optional",name,p);require(strings(plan.getAsJsonArray("options")).equals(List.of("take","decline")),"optional_boolean_domain",m());boolean result=str(plan,"choice").equals("take");require(result||str(plan,"choice").equals("decline"),"optional_boolean_reply",m());
            Map<String,Object> row=choiceReceipt(plan,name,sa,p,call);row.put("kind","trigger_optional");row.put("choice",result?"take":"decline");row.put("before_world",world());row.put("after_world",world());row.put("return_sequence",leave(call,m("result",result,"plan_id",str(plan,"id"),"actual_ability",ability(sa))));optionalChoices.add(row);return result;
        }
        boolean optionalDraw(Player p,SpellAbility sa,PlayerActionConfirmMode mode,String message,List<String> options,Card cardToShow,Map<String,Object> params){
            Call call=enter(p,"confirmAction",m("ability",ability(sa),"mode",mode==null?null:mode.name(),"message",message,"options",options,"card_to_show",diagnostic(cardToShow,0),"params",diagnostic(params,0)));
            callerPath(call,"forge.game.player.PlayerController.confirmAction","forge.game.ability.effects.DrawEffect.resolve");String name=entryName(sa);
            require(name!=null&&Boolean.FALSE.equals(paymentResults.get(name))&&resolutionPayment==null&&sa==selected&&sa.getHostCard().getName().equals("Rhystic Study")&&sa.getActivatingPlayer()==p&&"You".equals(sa.getParam("Defined"))&&"1".equals(sa.getParam("NumCards"))&&"You".equals(sa.getParam("OptionalDecider"))&&mode==null&&options.isEmpty()&&cardToShow==null&&params==null,"rhystic_unpaid_draw_callback",m());
            JsonObject plan=consumePlan("optional",name,p);require(strings(plan.getAsJsonArray("options")).equals(List.of("take","decline")),"optional_boolean_domain",m());boolean result=str(plan,"choice").equals("take");require(result||str(plan,"choice").equals("decline"),"optional_boolean_reply",m());
            Map<String,Object> row=choiceReceipt(plan,name,sa,p,call);row.put("kind","draw_optional");row.put("choice",result?"take":"decline");row.put("before_world",world());row.put("after_world",world());row.put("return_sequence",leave(call,m("result",result,"plan_id",str(plan,"id"),"actual_ability",ability(sa))));optionalChoices.add(row);return result;
        }
        Map<String,Object> quote(Cost cost,SpellAbility sa){
            require(cost.getCostParts().size()==1&&cost.getCostParts().get(0) instanceof CostPartMana,"resolution_mana_only_quote",m("actual_cost",diagnostic(cost,0)));
            CostPartMana part=(CostPartMana)cost.getCostParts().get(0);ManaCost mana=part.getManaCostFor(sa);List<String> required=new ArrayList<>();for(ManaCostShard shard:mana)required.add(shard.toString());
            return m("generic",String.valueOf(mana.getGenericCost()),"required",required,"actual_cost",cost.toString(),"actual_mana_cost",mana.toString(),"actual_part",objectRef(part));
        }
        boolean preventEffect(Player p,Cost cost,SpellAbility sa,boolean alreadyPaid,FCollectionView<Player> allPayers){
            Call call=enter(p,"payCostToPreventEffect",m("cost",diagnostic(cost,0),"ability",ability(sa),"already_paid",alreadyPaid,"all_payers",allPayers.stream().map(this::seat).toList()));
            callerPath(call,"forge.game.ability.AbilityUtils.handleUnlessCost","forge.game.ability.AbilityUtils.resolveApiAbility","forge.game.ability.AbilityUtils.resolve");String name=entryName(sa);
            require(name!=null&&name.equals(resolvingEntry())&&!alreadyPaid&&allPayers.size()==1&&allPayers.get(0)==p&&resolutionPayment==null&&fundingInstruction==null,"resolution_offer_owner",m());
            String source=sa.getHostCard().getName();require(source.equals("Mana Leak")||source.equals("Rhystic Study"),"resolution_offer_source",m("source",source));
            if(source.equals("Mana Leak")){require(Iterables.size(sa.getTargets().getTargetSpells())==1&&sa.getTargets().getFirstTargetedSpell().getActivatingPlayer()==p,"mana_leak_actual_target_payer",m());}
            else require(sa.getTriggeringObject(AbilityKey.Activator)==p&&sa.getActivatingPlayer()!=p&&"TriggeredActivator".equals(sa.getParam("UnlessPayer")),"rhystic_actual_cast_payer",m("triggering_objects",diagnostic(sa.getTriggeringObjects(),0)));
            JsonObject plan=consumePlan("payment",name,p);Map<String,Object> actualQuote=quote(cost,sa);JsonObject plannedQuote=plan.getAsJsonObject("cost");
            require(actualQuote.get("generic").equals(str(plannedQuote,"generic"))&&JSON.toJson(actualQuote.get("required")).equals(JSON.toJson(plannedQuote.get("required")))&&actualQuote.get("generic").equals(source.equals("Mana Leak")?"3":"1"),"resolution_quote",m("actual",actualQuote,"plan_id",str(plan,"id")));
            Map<String,Object> row=choiceReceipt(plan,name,sa,p,call);row.put("quote",actualQuote);row.put("already_paid",alreadyPaid);row.put("all_payers",allPayers.stream().map(this::seat).toList());row.put("before_world",world());row.put("pool_before",pool(p));
            ResolutionPayment context=new ResolutionPayment(plan,sa,p,cost);resolutionPayment=context;Player activator=sa.getActivatingPlayer();List<String> funding=new ArrayList<>();boolean paid=false;
            try {
                for(JsonObject instruction:objects(plan.getAsJsonArray("funding")))funding.add(activateForPayment(p,context,instruction,funding.size()));
                row.put("pool_before_debit",pool(p));JsonObject outcome=plan.getAsJsonObject("outcome");
                if(str(outcome,"kind").equals("pay")){
                    context.payment=new CostPayment(cost,sa);paid=context.payment.payCost(new ResolutionCosts(p,sa,this));
                    require(paid&&context.payment.isFullyPaid()&&!context.spent.isEmpty(),"resolution_real_payment_completion",m("payment",objectRef(context.payment),"spent",context.spent));
                }else require(str(outcome,"kind").equals("decline"),"resolution_outcome_input",m());
                require(sa.getActivatingPlayer()==activator&&game.costPaymentStack.peek()==null,"resolution_payment_cleanup",m());
            }finally{resolutionPayment=null;}
            require(paymentResults.putIfAbsent(name,paid)==null,"duplicate_resolution_offer",m("entry",name));row.put("funding_operations",funding);row.put("outcome",paid?"paid":"declined");row.put("pool_after",pool(p));row.put("spent",context.spent);row.put("cost_payment_identity",objectRef(context.payment));row.put("effect",context.payment==null?null:true);row.put("after_world",world());
            row.put("return_sequence",leave(call,m("result",paid,"plan_id",str(plan,"id"),"payment",objectRef(context.payment),"actual_ability",ability(sa))));resolutionPayments.add(row);return paid;
        }
        Map<String,Object> resolutionScope(CostPart part,SpellAbility sa){CostPaymentStack.Entry entry=game.costPaymentStack.peek();ResolutionPayment context=resolutionPayment;return m("actual_part",objectRef(part),"payment",entry==null?null:objectRef(entry.payment()),"stack_part_is_actual_part",entry!=null&&entry.cost()==part,"payment_ability_is_actual_offer",entry!=null&&context!=null&&entry.payment().getAbility()==sa&&sa==context.ability,"payment_is_offer_payment",entry!=null&&context!=null&&entry.payment()==context.payment,"quote_identity",context==null?null:objectRef(context.quote));}
        void validateResolutionScope(CostPart part,SpellAbility sa,Player p){
            CostPaymentStack.Entry entry=game.costPaymentStack.peek();ResolutionPayment context=resolutionPayment;
            require(context!=null&&context.payer==p&&context.ability==sa&&entry!=null&&entry.cost()==part&&entry.payment()==context.payment&&context.payment.getAbility()==sa&&context.payment.getCost()==context.quote&&context.quote.getCostParts().contains(part)&&part instanceof CostPartMana,"resolution_cost_part_scope",resolutionScope(part,sa));
        }
        boolean payResolutionMana(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){
            Call call=enter(p,"payManaCost",m("cost",cost.toString(),"ability",ability(sa),"payer",seat(p),"effect",effect,"prompt",prompt,"scope",resolutionScope(part,sa),"matrix",objectRef(matrix)));
            callerPath(call,"forge.game.player.PlayerController.payManaCost","forge.game.cost.CostPartMana.payAsDecided");validateResolutionScope(part,sa,p);
            require(effect&&matrix==resolutionPayment.payment&&cost.toString().equals(part.getManaCostFor(sa).toString()),"resolution_mana_cost_context",m());boolean result=PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,true);require(result,"resolution_mana_owner_failed",m());leave(call,m("result",result,"pool",pool(p),"scope",resolutionScope(part,sa)));return result;
        }
        boolean applyResolutionMana(Player p,ManaCostBeingPaid cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){
            Call call=enter(p,"applyManaToCost",m("ability",ability(sa),"payer",seat(p),"effect",effect,"prompt",prompt,"unpaid",cost.toString(),"pool",pool(p),"matrix",objectRef(matrix)));
            callerPath(call,"forge.game.player.PlaySpellAbility.payManaCost");ResolutionPayment context=resolutionPayment;CostPaymentStack.Entry entry=game.costPaymentStack.peek();
            require(context!=null&&p==context.payer&&sa==context.ability&&effect&&entry!=null&&matrix==context.payment&&entry.payment()==context.payment&&entry.cost() instanceof CostPartMana&&cost.toManaCost().toString().equals(((CostPartMana)entry.cost()).getManaCostFor(sa).toString()),"resolution_apply_context",m());validateResolutionScope(entry.cost(),sa,p);
            List<Produced> tokens=new ArrayList<>();Set<Mana> used=Collections.newSetFromMap(new IdentityHashMap<>());
            for(JsonObject allocation:objects(context.plan.getAsJsonObject("outcome").getAsJsonArray("allocation"))){byte color=MagicColor.fromName(str(allocation,"mana_type"));int amount=Integer.parseInt(str(allocation,"amount"));require(amount>0,"resolution_allocation_amount",m());
                for(int i=0;i<amount;i++){Produced found=null;for(Mana mana:p.getManaPool())if(mana.getColor()==color&&!used.contains(mana)){for(Produced t:produced.values())if(t.mana==mana){found=t;break;}if(found!=null)break;}require(found!=null&&used.add(found.mana),"resolution_actual_token_domain",m("color",str(allocation,"mana_type")));tokens.add(found);}}
            p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);
            try{for(Produced t:tokens){require(t.mana.getPlayer()==p&&t.mana.meetsManaRestrictions(sa)&&sa.allowsPayingWithShard(t.mana.getSourceCard(),t.mana.getColor()),"resolution_token_restriction",m());String before=cost.toString();require(p.getManaPool().tryPayCostWithMana(sa,cost,t.mana,false),"resolution_actual_token_debit",token(t));sa.getPayingMana().add(t.mana);context.spent.add(m("token",token(t),"unpaid_before",before,"unpaid_after",cost.toString(),"pool_after",pool(p)));}require(cost.isPaid()&&sa.getPayingMana().size()==tokens.size(),"resolution_paid_completion",m());}
            finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}
            leave(call,m("result",true,"spent",context.spent,"paid",cost.isPaid(),"paying_mana",tokens.stream().map(this::token).toList()));return true;
        }
        String activateForPayment(Player p,ResolutionPayment parent,JsonObject instruction,int ordinal){
            require(resolutionPayment==parent&&fundingInstruction==null,"payment_funding_nesting",m());String sourceAlias=objectAlias(instruction.getAsJsonObject("source"));Card source=card(sourceAlias);FCollectionView<SpellAbility> options=source.getManaAbilities();
            require(options.size()==1&&source.getController()==p&&source.isInPlay()&&!source.isTapped(),"payment_funding_source",m("source",sourceAlias));SpellAbility mana=options.get(0);mana.setActivatingPlayer(p);boolean color=str(instruction,"kind").equals("color");require(color?str(instruction,"amount").equals("1"):instruction.getAsJsonArray("produced").size()==1&&str(instruction.getAsJsonArray("produced").get(0).getAsJsonObject(),"amount").equals("1"),"payment_funding_single_output_input",m());
            require(mana.isManaAbility()&&mana.canPlay()&&(color?source.getName().equals("Manalith")&&mana.getManaPart().isAnyMana():str(instruction,"kind").equals("fixed")&&!mana.getManaPart().isAnyMana()),"payment_funding_ability",m("ability",ability(mana)));
            SpellAbility oldSelected=selected;JsonObject oldAction=selectedAction;String oldOperation=ordinaryOperation;String operation=str(parent.plan,"id")+".funding."+ordinal;String outerEntry=resolvingEntry();List<String> stackBefore=stackAliases();Map<String,Object> beforeWorld=world(),poolBefore=pool(p);long entrySequence=add("operations","payment_mana_entry",m("operation_id",operation,"parent_plan_id",str(parent.plan,"id"),"ability",ability(mana),"source",sourceAlias,"before_world",beforeWorld));
            selected=mana;selectedAction=new JsonObject();selectedAction.addProperty("policy_id",operation);selectedAction.addProperty("required_cost","{0}");ordinaryOperation=operation;fundingInstruction=instruction;fundingColorCallback=null;fundingCostCallback=null;fundingCostReturnSequence=null;fundingPoolAtEntry=poolBefore;fundingNotifications.clear();
            Long colorCallback,activationReturn;Call activationCallback;List<Object> actualTokens=new ArrayList<>();
            try{require(PlaySpellAbility.playSpellAbility(p.getController(),p,mana),"payment_real_mana_activation",m());require(source.isTapped()&&stackAliases().equals(stackBefore)&&Objects.equals(resolvingEntry(),outerEntry),"payment_mana_completion_context",m());Collection<Mana> actual=mana.getManaPart().getLastManaProduced();require(actual.size()==1&&(color?fundingColorCallback!=null&&fundingNotifications.size()==1:fundingColorCallback==null&&fundingNotifications.isEmpty()),"payment_actual_production_count",m());
                Mana output=actual.iterator().next();String wanted=color?str(instruction,"choice"):str(instruction.getAsJsonArray("produced").get(0).getAsJsonObject(),"mana_type");require(output.getSourceCard().getId()==source.getId()&&output.getPlayer()==p&&output.getColor()==MagicColor.fromName(wanted)&&!output.isSnow(),"payment_actual_production_identity",m());boolean inPool=false;for(Mana existing:p.getManaPool())if(existing==output)inPool=true;require(inPool,"payment_produced_token_pool_membership",m());Produced receipt=new Produced(output,sourceAlias,operation,source.getGameTimestamp());produced.put(sourceAlias,receipt);actualTokens.add(token(receipt));colorCallback=fundingColorCallback;require(fundingCostCallback!=null&&fundingCostReturnSequence!=null,"payment_real_activation_callback",m());activationCallback=fundingCostCallback;activationReturn=fundingCostReturnSequence;
            }finally{selected=oldSelected;selectedAction=oldAction;ordinaryOperation=oldOperation;fundingInstruction=null;fundingColorCallback=null;fundingCostCallback=null;fundingCostReturnSequence=null;fundingPoolAtEntry=null;}
            long exitSequence=add("operations","payment_mana_return",m("operation_id",operation,"parent_plan_id",str(parent.plan,"id"),"ability",ability(mana),"produced",actualTokens,"after_world",world()));
            manaActivations.add(m("plan_id",str(parent.plan,"id"),"entry_label",outerEntry,"actual_ability",ability(mana),"actor",seat(p),"callback_id",activationCallback.id,"entry_sequence",activationCallback.sequence,"return_sequence",activationReturn,"parent_callback_id",callStack.peek().id,"operation_entry_sequence",entrySequence,"operation_return_sequence",exitSequence,"operation_id",operation,"parent_plan_id",str(parent.plan,"id"),"source",sourceAlias,"before_world",beforeWorld,"after_world",world(),"pool_before",poolBefore,"pool_after",pool(p),"color_callback_id",colorCallback,"produced",actualTokens));return operation;
        }
        byte paymentColor(Player p,String message,SpellAbility sa,ColorSet colors){
            Call call=enter(p,"chooseColor",m("message",message,"ability",ability(sa),"color_mask",colors.getColor(),"pool",pool(p),"before_world",world(),"resolving_entry_label",resolvingEntry()));callerPath(call,"forge.game.ability.effects.ManaEffect.resolve");
            require(resolutionPayment!=null&&resolutionPayment.payer==p&&fundingInstruction!=null&&str(fundingInstruction,"kind").equals("color")&&selected==sa&&sa.isManaAbility()&&sa.getActivatingPlayer()==p&&sa.getHostCard().getController()==p&&sa.getHostCard().isTapped()&&sa.getHostCard().getName().equals("Manalith")&&sa.getManaPart().isAnyMana()&&colors.getColor()==ColorSet.WUBRG.getColor()&&fundingColorCallback==null&&pool(p).equals(fundingPoolAtEntry),"payment_color_callback_context",m());
            require(strings(fundingInstruction.getAsJsonArray("colors")).equals(List.of("White","Blue","Black","Red","Green")),"payment_color_input_domain",m());byte color=MagicColor.fromName(str(fundingInstruction,"choice"));require(color!=0&&(color&(color-1))==0&&(colors.getColor()&color)==color,"payment_color_singleton",m());fundingColorCallback=call.id;
            leave(call,m("result",color,"symbol",MagicColor.toSymbol(color),"parent_plan_id",str(resolutionPayment.plan,"id"),"pool",pool(p),"actual_ability",ability(sa),"after_world",world()));return color;
        }
        void manaColorNotification(Player p,SpellAbility sa,GameObject relatedTarget,String value){
            Call call=enter(p,"notifyOfValue",m("ability",ability(sa),"related_target",diagnostic(relatedTarget,0),"value",value));callerPath(call,"forge.game.GameAction.notifyOfValue","forge.game.ability.effects.ManaEffect.resolve");
            require(resolutionPayment!=null&&fundingInstruction!=null&&str(fundingInstruction,"kind").equals("color")&&sa==selected&&sa.getHostCard()==relatedTarget&&fundingColorCallback!=null&&p!=resolutionPayment.payer&&p==seats.get(other(seat(resolutionPayment.payer)))&&MagicColor.toSymbol(MagicColor.fromName(str(fundingInstruction,"choice"))).equals(value)&&fundingNotifications.add(seat(p))&&pool(resolutionPayment.payer).equals(fundingPoolAtEntry),"mana_color_notification_context",m());
            leave(call,m("kind","void_ack","color_callback_id",fundingColorCallback,"parent_plan_id",str(resolutionPayment.plan,"id")));
        }
        void installCopies(Player p,List<SpellAbility> offered){
            require(resolutionPayment==null&&copyPlan==null&&offered.size()==1,"copy_callback_domain",m());SpellAbility copy=offered.get(0);require(copy.isCopied()&&copy.isSpell()&&copy.getActivatingPlayer()==p&&!aliases.containsKey(copy.getHostCard().getId()),"copy_actual_identity",m());
            // Alias only the synthetic Forge host. It never enters selected physical inventory.
            String temporary="forge-copy-host:"+copy.getId();aliases.put(copy.getHostCard().getId(),temporary);syntheticCopies.put(copy.getHostCard().getId(),temporary);
            Call call=enter(p,"orderAndPlaySimultaneousSa",m("domain",offered.stream().map(this::ability).toList()));callerPath(call,"forge.game.ability.effects.CopySpellAbilityEffect.resolve");
            String creator=resolvingEntry();SpellAbility owner=entryAbility(creator);require(owner.getHostCard().getName().equals("Twincast")&&owner.getActivatingPlayer()==p&&Iterables.size(owner.getTargets().getTargetSpells())==1,"twincast_copy_owner",m());
            JsonObject plan=consumePlan("copy_targets",creator,p);String originalLabel=entryRef(plan.getAsJsonObject("original"));SpellAbility original=entryAbility(originalLabel);require(owner.getTargets().getFirstTargetedSpell().getId()==original.getId()&&copy.getHostCard().getCopiedPermanent()!=null&&copy.getHostCard().getCopiedPermanent().getId()==original.getHostCard().getId()&&JSON.toJson(ability(copy).get("targets")).equals(JSON.toJson(ability(original).get("targets"))),"copy_actual_origin_targets",m());
            String label=str(plan,"bind_copy");require(!copyAbilities.containsKey(label)&&copyNames.putIfAbsent(copy.getId(),label)==null,"copy_unique_label",m());copyAbilities.put(label,copy);castAbilities.put(label,copy);copyTargetChoiceConsumed=false;copyPlan=plan;copyingAbility=copy;Map<String,Object> row=choiceReceipt(plan,creator,owner,p,call);Object beforeTargets=ability(copy).get("targets");row.put("before_world",world());pendingCopyReceipts.put(copy.getId(),row);
            try{if(!copy.getHostCard().isInZone(ZoneType.Stack))copy.setHostCard(game.getAction().moveToStack(copy.getHostCard(),copy));else game.getStackZone().add(copy.getHostCard());require(copy.isMayChooseNewTargets(),"copy_target_choice_flag",m());copy.setupNewTargets(p);game.getStack().add(copy);
                SpellAbilityStackInstance installed=game.getStack().getInstanceMatchingSpellAbilityID(copy);require((installed!=null||game.getStack().isFrozen())&&copyTargetChoiceConsumed&&Iterables.size(copy.getTargets().getTargetSpells())==1,"copy_real_stack_admission",m());
                row.put("copy_label",label);row.put("original_entry_label",originalLabel);row.put("copy_ability",ability(copy));if(installed!=null)row.put("copy_stack_id",installed.getId());row.put("stack_frozen_at_owner_return",game.getStack().isFrozen());row.put("copy_host",m("classification","forge_synthetic_copy_host","selected_physical",false,"state",cardState(copy.getHostCard()),"host_alias",alias(copy.getHostCard())));row.put("choice",str(plan.getAsJsonObject("choice"),"kind"));row.put("targets_before",beforeTargets);row.put("targets_after",ability(copy).get("targets"));row.put("after_owner_world",world());
            }finally{copyPlan=null;copyingAbility=null;}
            row.put("return_sequence",leave(call,m("copy",label,"actual_ability",ability(copy),"stack_bottom_first",stackAliases())));spellCopies.add(row);
        }
        TargetChoices copyTargets(Player p,SpellAbility sa,Predicate<GameObject> filter,boolean optionalChoice){
            Call call=enter(p,"chooseNewTargetsFor",m("ability",ability(sa),"filter_is_null",filter==null,"optional",optionalChoice,"before_world",world(),"actual_spell_domain",java.util.stream.StreamSupport.stream(game.getStack().spliterator(),false).filter(si->sa.canTarget(si.getSpellAbility())).map(si->entryName(si.getSpellAbility())).toList()));callerPath(call,"forge.game.spellability.SpellAbility.setupNewTargets");
            require(copyPlan!=null&&!copyTargetChoiceConsumed&&sa==copyingAbility&&sa.isCopied()&&sa.getActivatingPlayer()==p&&optionalChoice&&filter==null,"copy_new_targets_context",m());copyTargetChoiceConsumed=true;JsonObject choice=copyPlan.getAsJsonObject("choice");
            if(str(choice,"kind").equals("retain")){leave(call,m("result",null,"choice","retain","actual_ability",ability(sa),"after_world",world()));return null;}
            require(str(choice,"kind").equals("change")&&str(choice.getAsJsonObject("target"),"kind").equals("stack"),"copy_target_choice_input",m());String label=entryRef(choice.getAsJsonObject("target").getAsJsonObject("entry"));SpellAbility target=entryAbility(label);require(game.getStack().getInstanceMatchingSpellAbilityID(target)!=null&&sa.canTarget(target),"copy_changed_target_domain",m());TargetChoices targets=new TargetChoices();targets.add(target);sa.setTargets(targets);require(sa.isTargetNumberValid(),"copy_changed_target_count",m());leave(call,m("choice","change","target_entry",label,"actual_ability",ability(sa),"after_world",world()));return targets;
        }
        void classifyStackAddition(GameEventSpellAbilityCast event,Map<String,Object> payload){
            List<SpellAbilityStackInstance> found=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())if(si.getId()==event.si().getId()&&si.getSpellAbility().getId()==event.sa().getId())found.add(si);require(found.size()==1,"raw_stack_addition_actual_object",m());SpellAbility actual=found.get(0).getSpellAbility();
            String classification=actual.isCopied()?"copy_addition":actual.isTrigger()?"trigger_addition":actual.isSpell()?"spell_cast":"ability_activation";String entry=entryName(actual);require(entry!=null,"raw_stack_addition_entry",m("actual_ability",ability(actual)));
            Map<String,Object> receipt=m("classification",classification,"entry_label",entry,"actual_ability",ability(actual),"actual_stack_id",found.get(0).getId(),"actual_copied",actual.isCopied(),"actual_spell",actual.isSpell(),"actual_trigger",actual.isTrigger(),"actual_stack_frames",stackFrames());if(actual.isCopied()){Map<String,Object> copyReceipt=pendingCopyReceipts.get(actual.getId());require(copyReceipt!=null&&!copyReceipt.containsKey("copy_stack_add_sequence"),"copy_actual_install_event",m());copyReceipt.put("copy_stack_id",found.get(0).getId());copyReceipt.put("copy_stack_add_sequence",global+1);copyReceipt.put("installed_copy_ability",ability(actual));}rawStackAdditions.add(receipt);payload.put("actual_classification",receipt);
            if(actual.isSpell()&&!actual.isCopied())for(JsonObject op:objects(fixture.getAsJsonArray("causal_operations")))if(str(op,"kind").equals("cast_completed")&&entryRef(op.getAsJsonObject("entry")).equals(entry))causalEvents.computeIfAbsent(str(op,"name"),key->new ArrayList<>()).add(m("source_global_sequence",global+1,"subject",alias(actual.getHostCard()),"event","cast","caster",seat(actual.getActivatingPlayer()),"actual_ability",ability(actual),"raw_classification",classification));
        }

        JsonObject combatRecipe(){for(JsonObject c:objects(fixture.getAsJsonArray("combat")))if(str(c,"active_role").equals(seat(game.getPhaseHandler().getPlayerTurn()))&&number(c,"individual_turn")==turn())return c;return null;}

        Object defenderEntity(GameEntity target){
            if(target instanceof Player player)return m("kind","player","seat",seat(player),"player_id",player.getId());
            if(target instanceof Card card)return m("kind","card","card",view(card.getView()));
            throw bad("attack_defender_entity",m("class",target.getClass().getName()));
        }
        void attackers(Player p,Player active,Combat combat){
            List<GameEntity> offeredDefenders=new ArrayList<>();for(GameEntity target:combat.getDefenders())offeredDefenders.add(target);
            Call c=enter(p,"declareAttackers",m("active",seat(active),"eligible",domain(CombatUtil.getPossibleAttackers(active)),"defender_domain",offeredDefenders.stream().map(this::defenderEntity).toList(),"actual_combat_is_current",combat==game.getCombat()));
            Player opponent=seats.get(other(seat(p)));
            require(p==active&&combat==game.getCombat()&&combat.getAttackingPlayer()==active,"attack_actor_combat",m());
            require(offeredDefenders.size()==1&&offeredDefenders.get(0)==opponent,"attack_defender_domain",m("actual",offeredDefenders.stream().map(this::defenderEntity).toList()));
            JsonObject recipe=combatRecipe();List<String> chosen=recipe==null?List.of():strings(recipe.getAsJsonArray("attackers"));
            require(step().equals("declare_attackers"),"attack_step",m());
            for(String alias:chosen){Card attacker=card(alias);require(CombatUtil.canAttack(attacker,opponent),"attack_legality",m("source",alias));combat.addAttacker(attacker,opponent);}
            require(CombatUtil.validateAttackers(combat),"attack_validation",m());
            if(recipe!=null){require(assigned.add("attack:"+turn()),"duplicate_attack_recipe",m());if(!causalProfile())require(new HashSet<>(strings(recipe.getAsJsonArray("expected_eligible_attackers"))).equals(new HashSet<>(domain(CombatUtil.getPossibleAttackers(active)))),"eligible_attackers",m("actual",domain(CombatUtil.getPossibleAttackers(active)),"recipe",recipe));}
            if((prepared()&&!causalProfile())||causalProfile()&&recipe!=null){require(actionCursor<actions.size()&&str(actions.get(actionCursor),"kind").equals("declare_attackers"),"attack_action_cursor",m());declarationActions.put("attackers",actionCursor++);}
            leave(c,m("input_policy",recipe,"selected",chosen,"defenders",Collections.nCopies(chosen.size(),seat(opponent))));
        }
        void blockers(Player p,Player defender,Combat combat){Call c=enter(p,"declareBlockers",m("defender",seat(defender),"attackers",domain(combat.getAttackers()),"available_creatures",domain(p.getCreaturesInPlay())));
            JsonObject recipe=combatRecipe();require(recipe!=null&&str(recipe,"defender_role").equals(seat(p))&&p==defender&&step().equals("declare_blockers"),"block_context",m());
            List<Object> chosen=new ArrayList<>();for(JsonObject b:objects(recipe.getAsJsonArray("blockers"))){String source=str(b,"blocker"),target=str(b,"attacker");require(CombatUtil.canBlock(card(target),card(source),combat),"block_legality",m());combat.addBlocker(card(target),card(source));chosen.add(m("blocker",source,"attacker",target));}
            require(CombatUtil.validateBlocks(combat,p)==null&&assigned.add("block:"+turn()),"block_validation",m());if((prepared()||causalProfile())&&actionCursor<actions.size()&&str(actions.get(actionCursor),"kind").equals("declare_blockers")){declarationActions.put("blockers",actionCursor++);}leave(c,m("input_policy",recipe,"selected",chosen));
        }
        /** Retire a plan slot only when pinned Forge declared blockers without issuing its controller callback. */
        void retireUnissuedEmptyBlockers(long eventSequence,Map<String,Object> payload){
            if(!optional()||actionCursor>=actions.size()||!str(actions.get(actionCursor),"kind").equals("declare_blockers"))return;
            JsonObject action=actions.get(actionCursor),recipe=combatRecipe();Combat combat=game.getCombat();Player defender=seats.get(payload.get("defendingPlayer"));
            List<Map<String,Object>> frames=stackFrames();List<String> tags=tagsFromFrames(frames);
            require(Collections.frequency(tags,"forge.game.phase.PhaseHandler.declareBlockersTurnBasedAction")==1&&callStack.isEmpty(),"unissued_blockers_actual_event_source",m("actual_stack_frames",frames));
            require(recipe!=null&&combat!=null&&combat.getAttackingPlayer()==game.getPhaseHandler().getPlayerTurn()&&defender!=null&&combat.isPlayerAttacked(defender),"unissued_blockers_combat_context",m());
            require(turn()==number(action,"individual_turn")&&step().equals("declare_blockers")&&step().equals(str(action,"step"))&&seat(defender).equals(str(action,"seat"))&&seat(defender).equals(str(recipe,"defender_role"))&&stackAliases().equals(strings(action.getAsJsonArray("expected_stack_bottom_first"))),"unissued_blockers_plan_context",m("action",action,"context",header()));
            require(action.get("input_instruction").equals(recipe.get("input_instruction"))&&str(action,"policy_id").equals(str(recipe.getAsJsonObject("input_instruction"),"id")+".blockers")&&recipe.getAsJsonArray("blockers").size()==0,"unissued_blockers_exact_empty_plan",m("action",action,"recipe",recipe));
            List<String> attackers=domain(combat.getAttackersOf(defender));
            require(!attackers.isEmpty()&&attackers.equals(domain(combat.getAttackers()))&&attackers.equals(new TreeSet<>(strings(recipe.getAsJsonArray("attackers"))).stream().toList())&&assigned.contains("attack:"+turn()),"unissued_blockers_actual_positive_attackers",m("actual_attackers",attackers));
            long callbackEntries=streams.get("callbacks").stream().filter(row->"entry".equals(row.get("kind"))&&"declareBlockers".equals(row.get("method"))).filter(row->row.get("context") instanceof Map<?,?> context&&context.get("individual_turn") instanceof Number n&&n.intValue()==turn()).count();
            boolean canBlock=CombatUtil.canBlock(defender,combat);String validationError=CombatUtil.validateBlocks(combat,defender);
            require(callbackEntries==0&&!declarationActions.containsKey("blockers")&&!assigned.contains("block:"+turn())&&!canBlock&&combat.getAllBlockers().isEmpty()&&validationError==null,"unissued_blockers_empty_legal_domain",m("actual_callback_entries",callbackEntries,"actual_can_block",canBlock,"validation_error",validationError));
            int beforeCursor=actionCursor;JsonObject beforeWorld=JSON.toJsonTree(world()).getAsJsonObject();actionCursor++;JsonObject afterWorld=JSON.toJsonTree(world()).getAsJsonObject();
            JsonObject beforeComparable=beforeWorld.deepCopy(),afterComparable=afterWorld.deepCopy();beforeComparable.getAsJsonObject("header").remove("action_cursor");afterComparable.getAsJsonObject("header").remove("action_cursor");
            require(beforeComparable.equals(afterComparable),"unissued_blockers_no_observable_game_transition",m());
            Map<String,Object> receipt=m("status","SKIPPED_NOT_ISSUED","policy_id",str(action,"policy_id"),"input_instruction",action.get("input_instruction"),"planned_slot_index",beforeCursor,"plan_cursor_before",beforeCursor,"plan_cursor_after",actionCursor,"actual_event_sequence",eventSequence,"actual_event","GameEventBlockersDeclared","actual_event_payload",payload,"actual_defending_player",seat(defender),"actual_attackers",attackers,"actual_available_creatures",domain(defender.getCreaturesInPlay()),"actual_can_block",canBlock,"actual_validate_blocks_error",validationError,"actual_controller_callback_entries_this_turn",callbackEntries,"callback_id",null,"input_issued",false,"accepted_game_action",false,"game_transition_performed_by_adapter",false,"observable_world_unchanged_except_adapter_cursor",true,"before_world",beforeWorld,"after_world",afterWorld,"actual_stack_frames",frames);
            long seq=add("operations","planned_input_retirement",receipt);receipt.put("retirement_sequence",seq);plannedInputRetirements.add(receipt);observe("planned-slot."+beforeCursor+".not-issued",seq,true,m("status","SKIPPED_NOT_ISSUED","policy_id",str(action,"policy_id"),"actual_event_sequence",eventSequence,"accepted_game_action",false));
        }

        Map<Card,Integer> assign(Player p,Card source,CardCollectionView recipients,CardCollectionView remaining,int amount,GameEntity defender,boolean override){Call c=enter(p,"assignCombatDamage",m("source",alias(source),"recipients",sequence(recipients),"remaining",remaining==null?null:sequence(remaining),"amount",String.valueOf(amount),"defender",defender instanceof Player pl?seat(pl):null,"override_order",override));
            if(recipients.isEmpty()&&game.getCombat()!=null&&game.getCombat().isBlocked(source)){Map<Card,Integer> result=new LinkedHashMap<>();leave(c,m("assignments",List.of(),"blocked_without_current_blocker",true));return result;}
            JsonObject recipe=combatRecipe();require(recipe!=null&&step().equals("combat_damage")&&source.getController()==p&&recipients.size()==1&&amount==Math.max(0,source.getNetPower())&&assigned.add("damage:"+turn()+":"+alias(source)),"assignment_context",m());
            String expected=null;for(JsonObject b:objects(recipe.getAsJsonArray("blockers"))){if(str(b,"attacker").equals(alias(source)))expected=str(b,"blocker");if(str(b,"blocker").equals(alias(source)))expected=str(b,"attacker");}
            require(expected!=null&&recipients.contains(card(expected)),"assignment_recipient",m());Map<Card,Integer> result=new LinkedHashMap<>();result.put(card(expected),amount);leave(c,m("assignments",List.of(m("recipient",expected,"amount",String.valueOf(amount)))));return result;
        }
        CardCollectionView order(Player p,CardCollectionView cards,ZoneType dest,SpellAbility cause){Call c=enter(p,"orderMoveToZoneList",m("offered_sequence",sequence(cards),"destination",dest.name(),"cause",cause==null?null:ability(cause)));JsonObject d=atDecision(p,"order_graveyard");require(dest==ZoneType.Graveyard&&cause==null,"graveyard_order_context",m());List<String> desired=strings(d.getAsJsonArray("aliases"));require(new HashSet<>(desired).equals(new HashSet<>(sequence(cards))),"graveyard_domain",m());observe("death-order."+turn(),c.sequence,true,m("input_policy",d));CardCollection result=new CardCollection();for(int i=desired.size()-1;i>=0;i--)result.add(card(desired.get(i)));orderCount++;decisionCursor++;leave(c,m("input_policy",d,"selected_top_to_bottom",desired,"returned_sequence",sequence(result)));return result;}
        CardCollectionView discard(Player p,int count){Call c=enter(p,"chooseCardsToDiscardToMaximumHandSize",m("required",count,"domain",sequence(p.getCardsIn(ZoneType.Hand))));JsonObject d=atDecision(p,"discard");List<String> chosen=strings(d.getAsJsonArray("aliases"));require(count==chosen.size()&&count==p.getCardsIn(ZoneType.Hand).size()-7&&game.getStack().isEmpty(),"discard_context",m());observe("discard-before."+turn(),c.sequence,true,m());CardCollection result=new CardCollection();for(int i=chosen.size()-1;i>=0;i--){Card source=card(chosen.get(i));require(source.isInZone(ZoneType.Hand)&&source.getOwner()==p,"discard_domain",m());result.add(source);}decisionCursor++;leave(c,m("input_policy",d,"selected_top_to_bottom",chosen,"returned_sequence",sequence(result)));return result;}
        void housekeeping(Player p,String method){
            Call c=enter(p,method,m());String branch,occurrence;List<Player> domain;
            if(method.equals("awaitNextInput")){
                callerPath(c,"forge.game.GameAction.determineFirstTurnPlayer","forge.game.GameAction.startGame","forge.game.Match.startGame");
                require(game.getAge()==GameStage.BeforeMulligan&&entropy.currentRole().equals("starting_chooser")&&entropy.state.equals("waiting_chooser")&&p==seats.get(roles().get(1)),"await_context",m());branch="starting_chooser_notification";occurrence=branch;domain=List.of(seats.get(roles().get(1)));
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
        Map<DeckSection,List<? extends PaperCard>> complain(Player p,Deck deck){Call c=enter(p,"complainCardsCantPlayWell",m("main_count",deck.getMain().countAll(),"deck_is_registered",deck==p.getRegisteredPlayer().getDeck(),"deck",diagnosticDeck(deck)));callerPath(c,"forge.game.Match.prepareAllZones","forge.game.Match.startGame");require(game.getAge()==GameStage.BeforeMulligan&&deck==p.getRegisteredPlayer().getDeck()&&deck.getMain().countAll()==60&&entropy.state.equals("waiting_raw")&&entropy.cursor==(seat(p).equals(roles().get(0))?1:2)&&complained.add(seat(p)),"complain_context",m());leave(c,m("empty_map",true,"preceding_operation_id",str(entropy.queue.get(entropy.cursor-1),"operation_id")));return Collections.emptyMap();}
        Object diagnosticDeck(Deck deck){List<Object> rows=new ArrayList<>();for(var row:deck.getMain())rows.add(m("printing",diagnostic(row.getKey(),0),"count",row.getValue()));rows.sort(Comparator.comparing(JSON::toJson));return m("name",deck.getName(),"main_count",deck.getMain().countAll(),"main_printings",rows);}
        void unsupported(Player p,Map<Player,List<PaperCard>> values){Map<String,Object> domain=new LinkedHashMap<>();for(var e:values.entrySet())domain.put(seat(e.getKey()),e.getValue().stream().map(PaperCard::getName).toList());Call c=enter(p,"revealUnsupported",m("domain",domain));require(values.size()==2&&values.keySet().containsAll(seats.values())&&values.values().stream().allMatch(List::isEmpty)&&physical.size()==inventory.size()&&unsupported.add(seat(p)),"unsupported_context",m());leave(c,m("kind","void_ack"));}
        Player starting(Player p,boolean first){Call c=enter(p,"chooseStartingPlayer",m("is_first_game",first));require(first&&p==seats.get(roles().get(0))&&physical.size()==inventory.size(),"starting_context",m());entropy.chooser(p);long seq=leave(c,m("selected",str(fixture,"starting_role")));observe("setup.predraw",seq,true,m("reference_only",true));return seats.get(str(fixture,"starting_role"));}
        void event(String tag,Map<String,Object> payload){
            healthy();if(optional()&&resolutionPayment!=null&&tag.equals("GameEventPlayerPriority"))throw bad("priority_event_inside_resolution_payment",payload);if(tag.equals("GameEventCardChangeZone")){payload.put("actual_stack_frames",stackFrames());payload.put("resolving_abilities",java.util.stream.StreamSupport.stream(game.getStack().spliterator(),false).filter(si->(optional()?actualResolving(si):game.getStack().isResolving(si.getSourceCard()))).map(si->ability(si.getSpellAbility())).toList());}long seq=add("events",tag,m("payload",payload,"context",header(),"parent_callback_id",callStack.isEmpty()?null:callStack.peek().id,"operation_id",ordinaryOperation));
            if(causalProfile()&&tag.equals("GameEventCardChangeZone")){
                @SuppressWarnings("unchecked") Map<String,Object> card=(Map<String,Object>)payload.get("card");
                @SuppressWarnings("unchecked") Map<String,Object> from=(Map<String,Object>)payload.get("from");
                @SuppressWarnings("unchecked") Map<String,Object> to=(Map<String,Object>)payload.get("to");
                String cause=causalOperation();String event=to!=null&&"battlefield".equals(to.get("zone_type"))?"entry":from!=null&&"battlefield".equals(from.get("zone_type"))&&to!=null&&"graveyard".equals(to.get("zone_type"))?"dies":null;
                if(cause!=null&&event!=null)causalEvents.computeIfAbsent(cause,key->new ArrayList<>()).add(m("source_global_sequence",seq,"subject",card.get("alias"),"event",event,"from",from,"to",to,"card",card));
            }
            if(causalProfile()&&List.of("GameEventPlayerLivesChanged","GameEventSpellResolved","GameEventCardDamaged","GameEventTokenCreated").contains(tag))observe("event."+seq,seq,true,m("raw_event",tag));
            if(tag.equals("GameEventSpellResolved")||tag.equals("GameEventSpellRemovedFromStack")){
                @SuppressWarnings("unchecked") Map<String,Object> spell=(Map<String,Object>)payload.get(tag.equals("GameEventSpellResolved")?"spell":"sa");
                if(spell!=null){int abilityId=((Number)spell.get("ability_id")).intValue();String action=optional()&&copyNames.containsKey(abilityId)?copyNames.get(abilityId):castActions.get(abilityId);if(action!=null){List<String> tags=stackTags();String disposition=tag.equals("GameEventSpellResolved")?(Boolean.TRUE.equals(payload.get("hasFizzled"))?"all_targets_illegal_nonresolution":"resolved"):tags.stream().anyMatch(x->x.startsWith("forge.game.ability.effects.CounterEffect."))?"countered":"removed";if(!disposition.equals("removed"))settlements.put(action,disposition);settlementEvents.add(m("action_id",action,"raw_event",tag,"disposition",disposition,"event_sequence",seq,"payload",payload,"actual_stack_tags",tags,"actual_stack_frames",stackFrames(),"resolving_abilities",java.util.stream.StreamSupport.stream(game.getStack().spliterator(),false).filter(si->(optional()?actualResolving(si):game.getStack().isResolving(si.getSourceCard()))).map(si->ability(si.getSpellAbility())).toList(),"stack_bottom_first",stackAliases()));}}
            }
            if(tag.equals("GameEventShuffle"))entropy.committed((String)payload.get("player"),seq);
            if(tag.equals("GameEventMulligan"))observe("mulligan.bottom.after",seq,true,m());
            if(tag.equals("GameEventAttackersDeclared")){observe("attack-event."+turn(),seq,true,payload);if(declarationActions.containsKey("attackers"))observe("action."+declarationActions.remove("attackers"),seq,true,payload);}
            if(tag.equals("GameEventBlockersDeclared")){observe("block-event."+turn(),seq,true,payload);if(declarationActions.containsKey("blockers"))observe("action."+declarationActions.remove("blockers"),seq,true,payload);else if(optional())retireUnissuedEmptyBlockers(seq,payload);}
            if(tag.equals("GameEventPlayerDamaged"))playerDamageTurns.add(turn());
            if(tag.equals("GameEventPlayerControl"))require(stackTags().stream().anyMatch(x->x.equals("forge.game.Game.setGameOver"))&&stackTags().stream().anyMatch(x->x.equals("forge.game.player.Player.clearController"))&&payload.get("newLobbyPlayerName")==null&&Boolean.TRUE.equals(payload.get("newControllerIsHuman")),"player_control_branch",payload);
            if(tag.equals("GameEventGameOutcome")){if(playerDamageTurns.contains(turn()))once("damage."+personal(),seq,true,m());observe("terminal",seq,true,payload);}
            if(tag.equals("GameEventGameFinished")){require(game.isGameOver(),"finished_without_outcome",m());finishedEvent=true;}
        }
        List<Map<String,Object>> stackFrames(){return StackWalker.getInstance().walk(frames->frames.map(f->m("class",f.getClassName(),"method",f.getMethodName(),"descriptor",f.getDescriptor())).toList());}
        String resolvingEntry(){
            if(optional()){if(game.getStack().isEmpty()||!game.getStack().isResolving())return null;SpellAbility actual=game.getStack().peekAbility();if(!game.getStack().isResolving(actual.getHostCard()))return null;String label=entryName(actual);require(label!=null,"unknown_resolving_entry",m("ability",ability(actual)));return label;}
            List<String> resolving=new ArrayList<>();for(SpellAbilityStackInstance si:game.getStack())if((optional()?actualResolving(si):game.getStack().isResolving(si.getSourceCard()))){
                String name=optional()?entryName(si.getSpellAbility()):si.getSpellAbility().isTrigger()?triggerNames.get(si.getSpellAbility().getId()):castActions.get(si.getSpellAbility().getId());
                require(name!=null,"unknown_resolving_entry",m("ability",ability(si.getSpellAbility())));resolving.add(name);
            }
            if(!resolving.isEmpty()){require(resolving.size()==1,"ambiguous_resolving_entry",m());return resolving.get(0);}return null;
        }
        String causalOperation(){
            String entry=resolvingEntry();if(entry!=null){List<String> names=new ArrayList<>();for(JsonObject op:objects(fixture.getAsJsonArray("causal_operations")))if(op.has("entry")&&(!optional()||!str(op,"kind").equals("cast_completed"))){JsonObject ref=op.getAsJsonObject("entry");if(entry.equals((optional()?entryRef(ref):str(ref,str(ref,"kind").equals("cast_entry")?"operation":"trigger"))))names.add(str(op,"name"));}require(names.size()<=1,"ambiguous_cause_operation",m());return names.isEmpty()?entry:names.get(0);}
            JsonObject combat=combatRecipe();if(combat!=null&&step().equals("combat_damage"))return str(combat.getAsJsonObject("input_instruction"),"cause");
            return null;
        }
        String objectAlias(JsonObject ref){
            String kind=str(ref,"kind");if(kind.equals("physical_current")||kind.equals("physical_at_cause"))return str(ref,"alias");
            if(kind.equals("token_at_cause"))return objectAlias(ref.getAsJsonObject("token"));
            require(kind.equals("created_token"),"trigger_reference_kind",m("ref",ref));JsonObject entry=ref.getAsJsonObject("creation_entry");
            return "token:"+str(entry,str(entry,"kind").equals("cast_entry")?"operation":"trigger")+":"+number(ref,"instruction")+":"+str(ref,"creator")+":"+number(ref,"member");
        }
        String registerToken(Card token){
            require(causalProfile()&&token.isToken()&&token.isInPlay(),"new_token_context",m("id",token.getId()));String cause=resolvingEntry();require(cause!=null,"token_creation_cause",m());
            String cohort=cause+":0:"+seat(token.getOwner());List<String> members=tokenCohorts.computeIfAbsent(cohort,key->new ArrayList<>());String alias="token:"+cohort+":"+members.size();members.add(alias);require(!aliases.containsValue(alias),"duplicate_token_alias",m());aliases.put(token.getId(),alias);
            tokenReceipts.add(m("alias",alias,"creation_entry",cause,"instruction",0,"creator",seat(token.getOwner()),"member",members.size()-1,"physical_id",String.valueOf(token.getId()),"timestamp",String.valueOf(token.getGameTimestamp()),"name",token.getName(),"mana_cost",token.getManaCost().toString(),"color_mask",token.getColor().getColor(),"core_types",token.getType().getCoreTypes().stream().map(Enum::name).sorted().toList(),"supertypes",token.getType().getSupertypes().stream().map(Enum::name).sorted().toList(),"subtypes",token.getType().getSubtypes().stream().sorted().toList(),"keywords",token.getKeywords().stream().map(Object::toString).toList(),"triggered_count",token.getTriggers().size(),"activated_count",token.getSpellAbilities().stream().filter(sa->sa.isActivatedAbility()||sa.isManaAbility()).count(),"printing",m("name",token.getPaperCard().getName(),"edition",token.getPaperCard().getEdition(),"collector_number",token.getPaperCard().getCollectorNumber(),"art_index",token.getPaperCard().getArtIndex(),"foil",token.getPaperCard().isFoil()),"state",cardState(token),"observed_before_sequence",global+1));return alias;
        }
        void triggers(Player p,List<SpellAbility> offered){
            if(optional())require(resolutionPayment==null,"trigger_placement_inside_resolution_payment",m());
            Call call=enter(p,"orderAndPlaySimultaneousSa",m("domain",offered.stream().map(this::ability).toList(),"triggering_objects",offered.stream().map(sa->diagnostic(sa.getTriggeringObjects(),0)).toList()));
            callerPath(call,"forge.game.zone.MagicStack.chooseOrderOfSimultaneousStackEntry","forge.game.zone.MagicStack.addAllTriggeredAbilitiesToStack");
            Map<String,SpellAbility> matched=new LinkedHashMap<>();String groupName=null;JsonObject group=null;
            for(SpellAbility sa:offered){
                require(sa.isTrigger()&&(optional()||!sa.isOptionalTrigger())&&sa.getActivatingPlayer()==p,"mandatory_trigger_domain",m("ability",ability(sa)));
                Object actual=sa.getTriggeringObject(AbilityKey.Card);Object castCause=sa.getTriggeringObject(AbilityKey.SpellAbility);
                if(optional()&&castCause instanceof SpellAbility spell){require(spell.isSpell()&&!spell.isCopied()&&sa.getTriggeringObject(AbilityKey.Activator)==spell.getActivatingPlayer(),"actual_cast_trigger_cause",m("actual",diagnostic(sa.getTriggeringObjects(),0)));actual=spell.getHostCard();}
                require(actual instanceof Card,"trigger_subject",m("actual",diagnostic(actual,0)));String subject=alias((Card)actual),source=alias(sa.getHostCard());
                List<JsonObject> candidates=new ArrayList<>();List<JsonObject> candidateGroups=new ArrayList<>();
                for(JsonObject candidateGroup:objects(fixture.getAsJsonArray("trigger_groups")))for(JsonObject declaration:objects(candidateGroup.getAsJsonArray("triggers"))){
                    if(!usedTriggers.contains(str(declaration,"name"))&&objectAlias(declaration.getAsJsonObject("source")).equals(source)&&objectAlias(declaration.getAsJsonObject("subject")).equals(subject)&&str(declaration,"captured_controller").equals(seat(p))){
                        String cause=str(declaration,"cause_operation");String event=str(declaration,"event");List<Map<String,Object>> receipts=causalEvents.getOrDefault(cause,List.of());
                        if((!declaration.has("caster")||castCause instanceof SpellAbility spell&&str(declaration,"caster").equals(seat(spell.getActivatingPlayer())))&&receipts.stream().anyMatch(row->subject.equals(row.get("subject"))&&event.equals(row.get("event")))){candidates.add(declaration);candidateGroups.add(candidateGroup);}
                    }
                }
                require(candidates.size()==1,"trigger_causal_bijection",m("source",source,"subject",subject,"matches",candidates.size(),"actual",diagnostic(sa.getTriggeringObjects(),0)));JsonObject declaration=candidates.get(0);JsonObject actualGroup=candidateGroups.get(0);
                if(groupName==null){groupName=str(actualGroup,"name");group=actualGroup;}require(groupName.equals(str(actualGroup,"name")),"trigger_group_cohort",m());String name=str(declaration,"name");require(matched.put(name,sa)==null&&usedTriggers.add(name),"duplicate_trigger_instance",m());triggerNames.put(sa.getId(),name);if(sa instanceof WrappedAbility wrapped)triggerNames.put(wrapped.getWrappedAbility().getId(),name);
                triggerReceipts.add(m("name",name,"group",groupName,"input_selector",declaration,"actual_ability",ability(sa),"trigger_id",sa.getTrigger().getId(),"trigger_mode",sa.getTrigger().getMode().name(),"actual_triggering_objects",diagnostic(sa.getTriggeringObjects(),0),"subject",subject,"captured_controller",seat(sa.getActivatingPlayer()),"actual_source_state",cardState(sa.getHostCard()),"causal_events",causalEvents.get(str(declaration,"cause_operation")),"callback_id",call.id));
            }
            require(group!=null,"empty_trigger_callback",m());List<String> order=new ArrayList<>();for(String name:strings(group.getAsJsonArray("order_bottom_first")))if(matched.containsKey(name))order.add(name);
            long expected=objects(group.getAsJsonArray("triggers")).stream().filter(row->str(row,"captured_controller").equals(seat(p))).count();require(order.size()==offered.size()&&order.size()==expected,"complete_controller_trigger_domain",m());
            List<Object> admitted=new ArrayList<>();for(String name:order){SpellAbility sa=matched.get(name);selected=sa;selectedAction=new JsonObject();selectedAction.addProperty("policy_id",name);selectedAction.addProperty("required_cost","{0}");require(PlaySpellAbility.playSpellAbility(p.getController(),p,sa),"mandatory_trigger_admission",m("trigger",name));SpellAbilityStackInstance si=game.getStack().getInstanceMatchingSpellAbilityID(sa);require(si!=null,"trigger_installed",m("trigger",name));admitted.add(m("trigger",name,"stack_id",si.getId(),"ability",ability(si.getSpellAbility())));selected=null;selectedAction=null;}
            leave(call,m("group",groupName,"selected_bottom_first",order,"admitted",admitted,"actual_stack_bottom_first",stackAliases()));
        }
        void triggerBody(Player p,SpellAbility sa,boolean newTargets){
            Call call=enter(p,"playSpellAbilityNoStack",m("ability",ability(sa),"may_choose_new_targets",newTargets));callerPath(call,"forge.game.trigger.WrappedAbility.resolve","forge.game.ability.AbilityUtils.resolveApiAbility","forge.game.ability.AbilityUtils.resolve","forge.game.zone.MagicStack.resolveStack");
            String name=triggerNames.get(sa.getId());require(name!=null&&!newTargets&&p==sa.getActivatingPlayer()&&game.getStack().isResolving(sa.getHostCard()),"installed_trigger_body_context",m());
            require(selected==null&&selectedAction==null,"trigger_body_selection",m());selected=sa;selectedAction=new JsonObject();selectedAction.addProperty("required_cost","{0}");
            require(PlaySpellAbility.playSpellAbilityNoStack(p.getController(),p,sa,true),"trigger_body_completion",m());selected=null;selectedAction=null;long seq=leave(call,m("trigger",name,"actual_ability",ability(sa),"body_complete",true));observe("body."+name,seq,true,m("trigger",name));
        }
        List<String> tagsFromFrames(List<Map<String,Object>> frames){return frames.stream().map(f->f.get("class")+"."+f.get("method")).toList();}
        List<String> stackTags(){return StackWalker.getInstance().walk(frames->frames.map(f->f.getClassName()+"."+f.getMethodName()).toList());}
        void finish(){
            healthy();settlementCuts();boolean prepared=prepared();if(optional())require(decisionPlanCursor==fixture.getAsJsonArray("decision_plans").size()&&resolutionPayment==null&&fundingInstruction==null&&copyPlan==null&&spellCopies.stream().allMatch(row->row.containsKey("copy_stack_id")&&row.containsKey("copy_stack_add_sequence")),"complete_optional_plan_consumption",m("consumed",decisionPlanConsumption));
            if(causalProfile()){Set<String> declared=new LinkedHashSet<>();for(JsonObject group:objects(fixture.getAsJsonArray("trigger_groups")))for(JsonObject trigger:objects(group.getAsJsonArray("triggers")))declared.add(str(trigger,"name"));require(usedTriggers.equals(declared),"complete_trigger_input_consumption",m());require(usedPlayerChoices.equals(new LinkedHashSet<>(objects(fixture.getAsJsonArray("player_choices")).stream().map(row->str(row,"trigger")).toList())),"complete_player_choice_consumption",m());}
            for(JsonObject recipe:objects(fixture.getAsJsonArray("combat")))if(!strings(recipe.getAsJsonArray("attackers")).isEmpty())require(assigned.contains("attack:"+number(recipe,"individual_turn")),"unused_combat_recipe",m("recipe",recipe));
            require(callStack.isEmpty()&&actionCursor==actions.size()&&decisionCursor==decisions.size()&&openingUsed.size()==opening.size()&&entropy.cursor==entropy.queue.size(),"incomplete_run",m("actions",actionCursor,"planned",actions.size()));
            if(boundedStop)require(prepared&&!game.isGameOver()&&outcome()==null&&(optional()?stackAliases().equals(strings(fixture.getAsJsonObject("stop").getAsJsonArray("expected_stack_bottom_first"))):game.getStack().isEmpty()),"bounded_outcome",m());
            else require(game.isGameOver()&&(prepared?naturalPreparedReturn:naturalReturn&&finishedEvent),"natural_completion",m());
            require(physical.size()==inventory.size(),"terminal_inventory",m());world();
        }
        Map<String,Object> output(){Map<String,Object> out=m("schema","interaction-game-forge-v1","schema_version",1,"case_id",id,"reference","forge","forge_commit",PIN,"inputs_sha256",INPUT_SHA,"fixture",fixture,"paper_cards",paperCards,"actual_outcome",outcome(),"physical_aliases",physical,"registered_players",registeredPlayers(),"callbacks",streams.get("callbacks"),"events",streams.get("events"),"random",streams.get("random"),"operations",streams.get("operations"),"cast_operations",castOperations,"settlement_events",settlementEvents,"settlements",settlements,"failures",failures,"observations",observations,"checkpoints",checkpoints,"runtime",m("loader_classes",loaderClasses,"java_home",System.getProperty("java.home"),"java_version",System.getProperty("java.runtime.version"),"java_vendor",System.getProperty("java.vendor"),"java_class_path",System.getProperty("java.class.path")),"completion",m("kind",boundedStop?"bounded":"natural","natural_start_game_return",naturalReturn,"natural_prepared_loop_return",naturalPreparedReturn,"bounded_ready_exit",boundedStop,"game_over",game!=null&&game.isGameOver(),"game_finished_event",finishedEvent,"actions_consumed",actionCursor,"actions_planned",actions.size(),"decisions_consumed",decisionCursor,"decisions_planned",decisions.size(),"opening_consumed",openingUsed.size(),"opening_planned",opening.size(),"random_operations_consumed",entropy.cursor,"random_operations_planned",entropy.queue.size(),"sticky_failure_count",failures.size(),"callback_depth",callStack.size()),"final_world",game==null||physical.size()!=inventory.size()?null:world());if(causalProfile()){out.put("profile",optional()?"optional-payment-v1":"mandatory-trigger-v1");out.put("trigger_receipts",triggerReceipts);out.put("token_receipts",tokenReceipts);out.put("causal_events",causalEvents);out.put("used_player_choices",usedPlayerChoices);out.put("unavailable",List.of("private waiting queue","identical-token relative timestamp discriminator","private failed-draw flag","dedicated token cessation event"));}if(optional()){
                out.put("optional_schema_version",2);out.put("optional_choices",optionalChoices);out.put("resolution_payments",resolutionPayments);out.put("mana_activations",manaActivations);out.put("spell_copies",spellCopies);out.put("raw_stack_additions",rawStackAdditions);out.put("decision_plan_consumption",decisionPlanConsumption);
                out.put("planned_input_retirements",plannedInputRetirements);out.put("plan_slot_accounting",m("advanced_slots",actionCursor,"retired_not_issued_slots",plannedInputRetirements.size(),"slots_advanced_by_existing_paths",actionCursor-plannedInputRetirements.size(),"actions_consumed_includes_retired_plan_slots",true,"existing_paths_are_not_asserted_to_be_accepted_game_actions",true));
                out.put("synthetic_copy_hosts",syntheticCopies);out.put("unavailable",List.of("native GameBasis/DecisionId/frame revision","native ResolutionCursor/instruction successors/locals","native PendingWork/payment parent/restoration","private Forge waiting queue","private Forge failed-draw flag","native canonical journals/ManaPaymentRecord/full-state replay","selected physical source for a Forge synthetic copy host"));
            }return out;}
    }

    static final class RecordedEntropy extends Random {
        private static final long serialVersionUID=1L;
        final Script s;final List<JsonObject> queue;final Map<String,JsonObject> plans=new LinkedHashMap<>();final Thread thread=Thread.currentThread();
        long installedCallCount;int cursor,sample;boolean insideDelegate;String state="waiting_raw";List<String> before,target;List<Integer> indices,bounds;List<Long> words;Map<String,Object> otherBefore;
        final Map<String,Integer> artSamples=new LinkedHashMap<>();
        RecordedEntropy(Script s){super(0);this.s=s;queue=objects(s.fixture.getAsJsonArray("forge_operation_queue"));for(JsonObject x:objects(s.fixture.getAsJsonArray("shuffle_operations")))plans.put(str(x,"operation_id"),x);}
        String currentRole(){return cursor<queue.size()?str(queue.get(cursor),"role"):"complete";}
        @Override public int nextInt(){
            if(insideDelegate)throw s.bad("random_reentrancy",m("actual_stack_tags",s.stackTags()));
            insideDelegate=true;
            try{return supply();}finally{insideDelegate=false;}
        }
        int supply(){
            if(s.causalProfile()){
                List<Map<String,Object>> frames=s.stackFrames();List<String> tags=s.tagsFromFrames(frames);
                boolean tokenDb=tags.contains("forge.token.TokenDb.getToken"),paper=tags.contains("forge.item.PaperToken.getImageKey");
                if(tokenDb||paper){String cause=s.resolvingEntry();s.require(Thread.currentThread()==thread&&cause!=null&&tags.contains("forge.game.card.token.TokenInfo.getProtoType")&&tags.contains("forge.game.ability.effects.TokenEffect.resolve")&&tags.contains("forge.util.MyRandom$CountingRandom.next")&&tags.contains("java.util.Random.nextInt"),"token_art_random_context",m("actual_stack_tags",tags));JsonObject policy=s.fixture.getAsJsonObject("token_art_entropy");int ordinal=artSamples.getOrDefault(cause,0);s.require(ordinal<number(policy,"maximum_samples_per_creation"),"token_art_random_bound",m());artSamples.put(cause,ordinal+1);int raw=number(policy,"raw_signed_32");s.add("random","token_art_raw",m("creation_entry",cause,"ordinal_in_creation",ordinal,"consumer",tokenDb?"TokenDb.getToken":"PaperToken.getImageKey","raw_signed_32",raw,"actual_stack_tags",tags,"actual_stack_frames",frames,"input_policy",policy));return raw;}
            }
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
            String delegate="forge.ai.SpellwakeBlueInteractionReferenceTest$RecordedEntropy.nextInt";
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
        void chooser(Player p){JsonObject op=queue.get(cursor);s.require(state.equals("waiting_chooser")&&s.seat(p).equals(str(op,"chosen_chooser")),"chooser_callback",m());s.add("random","chooser_complete",m("operation_id",str(op,"operation_id"),"actual_chooser",s.seat(p),"selected_starting_seat",str(s.fixture,"starting_role")));cursor++;state="waiting_raw";}
    }

    static class StrictCosts extends CostDecisionMakerBase {
        final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script){this(player,ability,script,false);}
        StrictCosts(Player player,SpellAbility ability,Script script,boolean effect){super(player,effect,ability,ability.getHostCard());this.script=script;}
        @Override public boolean paysRightAfterDecision(){return true;}
        @Override public PaymentDecision visit(CostTap cost) { boolean canPay=cost.canPay(ability,player,false);Call c=script.enter(player,"visit_CostTap",m("cost",cost.toString(),"ability",script.ability(ability),"payer",script.seat(player),"visitor",script.objectRef(this),"scope",script.costScope(cost,ability),"can_pay",canPay));script.validateCostScope(cost,ability,"original");script.require(ability==script.selected&&ability.isManaAbility()&&canPay&&!ability.getHostCard().isTapped(),"tap_cost_context",m());script.leave(c,m("payment_decision_number",0,"can_pay",canPay,"visitor",script.objectRef(this)));return PaymentDecision.number(0); }
        @Override public PaymentDecision visit(CostPartMana cost) { Call c=script.enter(player,"visit_CostPartMana",m("cost",cost.getManaCostFor(ability).toString(),"ability",script.ability(ability),"payer",script.seat(player),"visitor",script.objectRef(this),"scope",script.costScope(cost,ability)));script.validateCostScope(cost,ability,ability.isManaAbility()?"implicit_zero_mana":"original");script.require(ability==script.selected&&(ability.isManaAbility()?cost.getManaCostFor(ability).isZero():cost.getManaCostFor(ability).toString().equals(str(script.selectedAction,"required_cost"))),"mana_visitor_context",m());script.leave(c,m("payment_decision_number",0,"part_origin",ability.isManaAbility()?"implicit_zero_mana":"original","visitor",script.objectRef(this)));return PaymentDecision.number(0); }
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

    static final class ResolutionCosts extends StrictCosts {
        ResolutionCosts(Player player,SpellAbility ability,Script script){super(player,ability,script,true);}
        @Override public PaymentDecision visit(CostTap cost){throw script.unexpected(player,"visit_CostTap",cost);}
        @Override public PaymentDecision visit(CostPartMana cost){
            Call c=script.enter(player,"visit_CostPartMana",m("cost",cost.getManaCostFor(ability).toString(),"ability",script.ability(ability),"payer",script.seat(player),"visitor",script.objectRef(this),"effect",isEffect(),"payment_scope",script.resolutionScope(cost,ability)));
            script.validateResolutionScope(cost,ability,player);script.require(isEffect(),"resolution_visitor_effect",m());
            script.leave(c,m("payment_decision_number",0,"visitor",script.objectRef(this)));return PaymentDecision.number(0);
        }
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
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { if(!script.causalProfile())throw script.unexpected(getPlayer(),"playSpellAbilityNoStack",effectSA,mayChoseNewTargets);script.triggerBody(getPlayer(),effectSA,mayChoseNewTargets); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { throw script.unexpected(getPlayer(),"orderSimultaneousSa",activePlayerSAs); }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { if(!script.causalProfile())throw script.unexpected(getPlayer(),"orderAndPlaySimultaneousSa",activePlayerSAs);if(script.optional()&&activePlayerSAs.stream().allMatch(SpellAbility::isCopied))script.installCopies(getPlayer(),activePlayerSAs);else script.triggers(getPlayer(),activePlayerSAs); }
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
        @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) { if(!script.optional())throw script.unexpected(getPlayer(),"chooseNewTargetsFor",ability,filter,optional);return script.copyTargets(getPlayer(),ability,filter,optional); }
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { return script.chooseTargets(getPlayer(),currentAbility); }
        @Override public Pair<SpellAbilityStackInstance, GameObject> chooseTarget(SpellAbility sa, List<Pair<SpellAbilityStackInstance, GameObject>> allTargets) { throw script.unexpected(getPlayer(),"chooseTarget",sa,allTargets); }
        @Override public boolean helpPayForAssistSpell(ManaCostBeingPaid cost, SpellAbility sa, int max, int requested) { throw script.unexpected(getPlayer(),"helpPayForAssistSpell",cost,sa,max,requested); }
        @Override public Player choosePlayerToAssistPayment(FCollectionView<Player> optionList, SpellAbility sa, String title, int max) { throw script.unexpected(getPlayer(),"choosePlayerToAssistPayment",optionList,sa,title,max); }
        @Override public CardCollectionView chooseCardsForEffect(CardCollectionView sourceList, SpellAbility sa, String title, int min, int max, boolean isOptional, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseCardsForEffect",sourceList,sa,title,min,max,isOptional,params); }
        @Override public CardCollection chooseCardsForEffectMultiple(Map<String, CardCollection> validMap, SpellAbility sa, String title, boolean isOptional) { throw script.unexpected(getPlayer(),"chooseCardsForEffectMultiple",validMap,sa,title,isOptional); }
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSingleEntityForEffect",optionList,delayedReveal,sa,title,isOptional,relatedPlayer,params); }
        @Override public <T extends GameEntity> List<T> chooseEntitiesForEffect(FCollectionView<T> optionList, int min, int max, DelayedReveal delayedReveal, SpellAbility sa, String title, Player relatedPlayer, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseEntitiesForEffect",optionList,min,max,delayedReveal,sa,title,relatedPlayer,params); }
        @Override public List<SpellAbility> chooseSpellAbilitiesForEffect(List<SpellAbility> spells, SpellAbility sa, String title, int num, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSpellAbilitiesForEffect",spells,sa,title,num,params); }
        @Override public SpellAbility chooseSingleSpellForEffect(List<SpellAbility> spells, SpellAbility sa, String title, Map<String, Object> params) { throw script.unexpected(getPlayer(),"chooseSingleSpellForEffect",spells,sa,title,params); }
        @Override public boolean confirmAction(SpellAbility sa, PlayerActionConfirmMode mode, String message, List<String> options, Card cardToShow, Map<String, Object> params) { if(!script.optional())throw script.unexpected(getPlayer(),"confirmAction",sa,mode,message,options,cardToShow,params);return script.optionalDraw(getPlayer(),sa,mode,message,options,cardToShow,params); }
        @Override public boolean confirmBidAction(SpellAbility sa, PlayerActionConfirmMode bidlife, String string, int bid, Player winner) { throw script.unexpected(getPlayer(),"confirmBidAction",sa,bidlife,string,bid,winner); }
        @Override public boolean confirmReplacementEffect(ReplacementEffect replacementEffect, SpellAbility effectSA, GameEntity affected, String question) { throw script.unexpected(getPlayer(),"confirmReplacementEffect",replacementEffect,effectSA,affected,question); }
        @Override public boolean confirmStaticApplication(Card hostCard, PlayerActionConfirmMode mode, String message, String logic) { throw script.unexpected(getPlayer(),"confirmStaticApplication",hostCard,mode,message,logic); }
        @Override public boolean confirmTrigger(WrappedAbility sa) { if(!script.optional())throw script.unexpected(getPlayer(),"confirmTrigger",sa);return script.optionalTrigger(getPlayer(),sa); }
        @Override public List<Card> exertAttackers(List<Card> attackers) { throw script.unexpected(getPlayer(),"exertAttackers",attackers); }
        @Override public List<Card> enlistAttackers(List<Card> attackers) { throw script.unexpected(getPlayer(),"enlistAttackers",attackers); }
        @Override public void declareAttackers(Player attacker, Combat combat) { script.attackers(getPlayer(),attacker,combat); }
        @Override public void declareBlockers(Player defender, Combat combat) { script.blockers(getPlayer(),defender,combat); }
        @Override public CardCollection orderBlockers(Card attacker, CardCollection blockers) { throw script.unexpected(getPlayer(),"orderBlockers",attacker,blockers); }
        @Override public CardCollection orderBlocker(final Card attacker, final Card blocker, final CardCollection oldBlockers) { throw script.unexpected(getPlayer(),"orderBlocker",attacker,blocker,oldBlockers); }
        @Override public CardCollection orderAttackers(Card blocker, CardCollection attackers) { throw script.unexpected(getPlayer(),"orderAttackers",blocker,attackers); }
        @Override public void reveal(CardCollectionView cards, ZoneType zone, Player owner, String messagePrefix, boolean addMsgSuffix) { throw script.unexpected(getPlayer(),"reveal",cards,zone,owner,messagePrefix,addMsgSuffix); }
        @Override public void reveal(List<CardView> cards, ZoneType zone, PlayerView owner, String messagePrefix, boolean addMsgSuffix) { throw script.unexpected(getPlayer(),"reveal",cards,zone,owner,messagePrefix,addMsgSuffix); }
        @Override public void notifyOfValue(SpellAbility saSource, GameObject realtedTarget, String value) { if(!script.optional())throw script.unexpected(getPlayer(),"notifyOfValue",saSource,realtedTarget,value);script.manaColorNotification(getPlayer(),saSource,realtedTarget,value); }
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
        @Override public byte chooseColor(String message, SpellAbility sa, ColorSet colors) { if(!script.optional())throw script.unexpected(getPlayer(),"chooseColor",message,sa,colors);return script.paymentColor(getPlayer(),message,sa,colors); }
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
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { if(!script.optional())throw script.unexpected(getPlayer(),"payCostToPreventEffect",cost,sa,alreadyPaid,allPayers);return script.preventEffect(getPlayer(),cost,sa,alreadyPaid,allPayers); }
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
        @Subscribe public void cast(GameEventSpellAbilityCast e){emit("GameEventSpellAbilityCast",()->{Map<String,Object> payload=m("sa",s.view(e.sa()),"si",s.view(e.si()),"stackIndex",e.stackIndex(),"targetDescription",e.targetDescription());if(s.optional())s.classifyStackAddition(e,payload);return payload;});}
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
        @Subscribe public void tokenCreated(GameEventTokenCreated e){if(s.causalProfile())emit("GameEventTokenCreated",()->m("actual_creation_entry",s.resolvingEntry()));}
        @Subscribe public void finished(GameEventGameFinished e){emit("GameEventGameFinished",()->m());}
    }
}
