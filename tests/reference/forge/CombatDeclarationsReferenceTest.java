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
import java.io.IOException;
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



/** Strict real-loop declaration probes; normative expectations are external and frozen. */
public class CombatDeclarationsReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256="47a5b95c75a1c6ae7732cc55e6cec4e88fe34381ef1eee58031e2d1dd5433178";
    @Test public void noEligibleAttackersForcedEmpty() throws IOException {run("C01");}
    @Test public void eligibleCreatureChoosesNoAttack() throws IOException {run("C02");}
    @Test public void matureAttacksFreshWaitsAndCanDeclineBlock() throws IOException {run("C03");}
    @Test public void twoFreshBlockersOneAttacker() throws IOException {run("C04");}
    @Test public void blockedAttackerAfterSoleBlockerBounced() throws IOException {run("C05");}
    @Test public void blockerRemainsWhenItsAttackerBounced() throws IOException {run("C06");}
    @Test public void allAttackersRemovedBeforeBlockersHistory() throws IOException {run("C07");}
    @Test public void fourSeatApnapDefenderIsolation() throws IOException {run("C08");}
    @Test public void attackedSeatWithNoBlockAssignment() throws IOException {run("C09");}
    private static Map<String,Object> map(Object... pairs){final Map<String,Object> out=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)out.put((String)pairs[i],pairs[i+1]);return out;}
    private Game fresh(String id){
        final List<RegisteredPlayer> players=new ArrayList<>();final String[] order=id.equals("C08")?new String[]{"A","D","C","B"}:new String[]{"A","B"};
        final int[] ids=id.equals("C08")?new int[]{41,7,99,3}:new int[]{41,3};
        for(int i=0;i<order.length;i++){final RegisteredPlayer p=new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-"+order[i],null));p.setId(ids[i]);p.setStartingLife(20);p.setTeamNumber(i+1);players.add(p);}
        final GameRules rules=new GameRules(GameType.Constructed);rules.setOrderCombatants(false);final Game game=new Game(players,rules,new Match(rules,players,"Combat declaration reference"));game.setAge(GameStage.Play);return game;
    }
    private void run(String id)throws IOException{
        final Script s=new Script(fresh(id),id);boolean completed=false;
        try{s.seed();s.game.subscribeToEvents(s);s.game.getPhaseHandler().setupFirstTurn(s.seats.get("A"),null);
            for(int i=0;i<160;i++){Assert.assertFalse(s.game.isGameOver());final int before=s.priorities.size();s.game.getPhaseHandler().mainLoopStep();s.loops.add(map("ordinal",i,"priority_callbacks",s.priorities.size()-before,"after",s.control()));if(s.eventFailure!=null)throw new AssertionError("Observer failure",s.eventFailure);if(s.done)break;Assert.assertTrue(i<159,"bounded loop exhausted");}
            Assert.assertTrue(s.done);completed=true;
        }finally{
            final Map<String,Object> trace=map("schema_version",1,"profile","combat-declarations","scenario",id,"reference","forge","commit",COMMIT,"expected_sha256",EXPECTED_SHA256,"completed",completed,"strict_choices",true,"ai_policy_used",false,"priority_loop_executed",true,"dev_phase_advance_used",false,"direct_resolution_used",false,"registered_life","20","policy","UnrestrictedFreeForAll","order_combatants",s.game.getRules().hasOrderCombatants(),"configuration",s.configuration(),"setup","Exact ordinary PaperCard seeds before actual first Untap; explicit life, teams and seat order. Disclosed floating payment support only; no phase/priority/combat/resolution injection.","rows",s.rows,"declarations",s.declarations,"priorities",s.priorities,"loops",s.loops,"actions",s.actions,"events",s.events,"metadata",s.metadata,"ever_declared_attackers",s.everAttacked,"limits",List.of("Declaration and response facts only; damage unfinished natively.","C06 tail reference-only; C07 skip source hypothesis adjudicated externally.","Stable Forge IDs are not native incarnations; paid floating support is disclosed."));
            final String dir=System.getProperty("spellwake.evidence.dir");Assert.assertNotNull(dir);Files.write(Paths.get(dir,id+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH "+id+" completed="+completed+" rows="+s.rows.size()+" callbacks="+s.priorities.size());
        }
    }
    public static final class Script {
        final Game game;final String scenario;final Map<String,Player> seats=new LinkedHashMap<>();final Map<String,Card> observed=new LinkedHashMap<>();final Map<String,Object> metadata=new LinkedHashMap<>();
        final List<Map<String,Object>> rows=new ArrayList<>(),declarations=new ArrayList<>(),priorities=new ArrayList<>(),loops=new ArrayList<>(),actions=new ArrayList<>(),events=new ArrayList<>();final java.util.Set<String> recorded=new java.util.HashSet<>();
        SpellAbility selected;Player payer;String spend,selectedId,targetId;Card manaSource;boolean cast,responseCast,done,everAttacked;Throwable eventFailure;
        Script(Game g,String id){game=g;scenario=id;for(Player p:g.getPlayers()){String a=p.getName().substring("fixture-".length());seats.put(a,p);p.addController(game.getNextTimestamp(),p,new StrictController(game,p,this),false);Assert.assertFalse(p.getController().isAI());}Assert.assertFalse(game.getRules().hasOrderCombatants());}
        String alias(Player p){if(p==null)return null;for(Map.Entry<String,Player> e:seats.entrySet())if(e.getValue()==p)return e.getKey();throw new AssertionError("Unknown player");}
        String playerId(int id){for(Map.Entry<String,Player> e:seats.entrySet())if(e.getValue().getId()==id)return e.getKey();throw new AssertionError("Unknown player ID");}
        Card current(Card c){return game.getCardState(c);}Card card(String id){return current(observed.get(id));}
        String cardId(int id){for(Map.Entry<String,Card> e:observed.entrySet())if(e.getValue().getId()==id)return e.getKey();return "support-"+id;}
        String label(Card c){return cardId(c.getId());}
        List<String> labels(Iterable<Card> cs){final List<String> out=new ArrayList<>();for(Card c:cs)out.add(label(c));java.util.Collections.sort(out);return out;}
        Map<String,Object> configuration(){final List<String> order=new ArrayList<>();final Map<String,Object> players=new LinkedHashMap<>();for(Player p:game.getPlayersInTurnOrder()){order.add(alias(p));final List<String> opponents=new ArrayList<>();for(Player other:p.getOpponents())opponents.add(alias(other));java.util.Collections.sort(opponents);players.put(alias(p),map("id",p.getId(),"team",p.getTeam(),"opponents",opponents));}return map("turn_order",order,"players",players);}
        Card seedCard(String id,String name,Player owner,ZoneType zone,boolean primary){
            final String number=name.equals("Craw Wurm")?"239":name.equals("Unsummon")?"112":name.equals("Rod of Ruin")?"312":"335";final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name,"8ED",number);Assert.assertNotNull(paper);Assert.assertEquals(paper.getEdition(),"8ED");Assert.assertEquals(paper.getCollectorNumber(),number);Assert.assertEquals(paper.getName(),name);
            final Card c=Card.fromPaperCard(paper,owner);c.setGameTimestamp(game.getNextTimestamp());owner.getZone(zone).add(c);Assert.assertFalse(c.isStartsGameInPlay());metadata.put(id,map("name",name,"owner",alias(owner),"zone",zone.name(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText(),"card_id",c.getId(),"starts_game_in_play",c.isStartsGameInPlay(),"primary",primary));if(primary)observed.put(id,c);return c;
        }
        void seed(){
            Player a=seats.get("A"),b=seats.get("B");
            if(scenario.equals("C01"))seedCard("rod","Rod of Ruin",a,ZoneType.Battlefield,true);else seedCard("a1","Craw Wurm",a,ZoneType.Battlefield,true);
            if(scenario.equals("C03")){seedCard("freshA","Craw Wurm",a,ZoneType.Hand,true);seedCard("rod","Rod of Ruin",a,ZoneType.Battlefield,true);}
            if(List.of("C03","C04","C05","C06","C07","C08").contains(scenario))seedCard("b1","Craw Wurm",b,ZoneType.Battlefield,true);
            if(scenario.equals("C04"))seedCard("b2","Craw Wurm",b,ZoneType.Battlefield,true);
            if(scenario.equals("C08")){seedCard("a2","Craw Wurm",a,ZoneType.Battlefield,true);seedCard("c1","Craw Wurm",seats.get("C"),ZoneType.Battlefield,true);seedCard("d1","Craw Wurm",seats.get("D"),ZoneType.Battlefield,true);}
            if(scenario.equals("C09"))seedCard("rod","Rod of Ruin",b,ZoneType.Battlefield,true);
            if(List.of("C05","C06","C07").contains(scenario))seedCard("bounce","Unsummon",b,ZoneType.Hand,true);
            manaSource=seedCard("mana-source","Island",a,ZoneType.Battlefield,false);for(Map.Entry<String,Player> e:seats.entrySet())for(int i=0;i<8;i++)seedCard(e.getKey()+"-library-"+i,"Island",e.getValue(),ZoneType.Library,false);
        }
        Map<String,Object> control(){return map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase()==null?null:game.getPhaseHandler().getPhase().name(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"raw_priority",alias(game.getPhaseHandler().getPriorityPlayer()),"stack_size",game.getStack().size());}
        Map<String,Object> facts(Card input){final Card c=current(input);final boolean bf=c.isInPlay();final Combat combat=game.getPhaseHandler().getCombat();final boolean attacking=bf&&combat!=null&&combat.isAttacking(c),blocking=bf&&combat!=null&&combat.isBlocking(c);final String status=!attacking?null:combat.isBlocked(c)?"Blocked":combat.isUnblocked(c)?"Unblocked":"NotDeclared";
            return map("owner",alias(c.getOwner()),"controller",bf||c.isInZone(ZoneType.Stack)?alias(c.getController()):null,"zone",c.getZone().getZoneType().name(),"tapped",bf?c.isTapped():null,"continuous",bf?!c.isFirstTurnControlled():null,"attacking",attacking,"blocking",blocking,"defender",attacking?alias(combat.getDefenderPlayerByAttacker(c)):null,"block_status",status,"attackers_blocked",blocking?labels(combat.getAttackersBlockedBy(c)):List.of(),"blockers",attacking?labels(combat.getBlockers(c)):List.of());
        }
        Map<String,Object> snapshot(){final Map<String,Object> cs=new LinkedHashMap<>();for(Map.Entry<String,Card> e:observed.entrySet())cs.put(e.getKey(),facts(e.getValue()));final Combat c=game.getPhaseHandler().getCombat();return map("cards",cs,"combat",map("attackers",c==null?List.of():labels(c.getAttackers()),"blockers",c==null?List.of():labels(c.getAllBlockers())));}
        void record(String id,Player holder,String protocol,String scope){Assert.assertTrue(recorded.add(id),"Duplicate "+id);final Map<String,Object> v=control();v.putAll(snapshot());v.putAll(map("id",id,"priority",alias(holder),"protocol",protocol,"comparison_scope",scope,"callback_count",priorities.size(),"event_count",events.size()));rows.add(v);}
        void row(String suffix,Player holder){record(scenario+"-"+suffix,holder,holder==null?"declaration-callback":"actual-priority-callback","common");}
        List<SpellAbility> chooseSpell(Player p,String id,String colors,String target){selected=card(id).getFirstSpellAbility();selected.setActivatingPlayer(p);payer=p;selectedId=id;spend=colors;targetId=target;for(char c:spend.toCharArray())p.getManaPool().addMana(new Mana(ManaAtom.fromName(String.valueOf(c)),manaSource,null,p));actions.add(map("kind","trusted-floating-seed","actor",alias(p),"colors",spend,"priority_ordinal",priorities.size()-1));Assert.assertTrue(selected.canPlay());return java.util.Collections.singletonList(selected);}
        List<SpellAbility> choose(Player p){Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertFalse(p.getController().isAI());final Map<String,Object> point=control();point.putAll(map("ordinal",priorities.size(),"chooser",alias(p),"priority",alias(p)));priorities.add(point);Assert.assertEquals(game.getPhaseHandler().getTurn(),1);
            final PhaseType phase=game.getPhaseHandler().getPhase();final Player a=seats.get("A"),b=seats.get("B");
            if(scenario.equals("C03")&&phase==PhaseType.MAIN1&&p==a){if(!cast)return chooseSpell(a,"freshA","GGCCCC",null);if(game.getStack().isEmpty()&&card("freshA").isInPlay()&&!recorded.contains("C03-cast-resolved"))row("cast-resolved",p);}
            if((scenario.equals("C01")||scenario.equals("C02"))&&p==a){if(phase==PhaseType.COMBAT_BEGIN&&!recorded.contains(scenario+"-before"))row("before",p);if(phase==PhaseType.COMBAT_END&&!recorded.contains(scenario+"-endcombat-priority"))row("endcombat-priority",p);if(phase==PhaseType.MAIN2&&!recorded.contains(scenario+"-postcombat-main")){row("postcombat-main",p);done=true;}}
            if(phase==PhaseType.COMBAT_DECLARE_ATTACKERS&&p==a&&game.getStack().isEmpty()){
                if(!List.of("C05","C06").contains(scenario)&&!recorded.contains(scenario+"-attacker-priority"))row("attacker-priority",p);
                if(scenario.equals("C07")&&responseCast&&!recorded.contains("C07-after-bounce"))row("after-bounce",p);
            }
            if(scenario.equals("C07")&&phase==PhaseType.COMBAT_DECLARE_ATTACKERS&&p==b&&!responseCast)return chooseSpell(b,"bounce","U","a1");
            if(phase==PhaseType.COMBAT_DECLARE_BLOCKERS&&p==a&&game.getStack().isEmpty()){
                if(List.of("C03","C04","C08","C09").contains(scenario)&&!recorded.contains(scenario+"-blocker-priority")){row("blocker-priority",p);done=true;}
                if(scenario.equals("C05")||scenario.equals("C06")){if(!responseCast&&!recorded.contains(scenario+"-blocks-settled"))row("blocks-settled",p);if(responseCast&&!recorded.contains(scenario+"-after-bounce")){row("after-bounce",p);if(scenario.equals("C05"))done=true;}}
            }
            if((scenario.equals("C05")||scenario.equals("C06"))&&phase==PhaseType.COMBAT_DECLARE_BLOCKERS&&p==b&&!responseCast)return chooseSpell(b,"bounce","U",scenario.equals("C05")?"b1":"a1");
            if(scenario.equals("C06")&&p==a){if(phase==PhaseType.COMBAT_END&&!recorded.contains("C06-endcombat"))record("C06-endcombat",p,"actual-priority-callback","reference-only-tail");if(phase==PhaseType.MAIN2&&!recorded.contains("C06-postcombat-main")){record("C06-postcombat-main",p,"actual-priority-callback","reference-only-tail");done=true;}}
            if(scenario.equals("C07")&&p==a&&phase==PhaseType.MAIN2)done=true;
            return null;
        }
        void attackers(Player controller,Player attacker,Combat combat){Assert.assertSame(controller,seats.get("A"));Assert.assertSame(attacker,controller);Assert.assertEquals(game.getPhaseHandler().getPhase(),PhaseType.COMBAT_DECLARE_ATTACKERS);Assert.assertFalse(scenario.equals("C01"));final Map<String,Object> before=snapshot();final List<String> eligible=labels(forge.game.combat.CombatUtil.getPossibleAttackers(attacker));final List<Map<String,Object>> chosen=new ArrayList<>();
            if(scenario.equals("C03")||scenario.equals("C04"))row("before-attacks",null);
            if(!scenario.equals("C02")){if(scenario.equals("C08")){Assert.assertTrue(forge.game.combat.CombatUtil.canAttack(card("a2"),seats.get("C")));combat.addAttacker(card("a2"),seats.get("C"));chosen.add(map("attacker","a2","defender","C"));}Assert.assertTrue(forge.game.combat.CombatUtil.canAttack(card("a1"),seats.get("B")));combat.addAttacker(card("a1"),seats.get("B"));chosen.add(map("attacker","a1","defender","B"));}
            if(scenario.equals("C03")){Assert.assertEquals(eligible,List.of("a1"));Assert.assertFalse(forge.game.combat.CombatUtil.canAttack(card("freshA"),seats.get("B")));}
            declarations.add(map("ordinal",declarations.size(),"kind","attackers","actor",alias(controller),"eligible",eligible,"selected",chosen,"before",before,"after",snapshot(),"priority_callbacks",priorities.size()));
        }
        void blockers(Player controller,Player defender,Combat combat){Assert.assertSame(controller,defender);Assert.assertEquals(game.getPhaseHandler().getPhase(),PhaseType.COMBAT_DECLARE_BLOCKERS);Assert.assertFalse(List.of("C01","C02","C07","C09").contains(scenario));final Map<String,Object> before=snapshot();final List<Map<String,Object>> chosen=new ArrayList<>();final Map<String,Object> probes=new LinkedHashMap<>();
            if(scenario.equals("C04"))row("before-blocks",null);
            if(scenario.equals("C08")){String own=controller==seats.get("C")?"c1":"b1",at=controller==seats.get("C")?"a2":"a1",otherAt=at.equals("a1")?"a2":"a1";boolean cross=forge.game.combat.CombatUtil.canBlock(card(otherAt),card(own),combat);Assert.assertFalse(cross);probes.put("cross_allowed",cross);if(controller==seats.get("C"))Assert.assertEquals(labels(combat.getAllBlockers()),List.of());else {Assert.assertSame(controller,seats.get("B"));Assert.assertEquals(labels(combat.getAllBlockers()),List.of("c1"));}addBlock(combat,at,own,chosen);}
            else {Assert.assertSame(controller,seats.get("B"));if(!scenario.equals("C03")){addBlock(combat,"a1","b1",chosen);if(scenario.equals("C04"))addBlock(combat,"a1","b2",chosen);}}
            Assert.assertNull(forge.game.combat.CombatUtil.validateBlocks(combat,defender));declarations.add(map("ordinal",declarations.size(),"kind","blockers","actor",alias(controller),"selected",chosen,"before",before,"after",snapshot(),"probes",probes,"priority_callbacks",priorities.size()));
        }
        void addBlock(Combat c,String attacker,String blocker,List<Map<String,Object>> selected){Assert.assertTrue(forge.game.combat.CombatUtil.canBlock(card(attacker),card(blocker),c));c.addBlocker(card(attacker),card(blocker));selected.add(map("blocker",blocker,"attacker",attacker));}
        boolean play(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertTrue(sa.canPlay());Assert.assertTrue(card(selectedId).isInZone(ZoneType.Hand));int before=game.getStack().size();Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,sa));Assert.assertEquals(game.getStack().size(),before+1);Assert.assertTrue(card(selectedId).isInZone(ZoneType.Stack));actions.add(map("kind","actual-paid-cast","actor",alias(p),"card",selectedId,"spend",spend,"target",targetId,"before_stack",before,"after_stack",game.getStack().size(),"target_count",game.getStack().peekAbility().getTargets().size(),"priority_ordinal",priorities.size()-1));if(selectedId.equals("freshA"))cast=true;else {responseCast=true;row("response-cast",p);}return true;}
        SpellAbility select(Player p,Card host,List<SpellAbility> options){Assert.assertSame(p,payer);Assert.assertEquals(options.size(),1);Assert.assertSame(options.get(0),selected);return selected;}
        boolean target(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertNotNull(targetId);Assert.assertTrue(sa.canTarget(card(targetId)));sa.getTargets().add(card(targetId));Assert.assertTrue(sa.isTargetNumberValid());return true;}
        List<CostPart> order(Player p,List<CostPart> costs){Assert.assertSame(p,payer);for(CostPart c:costs)Assert.assertTrue(c instanceof CostPartMana);return costs;}
        boolean pay(Player p,ManaCost toPay,CostPartMana cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);actions.add(map("kind","mana-payment","actor",alias(p),"cost",toPay.toString()));return PlaySpellAbility.payManaCost(p.getController(),toPay,cost,sa,p,prompt,matrix,false);}
        boolean apply(Player p,ManaCostBeingPaid toPay,SpellAbility sa,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);p.pushPaidForSA(sa);sa.setManaCostBeingPaid(toPay);try{for(char c:spend.toCharArray())Assert.assertTrue(p.getManaPool().tryPayCostWithColor(ManaAtom.fromName(String.valueOf(c)),sa,toPay,sa.getPayingMana()));Assert.assertTrue(toPay.isPaid());return true;}finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}}
        @Subscribe public void onEvent(GameEvent event){if(eventFailure!=null)return;try{Map<String,Object> e=null;
            if(event instanceof GameEventPlayerPriority v)e=map("kind","priority-event","priority",playerId(v.priority().getId()),"step",v.phase().name());
            if(event instanceof GameEventTurnPhase v)e=map("kind","phase-event","active",playerId(v.playerTurn().getId()),"step",v.phase().name(),"description",v.phaseDesc());
            if(event instanceof GameEventAttackersDeclared v){final List<Map<String,Object>> selected=new ArrayList<>();for(Map.Entry<GameEntityView,CardView> link:v.attackersMap().entries())selected.add(map("attacker",cardId(link.getValue().getId()),"defender",playerId(link.getKey().getId())));selected.sort(java.util.Comparator.comparing(x->(String)x.get("attacker")));everAttacked|=!selected.isEmpty();e=map("kind","attackers-declared-event","actor",playerId(v.player().getId()),"selected",selected);}
            if(event instanceof GameEventBlockersDeclared v){final List<Map<String,Object>> links=new ArrayList<>();for(Map.Entry<GameEntityView,Multimap<CardView,CardView>> outer:v.blockers().entrySet())for(Map.Entry<CardView,CardView> link:outer.getValue().entries())links.add(map("defender",playerId(outer.getKey().getId()),"attacker",cardId(link.getKey().getId()),"raw_blocker",cardId(link.getValue().getId()),"unblocked_self_sentinel",link.getKey().getId()==link.getValue().getId()));links.sort(java.util.Comparator.comparing(x->x.toString()));e=map("kind","blockers-declared-event","actor",playerId(v.defendingPlayer().getId()),"raw_links",links);}
            if(event instanceof GameEventCardTapped v)e=map("kind","tap-event","card",cardId(v.card().getId()),"tapped",v.tapped());
            if(event instanceof GameEventSpellResolved v)e=map("kind","resolved","fizzled",v.hasFizzled(),"step",game.getPhaseHandler().getPhase().name());
            if(event instanceof GameEventCardChangeZone v&&observed.values().stream().anyMatch(c->c.getId()==v.card().getId()))e=map("kind","zone","card",cardId(v.card().getId()),"from",v.from()==null?null:v.from().zoneType().name(),"to",v.to()==null?null:v.to().zoneType().name());
            if(event instanceof GameEventCombatChanged)e=map("kind","combat-changed");if(event instanceof GameEventCombatEnded)e=map("kind","combat-ended");
            if(e!=null){e.put("ordinal",events.size());e.put("priority_callbacks_before",priorities.size());events.add(e);}
        }catch(Throwable t){eventFailure=t;}}
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
        @Override public void playSpellAbilityNoStack(SpellAbility effectSA, boolean mayChoseNewTargets) { throw new AssertionError("Unexpected nested resolution"); }
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.size()<=1); return activePlayerSAs; }
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> activePlayerSAs) { Assert.assertTrue(activePlayerSAs.isEmpty(), "Unexpected trigger"); }
        @Override public boolean playTrigger(Card host, WrappedAbility wrapperAbility, boolean isMandatory) { throw new AssertionError("Unexpected controller operation: playTrigger"); }
        @Override public boolean playSaFromPlayEffect(SpellAbility tgtSA) { throw new AssertionError("Unexpected controller operation: playSaFromPlayEffect"); }
        @Override public List<PaperCard> sideboard(final Deck deck, GameType gameType, String message) { throw new AssertionError("Unexpected controller operation: sideboard"); }
        @Override public List<PaperCard> chooseCardsYouWonToAddToDeck(List<PaperCard> losses) { throw new AssertionError("Unexpected controller operation: chooseCardsYouWonToAddToDeck"); }
        @Override public Map<Card, Integer> assignCombatDamage(Card attacker, CardCollectionView blockers, CardCollectionView remaining, int damageDealt, GameEntity defender, boolean overrideOrder) { throw new AssertionError("Unexpected controller operation: assignCombatDamage"); }
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
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { throw new AssertionError("Unexpected controller operation: chooseSingleEntityForEffect"); }
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
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destinationZone, SpellAbility source) { if (cards.size() > 1) throw new AssertionError("Unexpected zone ordering choice"); return cards; }
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
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { throw new AssertionError("Unexpected controller operation: chooseModeForAbility"); }
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
        @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> possibleReplacers) { throw new AssertionError("Unexpected controller operation: chooseSingleReplacementEffect"); }
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
