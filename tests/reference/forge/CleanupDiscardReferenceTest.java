package forge.ai;

import com.google.common.collect.ListMultimap;
import com.google.common.collect.Multimap;
import forge.card.ColorSet;
import forge.card.ICardFace;
import forge.card.mana.ManaCost;
import forge.card.mana.ManaCostShard;
import forge.deck.Deck;
import forge.deck.DeckSection;
import forge.game.*;
import forge.game.player.Player;
import forge.game.player.DelayedReveal;
import forge.game.player.PlayerActionConfirmMode;
import forge.game.player.PlayerView;
import forge.game.player.PlayerController;
import forge.game.ability.effects.RollDiceEffect;
import forge.game.card.*;
import forge.game.card.sticker.Sticker;
import forge.game.combat.Combat;
import forge.game.cost.*;
import forge.game.keyword.KeywordInterface;
import forge.game.mana.Mana;
import forge.game.mana.ManaConversionMatrix;
import forge.game.mana.ManaCostBeingPaid;
import forge.game.replacement.ReplacementEffect;
import forge.game.spellability.*;
import forge.game.staticability.StaticAbility;
import forge.game.trigger.WrappedAbility;
import forge.game.zone.PlayerZone;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import forge.util.ITriggerEvent;
import forge.util.collect.FCollectionView;
import org.apache.commons.lang3.tuple.ImmutablePair;
import org.apache.commons.lang3.tuple.Pair;

import java.util.Collection;
import java.util.List;
import java.util.Map;
import java.util.function.Predicate;


import com.google.gson.GsonBuilder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import org.testng.Assert;
import org.testng.annotations.Test;
import forge.model.FModel;
import forge.item.IPaperCard;

import forge.game.event.*;

import forge.card.mana.ManaAtom;

import forge.game.player.PlaySpellAbility;
import com.google.common.eventbus.Subscribe;


import forge.game.phase.PhaseType;
import forge.game.player.RegisteredPlayer;




import forge.game.ability.ApiType;
import java.security.MessageDigest;

