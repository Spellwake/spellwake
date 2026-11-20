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




import forge.game.ability.AbilityKey;
import forge.game.ability.AbilityUtils;
import forge.game.ability.ApiType;
import java.security.MessageDigest;

/** Strict actual-loop combat probes. Frozen source expectations are external. */
public class SharedDamageReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA="81e98b9bd60342eec10eb53c7d3e1b71bc33c2d3fb2c8e4a15a3b785ef4bab6c";
    @Test public void unblockedLifeDamage() throws Exception {run("D01");}
    @Test public void simultaneousMutualLethal() throws Exception {run("D02");}
    @Test public void multipleBlockersFreeOneFive() throws Exception {run("D03");}
    @Test public void multipleBlockersSparseSixZero() throws Exception {run("D04");}
    @Test public void blockedAttackerAfterBlockerBounce() throws Exception {run("D05");}
    @Test public void orphanBlockerAfterAttackerBounce() throws Exception {run("D06");}
    @Test public void removedSoleAttackerKeepsHistory() throws Exception {run("D07");}
    @Test public void paidGrowthChangesCurrentCombatPower() throws Exception {run("D08");}
    @Test public void combatLethalPlayerOutcome() throws Exception {run("D09");}
    @Test public void threeSeatMultipleDefenders() throws Exception {run("D10");}
    @Test public void fourSeatMultipleDefenders() throws Exception {run("D11");}
    @Test public void singleSourceShieldForcedAllocation() throws Exception {run("D12");}
    @Test public void sharedShieldWholeFirstSource() throws Exception {run("D13");}
    @Test public void sharedShieldWholeSecondSource() throws Exception {run("D14");}
    private static Map<String,Object> map(Object... pairs){Map<String,Object> out=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)out.put((String)pairs[i],pairs[i+1]);return out;}
    private static String hex(byte[] raw)throws Exception{byte[] digest=MessageDigest.getInstance("SHA-256").digest(raw);StringBuilder s=new StringBuilder();for(byte b:digest)s.append(String.format("%02x",b&255));return s.toString();}
    private static List<Map<String,Object>> runtime()throws Exception{
        List<Map<String,Object>> out=new ArrayList<>();String[] names={"forge.ai.SharedDamageReferenceTest","forge.ai.SharedDamageReferenceTest$Script","forge.ai.SharedDamageReferenceTest$StrictController","forge.ai.SharedDamageReferenceTest$StrictCosts","forge.game.Game","forge.game.GameAction","forge.game.GameRules","forge.game.phase.PhaseHandler","forge.game.combat.Combat","forge.game.combat.CombatUtil","forge.game.card.Card","forge.game.card.CardDamageTable","forge.game.player.Player","forge.game.player.PlayerController","forge.game.player.PlaySpellAbility","forge.game.replacement.ReplacementHandler","forge.game.replacement.ReplaceDamage","forge.game.ability.effects.DamagePreventEffect","forge.game.ability.effects.ReplaceDamageEffect","forge.game.ability.effects.PumpEffect","forge.game.ability.effects.ChangeZoneEffect"};
        for(String name:names){Class<?> c=Class.forName(name);String resource="/"+name.replace('.','/')+".class";byte[] raw;try(java.io.InputStream in=c.getResourceAsStream(resource)){Assert.assertNotNull(in);raw=in.readAllBytes();}java.nio.file.Path dest=Paths.get(System.getProperty("spellwake.evidence.dir"),"loader-classes",name.replace('.','/')+".class");Files.createDirectories(dest.getParent());if(Files.exists(dest))Assert.assertEquals(hex(Files.readAllBytes(dest)),hex(raw));else Files.write(dest,raw,java.nio.file.StandardOpenOption.CREATE_NEW);out.add(map("class",name,"source",c.getProtectionDomain().getCodeSource().getLocation().toString(),"resource",c.getResource(resource).toString(),"sha256",hex(raw)));}return out;
    }
    private Game fresh(String id){
        String[] order=id.equals("D11")?new String[]{"A","D","C","B"}:id.equals("D10")?new String[]{"A","C","B"}:new String[]{"A","B"};Map<String,Integer> ids=Map.of("A",41,"B",3,"C",99,"D",7);List<RegisteredPlayer> players=new ArrayList<>();
        for(int i=0;i<order.length;i++){RegisteredPlayer p=new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-"+order[i],null));p.setId(ids.get(order[i]));p.setStartingLife(id.equals("D09")&&order[i].equals("B")?6:20);p.setTeamNumber(i+1);players.add(p);}GameRules rules=new GameRules(GameType.Constructed);rules.setOrderCombatants(false);Game g=new Game(players,rules,new Match(rules,players,"Shared damage reference"));g.setAge(GameStage.Play);return g;
    }
    private void run(String id)throws Exception{
        Script s=new Script(fresh(id),id);boolean completed=false;List<Map<String,Object>> runtime=runtime();
        try{s.seed();s.game.subscribeToEvents(s);s.game.getPhaseHandler().setupFirstTurn(s.seats.get("A"),null);
            for(int i=0;i<240;i++){Assert.assertFalse(s.game.isGameOver());int before=s.priorities.size();s.game.getPhaseHandler().mainLoopStep();s.observe(s.loops,map("ordinal",i,"priority_callbacks",s.priorities.size()-before,"after",s.control()));if(s.eventFailure!=null)throw new AssertionError("Observer failure",s.eventFailure);if(s.game.isGameOver()){Assert.assertEquals(id,"D09");Assert.assertNotNull(s.game.getOutcome());s.rows.add(map("id",id+"-outcome","protocol","actual-game-outcome","comparison_scope","common-outcome","life",s.life(),"game_over",true,"winner",s.game.getOutcome().getWinningLobbyPlayer().getName().substring(8),"losers",List.of("B"),"observation_ordinal",s.ordinal++));s.done=true;}if(s.done)break;Assert.assertTrue(i<239,"bounded loop exhausted");}Assert.assertTrue(s.done);completed=true;
        }finally{Map<String,Object> trace=map("schema_version",1,"profile","shared-damage","scenario",id,"commit",COMMIT,"expected_sha256",EXPECTED_SHA,"completed",completed,"strict_choices",true,"ai_policy_used",false,"priority_loop_executed",true,"dev_phase_advance_used",false,"direct_damage_used",false,"direct_resolution_used",false,"order_combatants",s.game.getRules().hasOrderCombatants(),"configuration",s.initialConfiguration,"final_configuration",s.configuration(),"rows",s.rows,"declarations",s.declarations,"assignments",s.assignments,"choices",s.choices,"actions",s.actions,"events",s.events,"priorities",s.priorities,"loops",s.loops,"metadata",s.metadata,"runtime",runtime,"ever_declared_attackers",s.everAttacked,"limits",List.of("Prepared physical cards and explicit starting life; actual ordinary combat/paid response loops.","Floating Forge funding differs from native land production.","D05-D07 empty-damage priority and historical skip adjudicated separately.","Interior shield allocations remain native-only; no complete-card/game/format claim."));Files.write(Paths.get(System.getProperty("spellwake.evidence.dir"),id+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),java.nio.file.StandardOpenOption.CREATE_NEW);System.out.println("SHARED_DAMAGE "+id+" completed="+completed+" rows="+s.rows.size()+" priorities="+s.priorities.size());}
    }
    public static final class Script {
        final Game game;final String scenario;final Map<String,Object> initialConfiguration;final Map<String,Player> seats=new LinkedHashMap<>();final Map<String,Card> observed=new LinkedHashMap<>();final Map<String,Object> metadata=new LinkedHashMap<>();
        final List<Map<String,Object>> rows=new ArrayList<>(),declarations=new ArrayList<>(),assignments=new ArrayList<>(),choices=new ArrayList<>(),actions=new ArrayList<>(),events=new ArrayList<>(),priorities=new ArrayList<>(),loops=new ArrayList<>();final java.util.Set<String> recorded=new java.util.HashSet<>();final java.util.Set<Integer> assigned=new java.util.HashSet<>();
        SpellAbility selected;Player payer;String spend,targetId;Card manaSource;boolean responseCast,done,everAttacked;Throwable eventFailure;long ordinal;
        Script(Game g,String id){game=g;scenario=id;for(Player p:g.getPlayers()){String a=p.getName().substring(8);seats.put(a,p);p.addController(game.getNextTimestamp(),p,new StrictController(game,p,this),false);Assert.assertFalse(p.getController().isAI());}Assert.assertFalse(game.getRules().hasOrderCombatants());initialConfiguration=configuration();}
        void observe(List<Map<String,Object>> target,Map<String,Object> value){value.put("observation_ordinal",ordinal++);target.add(value);}
        boolean multi(){return scenario.equals("D10")||scenario.equals("D11");}boolean two(){return List.of("D03","D04","D13","D14").contains(scenario);}boolean shield(){return List.of("D12","D13","D14").contains(scenario);}boolean response(){return List.of("D05","D06","D07","D08","D12","D13","D14").contains(scenario);}
        String alias(Player p){if(p==null)return null;for(var e:seats.entrySet())if(e.getValue()==p)return e.getKey();throw new AssertionError("Unknown player "+p);}
        String playerId(int id){for(var e:seats.entrySet())if(e.getValue().getId()==id)return e.getKey();throw new AssertionError("Unknown seat ID");}
        Card current(Card c){return game.getCardState(c);}Card card(String id){return current(observed.get(id));}
        String cardId(int id){for(var e:observed.entrySet())if(e.getValue().getId()==id)return e.getKey();return "support-"+id;}
        String label(Card c){return cardId(c.getId());}List<String> labels(Iterable<Card> cs){List<String> out=new ArrayList<>();for(Card c:cs)out.add(label(c));java.util.Collections.sort(out);return out;}
        Map<String,Object> life(){Map<String,Object> out=new LinkedHashMap<>();for(var e:seats.entrySet())out.put(e.getKey(),String.valueOf(e.getValue().getLife()));return out;}
        Map<String,Object> configuration(){List<String> order=new ArrayList<>();Map<String,Object> ps=new LinkedHashMap<>();for(Player p:game.getPlayersInTurnOrder()){order.add(alias(p));List<String> opponents=new ArrayList<>();for(Player q:p.getOpponents())opponents.add(alias(q));java.util.Collections.sort(opponents);ps.put(alias(p),map("id",p.getId(),"team",p.getTeam(),"opponents",opponents));}return map("turn_order",order,"players",ps,"policy","UnrestrictedFreeForAll");}
        Card seedCard(String id,String name,Player owner,ZoneType zone,boolean primary){Map<String,String> numbers=Map.of("Craw Wurm","239","Hill Giant","194","Healing Salve","22","Giant Growth","254","Unsummon","112","Island","335");IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name,"8ED",numbers.get(name));Assert.assertNotNull(paper);Assert.assertEquals(paper.getEdition(),"8ED");Assert.assertEquals(paper.getCollectorNumber(),numbers.get(name));Assert.assertEquals(paper.getName(),name);Card c=Card.fromPaperCard(paper,owner);c.setGameTimestamp(game.getNextTimestamp());owner.getZone(zone).add(c);Assert.assertFalse(c.isStartsGameInPlay());metadata.put(id,map("name",name,"owner",alias(owner),"zone",zone.name(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText(),"mana_cost",paper.getRules().getManaCost().toString(),"type",paper.getRules().getType().toString(),"power",paper.getRules().getIntPower(),"toughness",paper.getRules().getIntToughness(),"card_id",c.getId(),"timestamp",c.getGameTimestamp(),"primary",primary,"starts_game_in_play",false));if(primary)observed.put(id,c);return c;}
        void seed(){Player a=seats.get("A"),b=seats.get("B");seedCard("a1","Craw Wurm",a,ZoneType.Battlefield,true);seedCard("b1",List.of("D02","D08").contains(scenario)?"Craw Wurm":"Hill Giant",b,ZoneType.Battlefield,true);if(two())seedCard("b2","Hill Giant",b,ZoneType.Battlefield,true);if(multi()){seedCard("a2","Hill Giant",a,ZoneType.Battlefield,true);seedCard("c1","Hill Giant",seats.get("C"),ZoneType.Battlefield,true);if(scenario.equals("D11"))seedCard("d1","Hill Giant",seats.get("D"),ZoneType.Battlefield,true);}if(response())seedCard("response",shield()?"Healing Salve":scenario.equals("D08")?"Giant Growth":"Unsummon",scenario.equals("D08")||shield()?a:b,ZoneType.Hand,true);manaSource=seedCard("funding-attribution","Island",a,ZoneType.Battlefield,false);for(var e:seats.entrySet())for(int i=0;i<8;i++)seedCard(e.getKey()+"-library-"+i,"Island",e.getValue(),ZoneType.Library,false);}
        Map<String,Object> control(){return map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"raw_priority",alias(game.getPhaseHandler().getPriorityPlayer()),"stack_size",game.getStack().size());}
        Map<String,Object> facts(Card input){Card c=current(input);boolean bf=c.isInPlay();Combat combat=game.getPhaseHandler().getCombat();boolean at=bf&&combat!=null&&combat.isAttacking(c),bl=bf&&combat!=null&&combat.isBlocking(c);return map("name",c.getName(),"owner",alias(c.getOwner()),"controller",bf||c.isInZone(ZoneType.Stack)?alias(c.getController()):null,"zone",c.getZone().getZoneType().name(),"tapped",bf?c.isTapped():null,"power",bf&&c.isCreature()?String.valueOf(c.getNetPower()):null,"toughness",bf&&c.isCreature()?String.valueOf(c.getNetToughness()):null,"damage",bf&&c.isCreature()?String.valueOf(c.getDamage()):null,"attacking",at,"blocking",bl,"defender",at?alias(combat.getDefenderPlayerByAttacker(c)):null,"block_status",!at?null:combat.isBlocked(c)?"Blocked":combat.isUnblocked(c)?"Unblocked":"NotDeclared","blockers",at?labels(combat.getBlockers(c)):List.of(),"attackers_blocked",bl?labels(combat.getAttackersBlockedBy(c)):List.of());}
        Map<String,Object> snapshot(){Map<String,Object> cs=new LinkedHashMap<>();for(var e:observed.entrySet())cs.put(e.getKey(),facts(e.getValue()));Combat c=game.getPhaseHandler().getCombat();Map<String,Object> out=map("cards",cs,"life",life(),"game_over",game.isGameOver(),"combat",map("attackers",c==null?List.of():labels(c.getAttackers()),"blockers",c==null?List.of():labels(c.getAllBlockers())));if(shield()&&responseCast&&card("response").isInZone(ZoneType.Graveyard)){out.put("live_shield_capacity",String.valueOf(game.getReplacementHandler().getTotalPreventionShieldAmount(card("a1"))));List<Map<String,Object>> live=new ArrayList<>();for(Card effect:game.getCardsIn(ZoneType.Command))if(effect.getEffectSource()!=null&&effect.getEffectSource().getId()==observed.get("response").getId())live.add(map("card_id",effect.getId(),"effect_source_id",effect.getEffectSource().getId(),"zone",effect.getZone().getZoneType().name(),"replacement_effect_count",effect.getReplacementEffects().size(),"stored_amount",effect.getSVar("ShieldAmount")));out.put("live_shield_effects",live);}return out;}
        void row(String suffix,Player holder){String id=scenario+"-"+suffix;Assert.assertTrue(recorded.add(id),id);Map<String,Object> r=control();r.putAll(snapshot());r.putAll(map("id",id,"priority",alias(holder),"protocol","actual-priority-callback","comparison_scope","common"));observe(rows,r);}
        List<SpellAbility> choose(Player p){Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertEquals(game.getPhaseHandler().getTurn(),1);Map<String,Object> q=control();q.putAll(map("ordinal",priorities.size(),"chooser",alias(p),"priority",alias(p)));observe(priorities,q);PhaseType phase=game.getPhaseHandler().getPhase();boolean empty=game.getStack().isEmpty();Player a=seats.get("A");
            if(p==a&&empty){String suffix=phase==PhaseType.COMBAT_BEGIN?"before":phase==PhaseType.COMBAT_DECLARE_ATTACKERS?"attacker-priority":phase==PhaseType.COMBAT_DECLARE_BLOCKERS?"blocker-priority":phase==PhaseType.COMBAT_DAMAGE?"damage-priority":phase==PhaseType.COMBAT_END?"endcombat":phase==PhaseType.MAIN2?"postcombat-main":null;if(suffix!=null&&!recorded.contains(scenario+"-"+suffix))row(suffix,p);if(responseCast&&response()&&card("response").isInZone(ZoneType.Graveyard)&&phase==(scenario.equals("D07")?PhaseType.COMBAT_DECLARE_ATTACKERS:PhaseType.COMBAT_DECLARE_BLOCKERS)&&!recorded.contains(scenario+"-after-response"))row("after-response",p);if(phase==PhaseType.MAIN2){done=true;return null;}}
            if(response()&&!responseCast&&empty&&phase==(scenario.equals("D07")?PhaseType.COMBAT_DECLARE_ATTACKERS:PhaseType.COMBAT_DECLARE_BLOCKERS)){Player actor=shield()||scenario.equals("D08")?a:seats.get("B");if(p==actor){selected=card("response").getFirstSpellAbility();selected.setActivatingPlayer(p);payer=p;spend=shield()?"W":scenario.equals("D08")?"G":"U";targetId=scenario.equals("D05")?"b1":"a1";p.getManaPool().addMana(new Mana(ManaAtom.fromName(spend),manaSource,null,p));observe(actions,map("kind","trusted-floating-seed","actor",alias(p),"color",spend,"amount","1","source",manaSource.getId()));Assert.assertTrue(selected.canPlay());return List.of(selected);}}return null;}
        void attackers(Player controller,Player attacker,Combat combat){Assert.assertSame(controller,seats.get("A"));Assert.assertSame(attacker,controller);Assert.assertEquals(game.getPhaseHandler().getPhase(),PhaseType.COMBAT_DECLARE_ATTACKERS);Map<String,Object> before=snapshot();List<Map<String,Object>> selected=new ArrayList<>();if(multi()){Assert.assertTrue(forge.game.combat.CombatUtil.canAttack(card("a2"),seats.get("C")));combat.addAttacker(card("a2"),seats.get("C"));selected.add(map("source","a2","defender","C"));}Assert.assertTrue(forge.game.combat.CombatUtil.canAttack(card("a1"),seats.get("B")));combat.addAttacker(card("a1"),seats.get("B"));selected.add(map("source","a1","defender","B"));observe(declarations,map("kind","attackers","actor",alias(controller),"selected",selected,"before",before,"after",snapshot(),"priority_callbacks",priorities.size()));}
        void blockers(Player controller,Player defender,Combat combat){Assert.assertSame(controller,defender);Assert.assertFalse(scenario.equals("D07"));Assert.assertNotEquals(alias(controller),"D");Map<String,Object> before=snapshot();List<Map<String,Object>> selected=new ArrayList<>();if(!List.of("D01","D09","D10","D11").contains(scenario)){block(combat,"a1","b1",selected);if(two())block(combat,"a1","b2",selected);}Assert.assertNull(forge.game.combat.CombatUtil.validateBlocks(combat,defender));observe(declarations,map("kind","blockers","actor",alias(controller),"selected",selected,"before",before,"after",snapshot(),"priority_callbacks",priorities.size()));}
        void block(Combat c,String a,String b,List<Map<String,Object>> out){Assert.assertTrue(forge.game.combat.CombatUtil.canBlock(card(a),card(b),c));c.addBlocker(card(a),card(b));out.add(map("source",b,"attacker",a));}
        Map<Card,Integer> assign(Player p,Card source,CardCollectionView recipients,CardCollectionView remaining,int amount,GameEntity defender,boolean override){Assert.assertEquals(game.getPhaseHandler().getPhase(),PhaseType.COMBAT_DAMAGE);Assert.assertTrue(override);Assert.assertSame(p,source.getController());Assert.assertTrue(assigned.add(source.getId()),"repeated source assignment");Assert.assertTrue(amount>0);String src=label(source);Map<Card,Integer> out=new LinkedHashMap<>();if(src.equals("a1")){Assert.assertEquals(amount,scenario.equals("D08")?9:6);Assert.assertEquals(labels(recipients),two()?List.of("b1","b2"):List.of("b1"));if(scenario.equals("D03")){out.put(card("b1"),1);out.put(card("b2"),5);}else if(scenario.equals("D04"))out.put(card("b1"),6);else if(two()){out.put(card("b1"),3);out.put(card("b2"),3);}else out.put(card("b1"),amount);}else{Assert.assertTrue(src.equals("b1")||src.equals("b2"));Assert.assertEquals(labels(recipients),List.of("a1"));Assert.assertEquals(amount,List.of("D02","D08").contains(scenario)?6:3);out.put(card("a1"),amount);}List<Map<String,Object>> rows=new ArrayList<>();int total=0;for(var e:out.entrySet()){Assert.assertTrue(recipients.contains(e.getKey()));Assert.assertTrue(e.getValue()>0);total+=e.getValue();rows.add(map("source",src,"target",label(e.getKey()),"amount",String.valueOf(e.getValue()),"kind","Combat"));}Assert.assertEquals(total,amount);observe(assignments,map("kind","assignment-callback","actor",alias(p),"source",src,"power",String.valueOf(amount),"recipient_domain",labels(recipients),"input_recipient_order",java.util.stream.StreamSupport.stream(recipients.spliterator(),false).map(this::label).toList(),"override_order",override,"selected",rows,"priority_callbacks",priorities.size(),"world",snapshot()));return out;}
        boolean play(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertTrue(sa.canPlay());Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,sa));responseCast=true;observe(actions,map("kind","actual-paid-cast","actor",alias(p),"card","response","target",targetId,"spend",spend,"stack_size",game.getStack().size(),"target_count",game.getStack().peekAbility().getAllTargetChoices().stream().mapToInt(t->t.size()).sum(),"target_cards",game.getStack().peekAbility().getAllTargetChoices().stream().flatMap(t->t.getTargetCards().stream()).map(this::label).toList(),"paying_mana",sa.getPayingMana().stream().map(m->m.toString()).toList(),"remaining_mana",String.valueOf(p.getManaPool().totalMana())));row("response-cast",p);return true;}
        SpellAbility select(Player p,Card host,List<SpellAbility> options){Assert.assertSame(p,payer);Assert.assertEquals(options.size(),1);Assert.assertSame(options.get(0),selected);return selected;}
        boolean target(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertEquals(sa.getHostCard().getId(),card("response").getId());Assert.assertTrue(sa.canTarget(card(targetId)));sa.getTargets().add(card(targetId));Assert.assertTrue(sa.isTargetNumberValid());observe(choices,map("kind","target","actor",alias(p),"target",targetId,"api",String.valueOf(sa.getApi())));return true;}
        List<AbilitySub> mode(Player p,SpellAbility sa,List<AbilitySub> possible,int min,int num,boolean repeat){Assert.assertTrue(shield());Assert.assertSame(p,seats.get("A"));Assert.assertEquals(possible.size(),2);AbilitySub chosen=null;for(AbilitySub a:possible)if(a.getApi()==ApiType.PreventDamage)chosen=a;Assert.assertNotNull(chosen);observe(choices,map("kind","mode","actor",alias(p),"possible",possible.stream().map(a->String.valueOf(a.getApi())).toList(),"chosen",String.valueOf(chosen.getApi()),"min",min,"num",num,"allow_repeat",repeat));return new ArrayList<>(List.of(chosen));}
        <T extends GameEntity> T source(Player p,FCollectionView<T> options,SpellAbility sa,boolean optional){Assert.assertTrue(scenario.equals("D13")||scenario.equals("D14"));Assert.assertSame(p,seats.get("A"));Assert.assertFalse(optional);String wanted=scenario.equals("D13")?"b1":"b2";T chosen=null;List<String> domain=new ArrayList<>();for(T e:options){Assert.assertTrue(e instanceof Card);String name=label((Card)e);domain.add(name);if(name.equals(wanted))chosen=e;}Assert.assertNotNull(chosen);java.util.Collections.sort(domain);Assert.assertEquals(domain,List.of("b1","b2"));observe(choices,map("kind","prevention-source","actor",alias(p),"domain",domain,"chosen",wanted,"marked_before",String.valueOf(card("a1").getDamage())));return chosen;}
        void replacement(Player p,SpellAbility sa,boolean may){Assert.assertTrue(shield());Assert.assertSame(p,seats.get("A"));Assert.assertTrue(sa.isReplacementAbility());Assert.assertEquals(sa.getApi(),ApiType.ReplaceDamage);Assert.assertNull(sa.getSubAbility());Card source=(Card)sa.getReplacingObject(AbilityKey.Source),target=(Card)sa.getReplacingObject(AbilityKey.Target);Assert.assertEquals(label(target),"a1");Assert.assertTrue(label(source).equals("b1")||label(source).equals("b2"));@SuppressWarnings("unchecked") Map<AbilityKey,Object> params=(Map<AbilityKey,Object>)sa.getReplacingObject(AbilityKey.OriginalParams);Map<String,Object> r=map("kind","prevention-application","actor",alias(p),"source",label(source),"target","a1","damage_before",String.valueOf(sa.getReplacingObject(AbilityKey.DamageAmount)),"shield_svar_before",sa.getHostCard().getSVar("ShieldAmount"),"marked_before",String.valueOf(card("a1").getDamage()));AbilityUtils.resolve(sa);r.put("damage_after",String.valueOf(params.get(AbilityKey.DamageAmount)));r.put("result",String.valueOf(params.get(AbilityKey.ReplacementResult)));r.put("retained_host_svar_after",sa.getHostCard().getSVar("ShieldAmount"));observe(choices,r);}
        List<CostPart> order(Player p,List<CostPart> costs){Assert.assertSame(p,payer);for(CostPart c:costs)Assert.assertTrue(c instanceof CostPartMana);return costs;}
        boolean pay(Player p,ManaCost cost,CostPartMana part,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);observe(actions,map("kind","mana-payment","actor",alias(p),"cost",cost.toString()));return PlaySpellAbility.payManaCost(p.getController(),cost,part,sa,p,prompt,matrix,false);}
        boolean apply(Player p,ManaCostBeingPaid cost,SpellAbility sa,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);p.pushPaidForSA(sa);sa.setManaCostBeingPaid(cost);try{Assert.assertTrue(p.getManaPool().tryPayCostWithColor(ManaAtom.fromName(spend),sa,cost,sa.getPayingMana()));Assert.assertTrue(cost.isPaid());return true;}finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}}
        @Subscribe public void event(GameEvent event){if(eventFailure!=null)return;try{Map<String,Object> e=null;if(event instanceof GameEventPlayerPriority v)e=map("kind","priority-event","priority",playerId(v.priority().getId()),"step",v.phase().name());if(event instanceof GameEventTurnPhase v)e=map("kind","phase-event","active",playerId(v.playerTurn().getId()),"step",v.phase().name());if(event instanceof GameEventAttackersDeclared v){everAttacked|=!v.attackersMap().isEmpty();e=map("kind","attackers-declared-event","actor",playerId(v.player().getId()),"count",v.attackersMap().size());}if(event instanceof GameEventBlockersDeclared v)e=map("kind","blockers-declared-event","actor",playerId(v.defendingPlayer().getId()));if(event instanceof GameEventCardTapped v)e=map("kind","tap-event","card",cardId(v.card().getId()),"tapped",v.tapped());if(event instanceof GameEventCardDamaged v)e=map("kind","damage-card","source",cardId(v.source().getId()),"target",cardId(v.card().getId()),"amount",String.valueOf(v.amount()),"type",v.type().name());if(event instanceof GameEventPlayerDamaged v)e=map("kind","damage-player","source",cardId(v.source().getId()),"target",playerId(v.target().getId()),"amount",String.valueOf(v.amount()),"combat",v.combat(),"infect",v.infect());if(event instanceof GameEventSpellResolved v)e=map("kind","resolved","fizzled",v.hasFizzled());if(event instanceof GameEventCardChangeZone v&&observed.values().stream().anyMatch(c->c.getId()==v.card().getId()))e=map("kind","zone","card",cardId(v.card().getId()),"from",v.from()==null?null:v.from().zoneType().name(),"to",v.to()==null?null:v.to().zoneType().name());if(event instanceof GameEventCombatEnded)e=map("kind","combat-ended");if(event instanceof GameEventGameOutcome v)e=map("kind","outcome-event","winner_name",v.winningPlayerName(),"outcome_strings",v.outcomeStrings());if(e!=null)observe(events,e);}catch(Throwable t){eventFailure=t;}}
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
        @Override public PaymentDecision visit(CostSacrifice cost) {throw new AssertionError("Unexpected cost: CostSacrifice");}
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
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { script.replacement(getPlayer(), effectSA, mayChoseNewTargets); }
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
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { return script.source(getPlayer(),optionList,sa,isOptional); }
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
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destinationZone, SpellAbility source) { if(cards.size()<=1)return cards; Assert.assertTrue(script.scenario.equals("D13")||script.scenario.equals("D14")); Assert.assertSame(getPlayer(),script.seats.get("B")); Assert.assertEquals(destinationZone,ZoneType.Graveyard); Assert.assertNull(source); Assert.assertEquals(script.labels(cards),List.of("b1","b2")); CardCollection ordered=new CardCollection();ordered.add(script.card("b1"));ordered.add(script.card("b2"));script.observe(script.choices,map("kind","reference-only-graveyard-order","actor","B","domain",script.labels(cards),"chosen",List.of("b1","b2"),"destination","Graveyard"));return ordered; }
        @Override public CardCollectionView chooseCardsToDiscardFrom(Player playerDiscard, SpellAbility sa, CardCollection validCards, int min, int max, CardCollectionView visibleToChooser) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardFrom"); }
        @Override public CardCollectionView chooseCardsToDiscardUnlessType(int min, CardCollectionView hand, String[] unlessTypes, SpellAbility sa) { throw new AssertionError("Unexpected controller operation: chooseCardsToDiscardUnlessType"); }
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { throw new AssertionError("Unexpected discard"); }
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
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { return script.mode(getPlayer(),sa,possible,min,num,allowRepeat); }
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
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { Assert.assertTrue(script.shield()); Assert.assertSame(getPlayer(),script.seats.get("A")); Assert.assertEquals(possibleReplacers.size(),1); script.observe(script.choices,map("kind","prevention-effect","actor","A","domain_size",possibleReplacers.size(),"marked_before",String.valueOf(script.card("a1").getDamage()))); return possibleReplacers.get(0); }
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