/** Strict actual Forge cleanup/ordered-pile probes; expectations are frozen externally. */
public class CleanupDiscardReferenceTest extends AITest {
    static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    static final String EXPECTED_SHA="218f71258bc1abe873297e163b750729f68f2aa6f8b3abe4a036cd093f9d6fed";
    @Test public void defaultSevenOrderedThree() throws Exception {run("C01");}
    @Test public void exactlySevenNoDiscard() throws Exception {run("C02");}
    @Test public void emptyHandNoDiscard() throws Exception {run("C03");}
    @Test public void discardBeforeMarkedDamageClears() throws Exception {run("C04");}
    @Test public void preparedZeroToughnessExceptionRepeat() throws Exception {run("C05");}
    @Test public void sameOwnerDeathFirstOrder() throws Exception {run("C06");}
    @Test public void sameOwnerDeathReverseOrder() throws Exception {run("C07");}
    @Test public void apnapActiveMiddleStolenOwners() throws Exception {run("C08");}
    @Test public void counteredTargetBeforeCounterSource() throws Exception {run("C09");}
    @Test public void resolvingSourceBeforeSubsequentSba() throws Exception {run("C10");}
    @Test public void sacrificeCostBeforeSubsequentSba() throws Exception {run("C11");}
    @Test public void primitiveDepartureAndReentry() throws Exception {run("C12");}
    @Test public void adversarialLethalMarksAtCleanupEntry() throws Exception {run("P01");}
    static Map<String,Object> map(Object... args){Map<String,Object> out=new LinkedHashMap<>();for(int i=0;i<args.length;i+=2)out.put((String)args[i],args[i+1]);return out;}
    static String hex(byte[] b)throws Exception{StringBuilder out=new StringBuilder();for(byte x:MessageDigest.getInstance("SHA-256").digest(b))out.append(String.format("%02x",x&255));return out.toString();}
    static List<Map<String,Object>> runtime()throws Exception{
        List<Map<String,Object>> out=new ArrayList<>();String[] classes={"forge.ai.CleanupDiscardReferenceTest","forge.ai.CleanupDiscardReferenceTest$Script","forge.ai.CleanupDiscardReferenceTest$StrictController","forge.ai.CleanupDiscardReferenceTest$StrictCosts","forge.game.Game","forge.game.GameAction","forge.game.GameActionUtil","forge.game.phase.PhaseHandler","forge.game.player.Player","forge.game.player.PlayerController","forge.game.player.PlaySpellAbility","forge.game.card.Card","forge.game.card.CardProperty","forge.game.zone.Zone","forge.game.zone.PlayerZone","forge.game.zone.MagicStack","forge.game.cost.CostSacrifice","forge.game.ability.effects.CounterEffect","forge.game.ability.effects.ChangeZoneEffect","forge.game.ability.effects.DamageDealEffect"};
        for(String n:classes){Class<?> c=Class.forName(n);String resource="/"+n.replace('.','/')+".class";byte[] bytes;try(java.io.InputStream in=c.getResourceAsStream(resource)){Assert.assertNotNull(in);bytes=in.readAllBytes();}java.nio.file.Path p=Paths.get(System.getProperty("spellwake.evidence.dir"),"loader-classes",n.replace('.','/')+".class");Files.createDirectories(p.getParent());if(Files.exists(p))Assert.assertEquals(hex(Files.readAllBytes(p)),hex(bytes));else Files.write(p,bytes,java.nio.file.StandardOpenOption.CREATE_NEW);out.add(map("class",n,"source",c.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",c.getResource(resource).toString(),"sha256",hex(bytes)));}return out;
    }
    static Game fresh(String id){List<RegisteredPlayer> ps=new ArrayList<>();String[] order=id.equals("C08")?new String[]{"A","C","B"}:new String[]{"A","B"};Map<String,Integer> ids=Map.of("A",41,"B",3,"C",99);for(int i=0;i<order.length;i++){RegisteredPlayer p=new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-"+order[i],null));p.setId(ids.get(order[i]));p.setStartingLife(20);p.setTeamNumber(i+1);ps.add(p);}GameRules r=new GameRules(GameType.Constructed);r.setOrderCombatants(false);Game g=new Game(ps,r,new Match(r,ps,"Cleanup reference"));g.setAge(GameStage.Play);return g;}
    void run(String id)throws Exception{Script s=new Script(fresh(id),id);boolean completed=false;List<Map<String,Object>> rt=runtime();try{s.seed();s.game.subscribeToEvents(s);s.game.getPhaseHandler().setupFirstTurn(s.seats.get(id.equals("C08")?"C":"A"),null);
        if(id.equals("C12")){s.row("initial","trusted primitive initial");s.game.getAction().moveToHand(s.card("traveller"),null);s.row("departed","trusted primitive move to owner hand");s.game.getAction().moveToGraveyard(s.card("traveller"),null);s.row("reentered","trusted primitive move to owner graveyard");s.done=true;}
        else for(int i=0;i<360;i++){Assert.assertFalse(s.game.isGameOver());int before=s.priorities.size();s.game.getPhaseHandler().mainLoopStep();s.observe(s.loops,map("index",i,"priority_callbacks",s.priorities.size()-before,"after",s.control()));if(s.eventFailure!=null)throw new AssertionError("Observer failed",s.eventFailure);if(s.done)break;Assert.assertTrue(i<359,"Bounded loop exhausted");}
        s.validate();completed=true;
    }finally{Map<String,Object> trace=map("schema_version",1,"profile","cleanup-discard","scenario",id,"commit",COMMIT,"expected_sha256",EXPECTED_SHA,"completed",completed,"strict_choices",true,"ai_policy_used",false,"priority_loop_executed",!id.equals("C12"),"direct_zone_primitive_used",id.equals("C12"),"dev_phase_advance_used",false,"direct_resolution_used",false,"direct_damage_used",false,"trusted_cleanup_entry_override",id.equals("C05")||id.equals("P01"),"order_combatants",s.game.getRules().hasOrderCombatants(),"configuration",s.configuration,"rows",s.rows,"choices",s.choices,"callbacks",s.callbacks,"actions",s.actions,"events",s.events,"priorities",s.priorities,"declarations",s.declarations,"assignments",s.assignments,"loops",s.loops,"metadata",s.metadata,"runtime",rt,"cleanup_entries",s.cleanupEntries,"actual_actions",s.actionCount,"limits",List.of("Prepared cards/libraries/life; native and reference funding/setup protocols differ.","C05 printing toughness override explicit; P01 adversarial marks reference-only; C12 trusted primitive.","No native immutable-token/transaction protocol, mixed-category SBA, expiry, general discard, full-card/game/format or capacity claim."));Files.write(Paths.get(System.getProperty("spellwake.evidence.dir"),id+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),java.nio.file.StandardOpenOption.CREATE_NEW);System.out.println("CLEANUP "+id+" completed="+completed+" rows="+s.rows.size()+" priorities="+s.priorities.size());}}
    public static final class Script {
        final Game game;final String scenario;final Map<String,Player> seats=new LinkedHashMap<>();final Map<String,Card> all=new LinkedHashMap<>(),tracked=new LinkedHashMap<>();final Map<String,Object> metadata=new LinkedHashMap<>(),configuration;
        final List<Map<String,Object>> rows=new ArrayList<>(),choices=new ArrayList<>(),callbacks=new ArrayList<>(),actions=new ArrayList<>(),events=new ArrayList<>(),priorities=new ArrayList<>(),declarations=new ArrayList<>(),assignments=new ArrayList<>(),loops=new ArrayList<>();final java.util.Set<String> recorded=new java.util.HashSet<>();final java.util.Set<Integer> assigned=new java.util.HashSet<>();
        SpellAbility selected;Player payer;String sourceId,targetId,spend;Card manaSource;int actionCount,discardCount,orderCount,cleanupEntries;long ordinal;boolean done;Throwable eventFailure;
        Script(Game g,String id){game=g;scenario=id;for(Player p:g.getPlayers()){String a=p.getName().substring(8);seats.put(a,p);p.addController(g.getNextTimestamp(),p,new StrictController(g,p,this),false);Assert.assertFalse(p.getController().isAI());}configuration=map("turn_order",new ArrayList<>(seats.keySet()),"active",id.equals("C08")?"C":"A","seat_ids",seats.entrySet().stream().collect(java.util.stream.Collectors.toMap(Map.Entry::getKey,e->e.getValue().getId())),"life",life());}
        void observe(List<Map<String,Object>> list,Map<String,Object> value){value.put("observation_ordinal",ordinal++);list.add(value);}
        String alias(Player p){if(p==null)return null;for(var e:seats.entrySet())if(e.getValue()==p)return e.getKey();throw new AssertionError("Unknown player");}
        String playerId(int id){for(var e:seats.entrySet())if(e.getValue().getId()==id)return e.getKey();throw new AssertionError("Unknown player ID");}
        String label(Card c){return cardId(c.getId());}String cardId(int id){for(var e:all.entrySet())if(e.getValue().getId()==id)return e.getKey();throw new AssertionError("Unknown card ID "+id);}
        Card card(String id){Assert.assertTrue(all.containsKey(id),id);return game.getCardState(all.get(id));}
        List<String> sequence(Iterable<Card> cards){List<String> out=new ArrayList<>();for(Card c:cards)out.add(label(c));return out;}List<String> domain(Iterable<Card> cards){List<String> out=sequence(cards);java.util.Collections.sort(out);return out;}
        Map<String,Object> life(){Map<String,Object> out=new LinkedHashMap<>();for(var e:seats.entrySet())out.put(e.getKey(),String.valueOf(e.getValue().getLife()));return out;}
        Card seedCard(String id,String name,Player p,ZoneType z,boolean primary){Map<String,String> sets=Map.of("Counterspell","7ED","Arc Lightning","USG","Pyrite Spellbomb","MRD");Map<String,String> nums=Map.of("Island","335","Plains","331","Mountain","343","Craw Wurm","239","Hill Giant","194","Unsummon","112","Counterspell","67","Arc Lightning","174","Pyrite Spellbomb","232");String set=sets.getOrDefault(name,"8ED");IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name,set,nums.get(name));Assert.assertNotNull(paper,name);Assert.assertEquals(paper.getEdition(),set);Assert.assertEquals(paper.getCollectorNumber(),nums.get(name));Assert.assertEquals(paper.getName(),name);Card c=Card.fromPaperCard(paper,p);c.setGameTimestamp(game.getNextTimestamp());p.getZone(z).add(c);all.put(id,c);if(primary)tracked.put(id,c);metadata.put(id,map("name",name,"owner",alias(p),"zone",z.name(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText(),"mana_cost",paper.getRules().getManaCost().toString(),"type",paper.getRules().getType().toString(),"power",paper.getRules().getIntPower(),"toughness",paper.getRules().getIntToughness(),"card_id",c.getId(),"timestamp",c.getGameTimestamp(),"tracked",primary));return c;}
        void add(String id,String name,String owner,ZoneType z){seedCard(id,name,seats.get(owner),z,true);}
        void seed(){for(var e:seats.entrySet()){String a=e.getKey();add(a+"-old-bottom","Island",a,ZoneType.Graveyard);if(scenario.equals("C12")&&a.equals("A"))add("traveller","Hill Giant","A",ZoneType.Graveyard);add(a+"-old-top","Plains",a,ZoneType.Graveyard);for(int i=0;i<64;i++)seedCard(a+"-library-"+i,"Island",e.getValue(),ZoneType.Library,false);}
            int n=scenario.equals("C01")?10:scenario.equals("C02")?7:scenario.equals("C04")||scenario.equals("C05")?9:0;for(int i=0;i<n;i++)add("h"+i,i%2==0?"Island":"Plains","A",ZoneType.Hand);
            if(List.of("C04","C10","C11","P01").contains(scenario))add("giant","Hill Giant","A",ZoneType.Battlefield);
            if(scenario.equals("C04"))add("pyrite","Pyrite Spellbomb","B",ZoneType.Battlefield);
            if(scenario.equals("C05")){add("doomed","Hill Giant","A",ZoneType.Battlefield);add("returnee","Hill Giant","A",ZoneType.Battlefield);add("response","Unsummon","B",ZoneType.Hand);}
            if(scenario.equals("C06")||scenario.equals("C07")){add("a1","Craw Wurm","A",ZoneType.Battlefield);add("b1","Hill Giant","B",ZoneType.Battlefield);add("b2","Hill Giant","B",ZoneType.Battlefield);}
            if(scenario.equals("C08")){for(String x:List.of("a1","a2","c1","b1","b2","c2"))add(x,"Craw Wurm",x.substring(0,1).toUpperCase(),ZoneType.Battlefield);for(String x:List.of("a1","a2","c2")){Card c=card(x);Player next=seats.get(x.equals("c2")?"A":"C");c.setController(next,game.getNextTimestamp());game.getAction().controllerChangeZoneCorrection(c);observe(actions,map("kind","trusted-controller-and-zone-correction","card",x,"owner",alias(c.getOwner()),"controller",alias(c.getController()),"battlefield_zone_owner",alias(c.getZone().getPlayer()),"corrected",true));}}
            if(scenario.equals("C09")){add("response","Unsummon","A",ZoneType.Hand);add("counter","Counterspell","A",ZoneType.Hand);add("giant","Hill Giant","B",ZoneType.Battlefield);}
            if(scenario.equals("C10"))add("source","Arc Lightning","A",ZoneType.Hand);
            if(scenario.equals("C11")){add("pyrite1","Pyrite Spellbomb","A",ZoneType.Battlefield);add("pyrite2","Pyrite Spellbomb","A",ZoneType.Battlefield);}
            manaSource=seedCard("funding-attribution","Island",seats.get(scenario.equals("C04")||scenario.equals("C05")?"B":"A"),ZoneType.Battlefield,false);
        }
        Map<String,Object> control(){return map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"raw_priority",alias(game.getPhaseHandler().getPriorityPlayer()),"cleanup_entry",cleanupEntries-1,"stack_size",game.getStack().size());}
        Map<String,Object> facts(Card c){boolean bf=c.isInPlay();return map("name",c.getName(),"owner",alias(c.getOwner()),"controller",bf||c.isInZone(ZoneType.Stack)?alias(c.getController()):null,"zone",c.getZone().getZoneType().name(),"tapped",c.isTapped(),"power",bf&&c.isCreature()?String.valueOf(c.getNetPower()):null,"toughness",bf&&c.isCreature()?String.valueOf(c.getNetToughness()):null,"marked_damage",bf&&c.isCreature()?String.valueOf(c.getDamage()):null);}
        Map<String,Object> identity(Card c){return map("card_id",c.getId(),"timestamp",c.getGameTimestamp(),"java_identity",System.identityHashCode(c),"owner",alias(c.getOwner()),"zone",c.getZone().getZoneType().name());}
        List<Map<String,Object>> stack(){List<Map<String,Object>> out=new ArrayList<>();for(SpellAbilityStackInstance x:game.getStack()){SpellAbility sa=x.getSpellAbility();out.add(map("kind",sa.isSpell()?"Spell":"Ability","source",label(sa.getHostCard()),"controller",alias(sa.getActivatingPlayer()),"target_cards",sa.getTargets().getTargetCards().stream().map(this::label).toList(),"target_spells",java.util.stream.StreamSupport.stream(sa.getTargets().getTargetSpells().spliterator(),false).map(a->label(a.getHostCard())).toList()));}java.util.Collections.reverse(out);return out;}
        Map<String,Object> snapshot(){Map<String,Object> cards=new LinkedHashMap<>(),hands=new LinkedHashMap<>(),gy=new LinkedHashMap<>(),raw=new LinkedHashMap<>(),ids=new LinkedHashMap<>();for(String x:tracked.keySet()){cards.put(x,facts(card(x)));ids.put(x,identity(card(x)));}for(var e:seats.entrySet()){List<String> h=new ArrayList<>();for(Card c:e.getValue().getCardsIn(ZoneType.Hand))if(tracked.containsKey(label(c)))h.add(label(c));java.util.Collections.sort(h);hands.put(e.getKey(),h);List<String> b=sequence(e.getValue().getCardsIn(ZoneType.Graveyard));raw.put(e.getKey(),new ArrayList<>(b));java.util.Collections.reverse(b);gy.put(e.getKey(),b);}return map("cards",cards,"hands",hands,"graveyards",gy,"graveyards_raw_bottom_to_top",raw,"life",life(),"game_over",game.isGameOver(),"raw_identity",ids);}
        void row(String suffix,String protocol){String id=scenario+"-"+suffix;Assert.assertTrue(recorded.add(id),id);Map<String,Object> r=control();r.putAll(map("id",id,"protocol",protocol,"state",snapshot(),"stack",stack()));observe(rows,r);}
        boolean has(String suffix){return recorded.contains(scenario+"-"+suffix);}
        List<SpellAbility> choose(Player p){Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Map<String,Object> point=control();point.put("chooser",alias(p));point.put("world",snapshot());observe(priorities,point);PhaseType phase=game.getPhaseHandler().getPhase();boolean empty=game.getStack().isEmpty();String actor=alias(p);
            if(cleanupCase()&&game.getPhaseHandler().getTurn()==2&&phase==PhaseType.UPKEEP&&actor.equals("B")){row(scenario.equals("C05")?"after-repeat":"after-cleanup","actual-priority-callback");done=true;return null;}
            if((scenario.equals("C02")||scenario.equals("C03"))&&phase==PhaseType.END_OF_TURN&&actor.equals("A")&&!has("before-cleanup"))row("before-cleanup","actual-priority-callback");
            if(combatCase()&&phase==PhaseType.COMBAT_DAMAGE&&actor.equals(scenario.equals("C08")?"C":"A")&&empty&&!has("after-sba")){row("after-sba","actual-priority-callback");done=true;return null;}
            if(scenario.equals("C05")&&phase==PhaseType.CLEANUP&&actor.equals("A")&&empty){if(!has("sba-exception"))row("sba-exception","actual-exception-priority");if(actionCount==1&&card("response").isInZone(ZoneType.Graveyard)&&!has("bounce-resolved"))row("bounce-resolved","actual-exception-priority-after-resolution");}
            if(scenario.equals("C09")&&actor.equals("A")&&actionCount==2&&empty&&!has("after-counter")){row("after-counter","actual-priority-callback");done=true;return null;}
            if(scenario.equals("C10")&&actor.equals("A")&&actionCount==1&&empty&&!has("after-sba")){row("after-sba","actual-priority-callback");done=true;return null;}
            if(scenario.equals("C11")&&actor.equals("A")&&empty&&actionCount>0){if(actionCount==1&&!has("first-effect-settled"))row("first-effect-settled","actual-priority-callback");if(actionCount==2&&!has("after-sba")){row("after-sba","actual-priority-callback");done=true;return null;}}
            if(scenario.equals("C04")&&phase==PhaseType.END_OF_TURN&&actor.equals("B")&&actionCount==0)return prepare(p,"pyrite","giant","R",true);
            if(scenario.equals("C05")&&phase==PhaseType.CLEANUP&&actor.equals("B")&&actionCount==0)return prepare(p,"response","returnee","U",false);
            if(scenario.equals("C09")&&phase==PhaseType.MAIN1&&actor.equals("A")&&actionCount<2)return prepare(p,actionCount==0?"response":"counter",actionCount==0?"giant":"response",actionCount==0?"U":"UU",false);
            if(scenario.equals("C10")&&phase==PhaseType.MAIN1&&actor.equals("A")&&actionCount==0)return prepare(p,"source","giant","RRR",false);
            if(scenario.equals("C11")&&phase==PhaseType.MAIN1&&actor.equals("A")&&actionCount<2&&empty)return prepare(p,actionCount==0?"pyrite1":"pyrite2","giant","R",true);
            return null;
        }
        boolean cleanupCase(){return List.of("C01","C02","C03","C04","C05","P01").contains(scenario);}boolean combatCase(){return List.of("C06","C07","C08").contains(scenario);}
        List<SpellAbility> prepare(Player p,String src,String tgt,String colors,boolean activation){Assert.assertNull(selected);sourceId=src;targetId=tgt;spend=colors;payer=p;List<SpellAbility> domain=new ArrayList<>();if(activation){for(SpellAbility a:card(src).getNonManaAbilities())if(a.isActivatedAbility()&&a.getApi()==ApiType.DealDamage)domain.add(a);Assert.assertEquals(domain.size(),1);selected=domain.get(0);}else selected=card(src).getFirstSpellAbility();selected.setActivatingPlayer(p);for(char x:colors.toCharArray())p.getManaPool().addMana(new Mana(ManaAtom.fromName(String.valueOf(x)),manaSource,null,p));observe(actions,map("kind","trusted-floating-credit","actor",alias(p),"source",src,"colors",colors,"pool_after",String.valueOf(p.getManaPool().totalMana())));Assert.assertTrue(selected.canPlay());return List.of(selected);}
        boolean play(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);String src=sourceId;Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,sa));actionCount++;observe(actions,map("kind",sa.isSpell()?"actual-paid-cast":"actual-paid-activation","actor",alias(p),"source",src,"target",targetId,"cost",sa.getPayCosts().toString(),"colors",spend,"paid_mana",sa.getPayingMana().stream().map(Object::toString).toList(),"remaining_mana",String.valueOf(p.getManaPool().totalMana()),"stack",stack()));
            if(scenario.equals("C04"))row("cost-paid","actual-paid-admission");if(scenario.equals("C05"))row("bounce-admitted","actual-paid-admission");if(scenario.equals("C09"))row(actionCount==1?"target-spell-admitted":"counter-admitted","actual-paid-admission");if(scenario.equals("C10"))row("source-admitted","actual-paid-admission");if(scenario.equals("C11"))row(actionCount==1?"first-cost-paid":"second-cost-paid","actual-paid-admission");selected=null;return true;}
        SpellAbility select(Player p,Card host,List<SpellAbility> options){Assert.assertSame(p,payer);Assert.assertSame(host,selected.getHostCard());Assert.assertEquals(options.size(),1);Assert.assertSame(options.get(0),selected);observe(callbacks,map("kind","ability-selection","actor",alias(p),"source",sourceId,"domain_size",options.size(),"ability_id",selected.getId()));return selected;}
        boolean target(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);if(scenario.equals("C09")&&sourceId.equals("counter")){SpellAbility target=game.getStack().peekAbility();Assert.assertEquals(label(target.getHostCard()),"response");Assert.assertTrue(sa.canTargetSpellAbility(target));sa.getTargets().add(target);}else{Assert.assertTrue(sa.canTarget(card(targetId)));sa.getTargets().add(card(targetId));if(scenario.equals("C10")){sa.addDividedAllocation(card(targetId),3);Assert.assertEquals(sa.getTotalDividedValue(),3);}}Assert.assertTrue(sa.isTargetNumberValid());observe(callbacks,map("kind","target-selection","actor",alias(p),"source",sourceId,"target",targetId,"target_kind",sourceId.equals("counter")?"Spell":"Card","allocation",scenario.equals("C10")?"3":null));return true;}
        List<CostPart> order(Player p,List<CostPart> costs){Assert.assertSame(p,payer);for(CostPart c:costs)Assert.assertTrue(c instanceof CostPartMana||c instanceof CostSacrifice);observe(callbacks,map("kind","cost-order","actor",alias(p),"source",sourceId,"parts",costs.stream().map(c->c.getClass().getSimpleName()).toList()));return costs;}
        boolean pay(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);observe(callbacks,map("kind","mana-payment","actor",alias(p),"source",sourceId,"actual_cost",cost.toString()));return PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,false);}
        boolean apply(Player p,ManaCostBeingPaid cost,SpellAbility sa,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);try{for(char x:spend.toCharArray())Assert.assertTrue(p.getManaPool().tryPayCostWithColor(ManaAtom.fromName(String.valueOf(x)),sa,cost,sa.getPayingMana()));Assert.assertTrue(cost.isPaid());observe(callbacks,map("kind","mana-application","actor",alias(p),"source",sourceId,"colors",spend,"paid",cost.isPaid()));return true;}finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}}
        CardCollectionView discard(Player p,int count){Assert.assertEquals(alias(p),"A");Assert.assertEquals(game.getPhaseHandler().getPhase(),PhaseType.CLEANUP);Assert.assertTrue(game.getStack().isEmpty());List<String> wanted,eligible;if(scenario.equals("C01")){Assert.assertEquals(discardCount,0);Assert.assertEquals(count,3);wanted=List.of("h8","h2","h9");eligible=java.util.stream.IntStream.range(0,10).mapToObj(i->"h"+i).toList();row("discard-pending","actual-maximum-hand-discard-callback");}else if(scenario.equals("C04")){Assert.assertEquals(discardCount,0);Assert.assertEquals(count,2);wanted=List.of("h7","h2");eligible=java.util.stream.IntStream.range(0,9).mapToObj(i->"h"+i).toList();Assert.assertEquals(card("giant").getDamage(),2);row("discard-pending","actual-maximum-hand-discard-callback");}else{Assert.assertEquals(scenario,"C05");if(discardCount==0){Assert.assertEquals(count,2);wanted=List.of("h8","h1");eligible=java.util.stream.IntStream.range(0,9).mapToObj(i->"h"+i).toList();Assert.assertTrue(card("doomed").isInPlay());Assert.assertEquals(card("doomed").getNetToughness(),0);row("first-discard-pending","actual-maximum-hand-discard-callback");}else{Assert.assertEquals(discardCount,1);Assert.assertEquals(count,1);wanted=List.of("returnee");eligible=List.of("h0","h2","h3","h4","h5","h6","h7","returnee");row("second-discard-pending","actual-maximum-hand-discard-callback");}}List<String> d=new ArrayList<>(eligible);java.util.Collections.sort(d);Assert.assertEquals(domain(p.getCardsIn(ZoneType.Hand)),d);CardCollection result=new CardCollection();for(int i=wanted.size()-1;i>=0;i--)result.add(card(wanted.get(i)));observe(choices,map("kind","maximum-hand-discard","actor",alias(p),"domain",d,"input_sequence",sequence(p.getCardsIn(ZoneType.Hand)),"required",count,"selected_top_to_bottom",wanted,"returned_sequence",sequence(result),"destination","Graveyard","spell_cause",null,"cleanup_entry",cleanupEntries-1,"world",snapshot()));discardCount++;return result;}
        CardCollectionView zoneOrder(Player p,CardCollectionView cards,ZoneType dest,SpellAbility cause){Assert.assertTrue(combatCase());Assert.assertEquals(dest,ZoneType.Graveyard);Assert.assertNull(cause);Assert.assertTrue(cards.size()>1,"No singleton callback expected on pinned path");String owner=alias(p);List<String> eligible,wanted;if(scenario.equals("C08")){Assert.assertEquals(owner,List.of("C","B","A").get(orderCount));eligible=owner.equals("C")?List.of("c1","c2"):owner.equals("B")?List.of("b1","b2"):List.of("a1","a2");wanted=owner.equals("B")?List.of("b1","b2"):owner.equals("C")?List.of("c2","c1"):List.of("a2","a1");row("order-"+owner+"-pending","actual-owner-graveyard-order-callback");}else{Assert.assertEquals(orderCount,0);Assert.assertEquals(owner,"B");eligible=List.of("b1","b2");wanted=scenario.equals("C06")?List.of("b1","b2"):List.of("b2","b1");row("order-pending","actual-owner-graveyard-order-callback");}Assert.assertEquals(domain(cards),eligible);for(String x:eligible){Assert.assertTrue(card(x).isInPlay());Assert.assertEquals(alias(card(x).getOwner()),owner);}CardCollection out=new CardCollection();for(int i=wanted.size()-1;i>=0;i--)out.add(card(wanted.get(i)));observe(choices,map("kind","simultaneous-graveyard-order","actor",owner,"domain",domain(cards),"input_sequence",sequence(cards),"selected_top_to_bottom",wanted,"returned_sequence",sequence(out),"destination",dest.name(),"spell_cause",null,"world",snapshot()));orderCount++;return out;}
        void attackers(Player controller,Player actor,Combat combat){Assert.assertSame(controller,actor);List<Map<String,Object>> selected=new ArrayList<>();if(combatCase()){if(scenario.equals("C08")){attack(combat,"a1","B",selected);attack(combat,"a2","B",selected);attack(combat,"c1","A",selected);}else attack(combat,"a1","B",selected);}observe(declarations,map("kind","attackers","actor",alias(actor),"selected",selected,"world",snapshot()));}
        void attack(Combat combat,String c,String p,List<Map<String,Object>> out){Assert.assertTrue(forge.game.combat.CombatUtil.canAttack(card(c),seats.get(p)));combat.addAttacker(card(c),seats.get(p));out.add(map("source",c,"defender",p));}
        void blockers(Player controller,Player defender,Combat combat){Assert.assertTrue(combatCase());Assert.assertSame(controller,defender);List<Map<String,Object>> out=new ArrayList<>();if(scenario.equals("C08")){if(alias(controller).equals("B")){block(combat,"a1","b1",out);block(combat,"a2","b2",out);}else{Assert.assertEquals(alias(controller),"A");block(combat,"c1","c2",out);}}else{Assert.assertEquals(alias(controller),"B");block(combat,"a1","b1",out);block(combat,"a1","b2",out);}Assert.assertNull(forge.game.combat.CombatUtil.validateBlocks(combat,defender));observe(declarations,map("kind","blockers","actor",alias(controller),"selected",out,"world",snapshot()));}
        void block(Combat combat,String a,String b,List<Map<String,Object>> out){Assert.assertTrue(forge.game.combat.CombatUtil.canBlock(card(a),card(b),combat));combat.addBlocker(card(a),card(b));out.add(map("source",b,"attacker",a));}
        Map<Card,Integer> assign(Player p,Card source,CardCollectionView recipients,CardCollectionView remaining,int amount,GameEntity defender,boolean override){Assert.assertTrue(combatCase());Assert.assertTrue(override);Assert.assertTrue(assigned.add(source.getId()));Assert.assertSame(p,source.getController());String src=label(source);Map<Card,Integer> out=new LinkedHashMap<>();if(!scenario.equals("C08")&&src.equals("a1")){Assert.assertEquals(amount,6);Assert.assertEquals(domain(recipients),List.of("b1","b2"));out.put(card("b1"),3);out.put(card("b2"),3);}else{String tgt;if(scenario.equals("C08")){Map<String,String> pairs=Map.of("a1","b1","b1","a1","a2","b2","b2","a2","c1","c2","c2","c1");Assert.assertTrue(pairs.containsKey(src));tgt=pairs.get(src);Assert.assertEquals(amount,6);}else{Assert.assertTrue(src.equals("b1")||src.equals("b2"));tgt="a1";Assert.assertEquals(amount,3);}Assert.assertEquals(domain(recipients),List.of(tgt));out.put(card(tgt),amount);}observe(assignments,map("kind","assignment","actor",alias(p),"source",src,"domain",domain(recipients),"input_sequence",sequence(recipients),"amount",String.valueOf(amount),"selected",out.entrySet().stream().map(e->map("target",label(e.getKey()),"amount",String.valueOf(e.getValue()))).toList()));return out;}
        @Subscribe public void event(GameEvent v){if(eventFailure!=null)return;try{Map<String,Object> e=null;if(v instanceof GameEventTurnPhase x){e=map("kind","phase","step",x.phase().name(),"active",playerId(x.playerTurn().getId()),"marker",x.phaseDesc());if(x.phase()==PhaseType.CLEANUP){cleanupEntries++;if(cleanupEntries==1&&scenario.equals("C05")){card("doomed").setBaseToughness(0);observe(actions,map("kind","trusted-cleanup-entry-toughness-override","card","doomed","printed_toughness","3","prepared_toughness","0","world",snapshot()));}if(cleanupEntries==1&&scenario.equals("P01")){card("giant").setDamage(3);observe(actions,map("kind","trusted-cleanup-entry-damage-override","card","giant","amount","3","world",snapshot()));row("injected","trusted-natural-cleanup-entry-override");}}}
            if(v instanceof GameEventCardChangeZone x&&tracked.values().stream().anyMatch(c->c.getId()==x.card().getId()))e=map("kind","zone","card",cardId(x.card().getId()),"card_id",x.card().getId(),"from",x.from()==null?null:x.from().zoneType().name(),"to",x.to()==null?null:x.to().zoneType().name(),"world",snapshot());
            if(v instanceof GameEventCardDamaged x)e=map("kind","damage-card","source",cardId(x.source().getId()),"target",cardId(x.card().getId()),"amount",String.valueOf(x.amount()),"type",x.type().name());
            if(v instanceof GameEventSpellResolved x)e=map("kind","resolved","fizzled",x.hasFizzled());if(v instanceof GameEventPlayerPriority x)e=map("kind","priority-event","actor",playerId(x.priority().getId()),"step",x.phase().name());if(e!=null){e.put("control",control());observe(events,e);}
        }catch(Throwable t){eventFailure=t;}}
        void validate(){Assert.assertTrue(done);Assert.assertNull(eventFailure);Assert.assertFalse(game.isGameOver());Assert.assertFalse(game.getRules().hasOrderCombatants());int expectedDiscard=scenario.equals("C05")?2:List.of("C01","C04").contains(scenario)?1:0;Assert.assertEquals(discardCount,expectedDiscard);Assert.assertEquals(orderCount,scenario.equals("C08")?3:scenario.equals("C06")||scenario.equals("C07")?1:0);Assert.assertEquals(actionCount,scenario.equals("C09")||scenario.equals("C11")?2:List.of("C04","C05","C10").contains(scenario)?1:0);if(cleanupCase())Assert.assertEquals(cleanupEntries,scenario.equals("C05")?2:1);for(Player p:seats.values())Assert.assertEquals(p.getLife(),20);}
    }
    private static final class StrictCosts extends CostDecisionMakerBase {
        private final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script) { super(player,ability.isTrigger(),ability,ability.getHostCard()); this.script=script; }
        @Override public boolean paysRightAfterDecision() {return true;}
        @Override public PaymentDecision visit(CostTap cost) {throw new AssertionError("Unexpected tap cost");}
        @Override public PaymentDecision visit(CostPartMana cost) {return PaymentDecision.number(0);}
        @Override public PaymentDecision visit(CostBehold cost) {throw new AssertionError("Unexpected cost: CostBehold");}
        @Override public PaymentDecision visit(CostBeholdExile cost) {throw new AssertionError("Unexpected cost: CostBeholdExile");}
        @Override public PaymentDecision visit(CostGainControl cost) {throw new AssertionError("Unexpected cost: CostGainControl");}
        @Override public PaymentDecision visit(CostChooseColor cost) {throw new AssertionError("Unexpected cost: CostChooseColor");}
        @Override public PaymentDecision visit(CostChooseCreatureType cost) {throw new AssertionError("Unexpected cost: CostChooseCreatureType");}
        @Override public PaymentDecision visit(CostCollectEvidence cost) {throw new AssertionError("Unexpected cost: CostCollectEvidence");}
        @Override public PaymentDecision visit(CostDiscard cost) {throw new AssertionError("Unexpected cost: CostDiscard");}
        @Override public PaymentDecision visit(CostDamage cost) {throw new AssertionError("Unexpected cost: CostDamage");}
        @Override public PaymentDecision visit(CostDraw cost) {throw new AssertionError("Unexpected cost: CostDraw");}
        @Override public PaymentDecision visit(CostExile cost) {throw new AssertionError("Unexpected cost: CostExile");}
        @Override public PaymentDecision visit(CostExileFromStack cost) {throw new AssertionError("Unexpected cost: CostExileFromStack");}
        @Override public PaymentDecision visit(CostExiledMoveToGrave cost) {throw new AssertionError("Unexpected cost: CostExiledMoveToGrave");}
        @Override public PaymentDecision visit(CostExert cost) {throw new AssertionError("Unexpected cost: CostExert");}
        @Override public PaymentDecision visit(CostEnlist cost) {throw new AssertionError("Unexpected cost: CostEnlist");}
        @Override public PaymentDecision visit(CostFlipCoin cost) {throw new AssertionError("Unexpected cost: CostFlipCoin");}
        @Override public PaymentDecision visit(CostForage cost) {throw new AssertionError("Unexpected cost: CostForage");}
        @Override public PaymentDecision visit(CostRollDice cost) {throw new AssertionError("Unexpected cost: CostRollDice");}
        @Override public PaymentDecision visit(CostMill cost) {throw new AssertionError("Unexpected cost: CostMill");}
        @Override public PaymentDecision visit(CostAddMana cost) {throw new AssertionError("Unexpected cost: CostAddMana");}
        @Override public PaymentDecision visit(CostPayLife cost) {throw new AssertionError("Unexpected cost: CostPayLife");}
        @Override public PaymentDecision visit(CostPayEnergy cost) {throw new AssertionError("Unexpected cost: CostPayEnergy");}
        @Override public PaymentDecision visit(CostGainLife cost) {throw new AssertionError("Unexpected cost: CostGainLife");}
        @Override public PaymentDecision visit(CostPromiseGift cost) {throw new AssertionError("Unexpected cost: CostPromiseGift");}
        @Override public PaymentDecision visit(CostPutCardToLib cost) {throw new AssertionError("Unexpected cost: CostPutCardToLib");}
        @Override public PaymentDecision visit(CostSacrifice cost) {Assert.assertSame(ability,script.selected);Assert.assertSame(player,script.payer);Assert.assertEquals(source.getId(),script.card(script.sourceId).getId());Assert.assertEquals(source.getName(),"Pyrite Spellbomb");Assert.assertTrue(cost.payCostFromSource());Assert.assertEquals(cost.getAmount(),"1");Assert.assertTrue(cost.canPay(ability,player,false));script.observe(script.callbacks,map("kind","sacrifice-decision","actor",script.alias(player),"source",script.sourceId,"identity",script.identity(source)));return PaymentDecision.card(source);}
        @Override public PaymentDecision visit(CostReturn cost) {throw new AssertionError("Unexpected cost: CostReturn");}
        @Override public PaymentDecision visit(CostReveal cost) {throw new AssertionError("Unexpected cost: CostReveal");}
        @Override public PaymentDecision visit(CostRevealChosen cost) {throw new AssertionError("Unexpected cost: CostRevealChosen");}
        @Override public PaymentDecision visit(CostRemoveAnyCounter cost) {throw new AssertionError("Unexpected cost: CostRemoveAnyCounter");}
        @Override public PaymentDecision visit(CostRemoveCounter cost) {throw new AssertionError("Unexpected cost: CostRemoveCounter");}
        @Override public PaymentDecision visit(CostPutCounter cost) {throw new AssertionError("Unexpected cost: CostPutCounter");}
        @Override public PaymentDecision visit(CostPutCounterYou cost) {throw new AssertionError("Unexpected cost: CostPutCounterYou");}
        @Override public PaymentDecision visit(CostUntapType cost) {throw new AssertionError("Unexpected cost: CostUntapType");}
        @Override public PaymentDecision visit(CostUntap cost) {throw new AssertionError("Unexpected cost: CostUntap");}
        @Override public PaymentDecision visit(CostUnattach cost) {throw new AssertionError("Unexpected cost: CostUnattach");}
        @Override public PaymentDecision visit(CostTapType cost) {throw new AssertionError("Unexpected cost: CostTapType");}
        @Override public PaymentDecision visit(CostPayShards cost) {throw new AssertionError("Unexpected cost: CostPayShards");}
        @Override public PaymentDecision visit(CostBlight cost) {throw new AssertionError("Unexpected cost: CostBlight");}
    }
    /** Pinned controller surface. Every choice is explicit and all other decisions throw. */
    private static final class StrictController extends PlayerController {
        private final Script script;
        StrictController(Game game, Player player, Script script) {
            super(game, player, player.getLobbyPlayer());
            this.script = script;
        }
        @Override public SpellAbility getAbilityToPlay(Card hostCard, List<SpellAbility> abilities, ITriggerEvent triggerEvent) { return script.select(getPlayer(), hostCard, abilities); }
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw new AssertionError("Unexpected replacement or no-stack effect"); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.size()<=1); return activePlayerSAs; }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.isEmpty(), "Unexpected trigger"); }
        @Override public boolean playTrigger(Card host, WrappedAbility wrapperAbility, boolean isMandatory) { throw new AssertionError("Unexpected controller operation: playTrigger"); }
        @Override public boolean playSaFromPlayEffect(SpellAbility tgtSA) { throw new AssertionError("Unexpected controller operation: playSaFromPlayEffect"); }
        @Override public List<PaperCard> sideboard(final Deck deck, GameType gameType, String message) { throw new AssertionError("Unexpected controller operation: sideboard"); }
        @Override public List<PaperCard> chooseCardsYouWonToAddToDeck(List<PaperCard> losses) { throw new AssertionError("Unexpected controller operation: chooseCardsYouWonToAddToDeck"); }
        @Override public Map<Card, Integer> assignCombatDamage(Card attacker, CardCollectionView blockers, CardCollectionView remaining, int damageDealt, GameEntity defender, boolean overrideOrder) { return script.assign(getPlayer(),attacker,blockers,remaining,damageDealt,defender,overrideOrder); }
        @Override public Map<GameEntity, Integer> divideShield(Card effectSource, Map<GameEntity, Integer> affected, int shieldAmount) { throw new AssertionError("Unexpected controller operation: divideShield"); }
        @Override public Map<Byte, Integer> specifyManaCombo(SpellAbility sa, ColorSet colorSet, int manaAmount, boolean different) { throw new AssertionError("Unexpected controller operation: specifyManaCombo"); }
        @Override public CardCollectionView choosePermanentsToSacrifice(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw new AssertionError("Unexpected controller operation: choosePermanentsToSacrifice"); }
        @Override public CardCollectionView choosePermanentsToDestroy(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw new AssertionError("Unexpected controller operation: choosePermanentsToDestroy"); }
        @Override public Integer announceRequirements(SpellAbility ability, int min, int max, String announce) { throw new AssertionError("Unexpected controller operation: announceRequirements"); }
        @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) { throw new AssertionError("Unexpected controller operation: chooseNewTargetsFor"); }
        @Override public boolean chooseTargetsFor(SpellAbility currentAbility) { return script.target(getPlayer(),currentAbility); }
        @Override public Pair<SpellAbilityStackInstance, GameObject> chooseTarget(SpellAbility sa, List<Pair<SpellAbilityStackInstance, GameObject>> allTargets) { throw new AssertionError("Unexpected controller operation: chooseTarget"); }
        @Override public boolean helpPayForAssistSpell(ManaCostBeingPaid cost, SpellAbility sa, int max, int requested) { throw new AssertionError("Unexpected controller operation: helpPayForAssistSpell"); }
        @Override public Player choosePlayerToAssistPayment(FCollectionView<Player> optionList, SpellAbility sa, String title, int max) { throw new AssertionError("Unexpected controller operation: choosePlayerToAssistPayment"); }
        @Override public CardCollectionView chooseCardsForEffect(CardCollectionView sourceList, SpellAbility sa, String title, int min, int max, boolean isOptional, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseCardsForEffect"); }
        @Override public CardCollection chooseCardsForEffectMultiple(Map<String, CardCollection> validMap, SpellAbility sa, String title, boolean isOptional) { throw new AssertionError("Unexpected controller operation: chooseCardsForEffectMultiple"); }
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { throw new AssertionError("Unexpected source choice"); }
        @Override public <T extends GameEntity> List<T> chooseEntitiesForEffect(FCollectionView<T> optionList, int min, int max, DelayedReveal delayedReveal, SpellAbility sa, String title, Player relatedPlayer, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseEntitiesForEffect"); }
        @Override public List<SpellAbility> chooseSpellAbilitiesForEffect(List<SpellAbility> spells, SpellAbility sa, String title, int num, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSpellAbilitiesForEffect"); }
        @Override public SpellAbility chooseSingleSpellForEffect(List<SpellAbility> spells, SpellAbility sa, String title, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSingleSpellForEffect"); }
        @Override public boolean confirmAction(SpellAbility sa, PlayerActionConfirmMode mode, String message, List<String> options, Card cardToShow, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: confirmAction"); }
        @Override public boolean confirmBidAction(SpellAbility sa, PlayerActionConfirmMode bidlife, String string, int bid, Player winner) { throw new AssertionError("Unexpected controller operation: confirmBidAction"); }
        @Override public boolean confirmReplacementEffect(ReplacementEffect replacementEffect, SpellAbility effectSA, GameEntity affected, String question) { throw new AssertionError("Unexpected controller operation: confirmReplacementEffect"); }
        @Override public boolean confirmStaticApplication(Card hostCard, PlayerActionConfirmMode mode, String message, String logic) { throw new AssertionError("Unexpected controller operation: confirmStaticApplication"); }
        @Override public boolean confirmTrigger(WrappedAbility sa) { throw new AssertionError("Unexpected controller operation: confirmTrigger"); }
        @Override public List<Card> exertAttackers(List<Card> attackers) { throw new AssertionError("Unexpected controller operation: exertAttackers"); }
        @Override public List<Card> enlistAttackers(List<Card> attackers) { throw new AssertionError("Unexpected controller operation: enlistAttackers"); }
        @Override public void declareAttackers(Player attacker, Combat combat) { script.attackers(getPlayer(),attacker,combat); }
        @Override public void declareBlockers(Player defender, Combat combat) { script.blockers(getPlayer(),defender,combat); }
        @Override public CardCollection orderBlockers(Card attacker, CardCollection blockers) { throw new AssertionError("Unexpected controller operation: orderBlockers"); }
        @Override public CardCollection orderBlocker(final Card attacker, final Card blocker, final CardCollection oldBlockers) { throw new AssertionError("Unexpected controller operation: orderBlocker"); }
        @Override public CardCollection orderAttackers(Card blocker, CardCollection attackers) { throw new AssertionError("Unexpected controller operation: orderAttackers"); }
        @Override public void reveal(CardCollectionView cards, ZoneType zone, Player owner, String messagePrefix, boolean addMsgSuffix) { /* notification only; no player decision */ }
        @Override public void reveal(List<CardView> cards, ZoneType zone, PlayerView owner, String messagePrefix, boolean addMsgSuffix) { /* notification only; no player decision */ }
        @Override public void notifyOfValue(SpellAbility saSource, GameObject realtedTarget, String value) { /* notification only; no player decision */ }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForScry(CardCollection topN) { throw new AssertionError("Unexpected controller operation: arrangeForScry"); }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForSurveil(CardCollection topN) { throw new AssertionError("Unexpected controller operation: arrangeForSurveil"); }
        @Override public boolean willPutCardOnTop(Card c) { throw new AssertionError("Unexpected controller operation: willPutCardOnTop"); }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destinationZone, SpellAbility source) { return script.zoneOrder(getPlayer(),cards,destinationZone,source); }
        @Override public CardCollectionView chooseCardsToDiscardFrom(Player playerDiscard, SpellAbility sa, CardCollection validCards, int min, int max, CardCollectionView visibleToChooser) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardFrom"); }
        @Override public CardCollectionView chooseCardsToDiscardUnlessType(int min, CardCollectionView hand, String[] unlessTypes, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardUnlessType"); }
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { return script.discard(getPlayer(),numDiscard); }
        @Override public CardCollectionView chooseCardsToDelve(int genericAmount, CardCollection grave) { throw new AssertionError("Unexpected controller operation: chooseCardsToDelve"); }
        @Override public Map<Card, ManaCostShard> chooseCardsForConvokeOrImprovise(SpellAbility sa, ManaCost manaCost, CardCollectionView untappedCards, boolean artifacts, boolean creatures, Integer maxReduction) { throw new AssertionError("Unexpected controller operation: chooseCardsForConvokeOrImprovise"); }
        @Override public List<Card> chooseCardsForSplice(SpellAbility sa, List<Card> cards) { throw new AssertionError("Unexpected controller operation: chooseCardsForSplice"); }
        @Override public CardCollectionView chooseCardsToRevealFromHand(int min, int max, CardCollectionView valid) { throw new AssertionError("Unexpected controller operation: chooseCardsToRevealFromHand"); }
        @Override public List<SpellAbility> chooseSaToActivateFromOpeningHand(List<SpellAbility> usableFromOpeningHand) { throw new AssertionError("Unexpected controller operation: chooseSaToActivateFromOpeningHand"); }
        @Override public Player chooseStartingPlayer(boolean isFirstGame) { throw new AssertionError("Unexpected controller operation: chooseStartingPlayer"); }
        @Override public PlayerZone chooseStartingHand(List<PlayerZone> zones) { throw new AssertionError("Unexpected controller operation: chooseStartingHand"); }
        @Override public Mana chooseManaFromPool(List<Mana> manaChoices) { throw new AssertionError("Unexpected controller operation: chooseManaFromPool"); }
        @Override public String chooseSomeType(String kindOfType, SpellAbility sa, Collection<String> validTypes, boolean isOptional) { throw new AssertionError("Unexpected controller operation: chooseSomeType"); }
        @Override public String chooseSector(Card assignee, String ai, List<String> sectors) { throw new AssertionError("Unexpected controller operation: chooseSector"); }
        @Override public Sticker chooseSticker(List<Sticker> options, Card target, SpellAbility sa, boolean isOptional) { throw new AssertionError("Unexpected controller operation: chooseSticker"); }
        @Override public int chooseStickerNamePosition(Sticker sticker, Card target) { throw new AssertionError("Unexpected controller operation: chooseStickerNamePosition"); }
        @Override public Card chooseCardToKeepStickers(CardCollectionView options) { throw new AssertionError("Unexpected controller operation: chooseCardToKeepStickers"); }
        @Override public List<Card> chooseContraptionsToCrank(List<Card> contraptions) { throw new AssertionError("Unexpected controller operation: chooseContraptionsToCrank"); }
        @Override public int chooseSprocket(Card assignee, List<Integer> sprockets) { throw new AssertionError("Unexpected controller operation: chooseSprocket"); }
        @Override public PlanarDice choosePDRollToIgnore(List<PlanarDice> rolls) { throw new AssertionError("Unexpected controller operation: choosePDRollToIgnore"); }
        @Override public Integer chooseRollToIgnore(List<Integer> rolls) { throw new AssertionError("Unexpected controller operation: chooseRollToIgnore"); }
        @Override public List<Integer> chooseDiceToReroll(List<Integer> rolls) { throw new AssertionError("Unexpected controller operation: chooseDiceToReroll"); }
        @Override public Integer chooseRollToModify(List<Integer> rolls) { throw new AssertionError("Unexpected controller operation: chooseRollToModify"); }
        @Override public RollDiceEffect.DieRollResult chooseRollToSwap(List<RollDiceEffect.DieRollResult> rolls) { throw new AssertionError("Unexpected controller operation: chooseRollToSwap"); }
        @Override public String chooseRollSwapValue(List<String> swapChoices, Integer currentResult, int power, int toughness) { throw new AssertionError("Unexpected controller operation: chooseRollSwapValue"); }
        @Override public Object vote(SpellAbility sa, String prompt, List<Object> options, ListMultimap<Object, Player> votes, Player forPlayer, boolean optional) { throw new AssertionError("Unexpected controller operation: vote"); }
        @Override public boolean mulliganKeepHand(Player player, int cardsToReturn) { throw new AssertionError("Unexpected controller operation: mulliganKeepHand"); }
        @Override public CardCollectionView tuckCardsViaMulligan(CardCollectionView hand, int cardsToReturn) { throw new AssertionError("Unexpected controller operation: tuckCardsViaMulligan"); }
        @Override public List<SpellAbility> chooseSpellAbilityToPlay() { return script.choose(getPlayer()); }
        @Override public boolean playChosenSpellAbility(SpellAbility sa) { return script.play(getPlayer(),sa); }
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { throw new AssertionError("Unexpected modal choice"); }
        @Override public int chooseNumberForCostReduction(final SpellAbility sa, final int min, final int max) { throw new AssertionError("Unexpected controller operation: chooseNumberForCostReduction"); }
        @Override public int chooseNumberForKeywordCost(SpellAbility sa, Cost cost, KeywordInterface keyword, String prompt, int max) { throw new AssertionError("Unexpected controller operation: chooseNumberForKeywordCost"); }
        @Override public int chooseNumber(SpellAbility sa, String title, int min, int max) { throw new AssertionError("Unexpected controller operation: chooseNumber"); }
        @Override public int chooseNumber(SpellAbility sa, String title, List<Integer> values, Player relatedPlayer) { throw new AssertionError("Unexpected controller operation: chooseNumber"); }
        @Override public boolean chooseBinary(SpellAbility sa, String question, BinaryChoiceType kindOfChoice, Boolean defaultChoice) { throw new AssertionError("Unexpected controller operation: chooseBinary"); }
        @Override public boolean chooseFlipResult(SpellAbility sa, Player flipper, boolean call) { throw new AssertionError("Unexpected controller operation: chooseFlipResult"); }
        @Override public byte chooseColor(String message, SpellAbility sa, ColorSet colors) { throw new AssertionError("Unexpected controller operation: chooseColor"); }
        @Override public byte chooseColorAllowColorless(String message, Card c, ColorSet colors) { throw new AssertionError("Unexpected controller operation: chooseColorAllowColorless"); }
        @Override public ColorSet chooseColors(String message, SpellAbility sa, int min, int max, ColorSet options) { throw new AssertionError("Unexpected controller operation: chooseColors"); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, String message, Predicate<ICardFace> cpp, String name) { throw new AssertionError("Unexpected controller operation: chooseSingleCardFace"); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseSingleCardFace"); }
        @Override public CardState chooseSingleCardState(SpellAbility sa, List<CardState> states, String message, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSingleCardState"); }
        @Override public boolean chooseCardsPile(SpellAbility sa, CardCollectionView pile1, CardCollectionView pile2, String faceUp) { throw new AssertionError("Unexpected controller operation: chooseCardsPile"); }
        @Override public CounterType chooseCounterType(List<CounterType> options, SpellAbility sa, String prompt, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseCounterType"); }
        @Override public String chooseKeywordForPump(List<String> options, SpellAbility sa, String prompt, Card tgtCard) { throw new AssertionError("Unexpected controller operation: chooseKeywordForPump"); }
        @Override public boolean confirmPayment(CostPart costPart, String string, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: confirmPayment"); }
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { throw new AssertionError("Unexpected replacement choice"); }
        @Override public StaticAbility chooseSingleStaticAbility(List<StaticAbility> possibleReplacers) { throw new AssertionError("Unexpected controller operation: chooseSingleStaticAbility"); }
        @Override public String chooseProtectionType(SpellAbility sa, List<String> choices) { throw new AssertionError("Unexpected controller operation: chooseProtectionType"); }
        @Override public void revealAnte(String message, Multimap<Player, PaperCard> removedAnteCards) { throw new AssertionError("Unexpected controller operation: revealAnte"); }
        @Override public void revealAISkipCards(String message, Map<Player, Map<DeckSection, List<? extends PaperCard>>> deckCards) { throw new AssertionError("Unexpected controller operation: revealAISkipCards"); }
        @Override public void revealUnsupported(Map<Player, List<PaperCard>> unsupported) { throw new AssertionError("Unexpected controller operation: revealUnsupported"); }
        @Override public List<OptionalCostValue> chooseOptionalCosts(SpellAbility choosen, List<OptionalCostValue> optionalCostValues) { throw new AssertionError("Unexpected controller operation: chooseOptionalCosts"); }
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { return script.order(getPlayer(),costs); }
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { throw new AssertionError("Unexpected controller operation: payCostToPreventEffect"); }
        @Override public boolean payCostDuringRoll(Cost cost, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: payCostDuringRoll"); }
        @Override public boolean payCombatCost(Card card, Cost cost, SpellAbility sa, String prompt) { throw new AssertionError("Unexpected controller operation: payCombatCost"); }
        @Override public boolean payManaCost(ManaCost toPay, CostPartMana costPartMana, SpellAbility sa, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.pay(getPlayer(),toPay,costPartMana,sa,prompt,matrix,effect); }
        @Override public boolean applyManaToCost(ManaCostBeingPaid toPay, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { return script.apply(getPlayer(),toPay,ability,effect); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw new AssertionError("Unexpected controller operation: chooseCardsForCost"); }
        @Override public CostDecisionMakerBase getCostDecisionMaker(Player player, SpellAbility ability, boolean effect, String prompt) { Assert.assertEquals(player, script.payer); Assert.assertSame(ability, script.selected); Assert.assertFalse(effect); return new StrictCosts(player, ability, script); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw new AssertionError("Unexpected controller operation: chooseCardName"); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw new AssertionError("Unexpected controller operation: chooseSingleCardForZoneChange"); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw new AssertionError("Unexpected controller operation: chooseCardsForZoneChange"); }
        @Override public void autoPassCancel() { /* notification only; no player decision */ }
        @Override public void awaitNextInput() { /* notification only; no player decision */ }
        @Override public void cancelAwaitNextInput() { /* notification only; no player decision */ }
    }
}
