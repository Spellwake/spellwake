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



/** Actual paid ordinary permanent spells and post-resolution checks via strict priority loop. */
public class PermanentCastingReferenceTest extends AITest {
    private static final String COMMIT="3ead7e68d54bf95c6f778fd6aa7c604c036e0736";
    private static final String EXPECTED_SHA256="0a71b29eecccc14e736552f3a1ad009287120bc513d20885df594c8434f2b698";
    @Test public void ordinaryCreatureResolves() throws IOException { run("creature-resolve"); }
    @Test public void artifactResolvesThenActivates() throws IOException { run("artifact-ability"); }
    @Test public void permanentCounterResponse() throws IOException { run("creature-counter"); }
    @Test public void zeroToughnessBeforePriority() throws IOException { run("zero-toughness"); }
    private Game freshGame() {
        final List<RegisteredPlayer> players=new ArrayList<>();
        players.add(new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-B",null)));
        players.add(new RegisteredPlayer(new Deck()).setPlayer(new LobbyPlayerAi("fixture-A",null)));
        final GameRules rules=new GameRules(GameType.Constructed);
        final Game game=new Game(players,rules,new Match(rules,players,"Permanent casting reference"));
        game.setAge(GameStage.Play);return game;
    }
    private void run(String scenario) throws IOException {
        final Script script=new Script(freshGame(),scenario);boolean completed=false;
        try {
            script.seed();script.game.subscribeToEvents(script);
            script.game.getPhaseHandler().setupFirstTurn(script.actor,null);
            for(int i=0;i<60;i++) {
                Assert.assertFalse(script.game.isGameOver());
                final int before=script.priorities.size();
                script.game.getPhaseHandler().mainLoopStep();
                script.loops.add(map("ordinal",i,"priority_callbacks",script.priorities.size()-before,"after",script.control()));
                if(script.eventFailure!=null)throw new AssertionError("Observer failure",script.eventFailure);
                if(script.done)break;
                Assert.assertTrue(i<59,"bounded loop exhausted");
            }
            script.validate();completed=true;
        } finally {
            final Map<String,Object> trace=map("schema_version",1,"scenario",scenario,"reference","forge","commit",COMMIT,"expected_sha256",EXPECTED_SHA256,
                "completed",completed,"strict_choices",true,"ai_policy_used",false,"priority_loop_executed",true,"dev_phase_advance_used",false,"direct_resolution_used",false,
                "setup","Fresh Game; both lobby controllers replaced by non-AI strict controllers before setupFirstTurn(A,null). Explicit printing Card.fromPaperCard seeds, ordinary libraries and disclosed floating mana; no seeded phase/priority/stack. Force trample excluded.",
                "api_boundary","PlaySpellAbility/CostPayment admit real paid spells and tap/mana activation. Real PhaseHandler passes invoke ordinary resolution and SBA; no direct resolver call. Forge stable card identity is not native incarnation identity.",
                "rows",script.rows,"priorities",script.priorities,"loops",script.loops,"actions",script.actions,"events",script.events,"metadata",script.metadata);
            final String directory=System.getProperty("spellwake.evidence.dir");Assert.assertNotNull(directory);
            Files.write(Paths.get(directory,scenario+".json"),(new GsonBuilder().serializeNulls().create().toJson(trace)+"\n").getBytes(StandardCharsets.UTF_8),java.nio.file.StandardOpenOption.CREATE_NEW,java.nio.file.StandardOpenOption.WRITE);
            System.out.println("RESEARCH scenario="+scenario+" completed="+completed+" rows="+script.rows.size()+" priorities="+script.priorities.size());
        }
    }
    private static Map<String,Object> map(Object... pairs){final Map<String,Object> out=new LinkedHashMap<>();for(int i=0;i<pairs.length;i+=2)out.put((String)pairs[i],pairs[i+1]);return out;}
    public static final class Script {
        final Game game;final Player actor,other;final String scenario;
        final List<Map<String,Object>> rows=new ArrayList<>(),priorities=new ArrayList<>(),loops=new ArrayList<>(),actions=new ArrayList<>(),events=new ArrayList<>();
        final Map<String,Object> metadata=new LinkedHashMap<>();
        final java.util.Set<String> recorded=new java.util.HashSet<>();
        Card spell,response,target,manaSource; SpellAbility selected;Player payer;String selectedId,spend;boolean cast,responseCast,activated,done;int actualActions;Throwable eventFailure;
        Script(Game game,String scenario){this.game=game;this.scenario=scenario;actor=game.getPlayers().get(1);other=game.getPlayers().get(0);for(Player p:game.getPlayers())p.addController(game.getNextTimestamp(),p,new StrictController(game,p,this),false);Assert.assertFalse(actor.getController().isAI());Assert.assertFalse(other.getController().isAI());}
        String alias(Player p){if(p==actor)return "A";if(p==other)return "B";throw new AssertionError("Unknown player");}
        Card current(Card c){return game.getCardState(c);}
        String zone(Card c){return c==null?null:current(c).getZone().getZoneType().name();}
        Card seedCard(String id,String name,Player owner,ZoneType zone){
            final String edition=name.equals("Counterspell")?"7ED":name.equals("Force of Savagery")?"FUT":"8ED";
            final String number=name.equals("Counterspell")?"67":name.equals("Force of Savagery")?"126":name.equals("Craw Wurm")?"239":name.equals("Rod of Ruin")?"312":"335";
            final IPaperCard paper=FModel.getMagicDb().getCommonCards().getCard(name,edition,number);Assert.assertNotNull(paper);
            final Card c=Card.fromPaperCard(paper,owner);c.setGameTimestamp(game.getNextTimestamp());owner.getZone(zone).add(c);
            metadata.put(id,map("name",name,"owner",alias(owner),"zone",zone.name(),"edition",paper.getEdition(),"collector_number",paper.getCollectorNumber(),"oracle",paper.getRules().getOracleText(),"card_id",c.getId()));return c;
        }
        void seed(){
            spell=seedCard("spell",scenario.equals("artifact-ability")?"Rod of Ruin":scenario.equals("zero-toughness")?"Force of Savagery":"Craw Wurm",actor,ZoneType.Hand);
            if(scenario.equals("creature-counter"))response=seedCard("response","Counterspell",other,ZoneType.Hand);
            if(scenario.equals("artifact-ability"))target=seedCard("target","Craw Wurm",other,ZoneType.Battlefield);
            manaSource=seedCard("mana-source","Island",actor,ZoneType.Battlefield);
            for(int i=0;i<4;i++){seedCard("A-library-"+i,"Island",actor,ZoneType.Library);seedCard("B-library-"+i,"Island",other,ZoneType.Library);}
        }
        Map<String,Object> control(){return map("turn",game.getPhaseHandler().getTurn(),"step",game.getPhaseHandler().getPhase().name(),"active",alias(game.getPhaseHandler().getPlayerTurn()),"priority",alias(game.getPhaseHandler().getPriorityPlayer()),"stack_size",game.getStack().size());}
        Map<String,Object> pool(Player p){final Map<String,Object> m=new LinkedHashMap<>();for(char c:"WUBRGC".toCharArray())m.put(String.valueOf(c),String.valueOf(p.getManaPool().getAmountOfColor(ManaAtom.fromName(String.valueOf(c)))));return m;}
        void record(String id,String kind,Player p){
            Assert.assertTrue(recorded.add(id),"duplicate row "+id);final Card s=current(spell);final boolean field=s.isInPlay();final Card t=target==null?null:current(target);
            final Map<String,Object> v=control();v.putAll(map("id",id,"kind",kind,"actor",alias(p),"spell_zone",zone(spell),"spell_owner",alias(s.getOwner()),"spell_controller",field||s.isInZone(ZoneType.Stack)?alias(s.getController()):null,"spell_tapped",field?s.isTapped():null,"spell_power",field&&s.isCreature()?s.getNetPower():null,"spell_toughness",field&&s.isCreature()?s.getNetToughness():null,"response_zone",zone(response),"target_zone",zone(target),"target_damage",t!=null&&t.isInPlay()?t.getDamage():null,"target_toughness",t!=null&&t.isInPlay()?t.getNetToughness():null,"mana",map("A",pool(actor),"B",pool(other)),"protocol",kind.equals("resolved-boundary")?"physical-after-loop":kind.equals("prepared")?"prepared-current-holder":"actual-paid-action"));rows.add(v);
        }
        void floating(Player p,String colors){for(char c:colors.toCharArray())p.getManaPool().addMana(new Mana(ManaAtom.fromName(String.valueOf(c)),manaSource,null,p));actions.add(map("kind","trusted-floating-seed","actor",alias(p),"colors",colors,"step",game.getPhaseHandler().getPhase().name()));}
        List<SpellAbility> chooseSpell(Player p,Card card,String id,String colors){selected=current(card).getFirstSpellAbility();selected.setActivatingPlayer(p);payer=p;selectedId=id;spend=colors;floating(p,colors);Assert.assertTrue(selected.canPlay());return java.util.Collections.singletonList(selected);}
        List<SpellAbility> choose(Player p){
            Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertFalse(p.getController().isAI());final Map<String,Object> point=control();point.putAll(map("ordinal",priorities.size(),"chooser",alias(p)));priorities.add(point);
            if(game.getPhaseHandler().getTurn()!=1||game.getPhaseHandler().getPhase()!=PhaseType.MAIN1)return null;
            final String prefix=scenario.equals("artifact-ability")?"A":scenario.equals("creature-counter")?"K":scenario.equals("zero-toughness")?"Z":"C";
            if(p==actor&&!cast){final List<SpellAbility> choice=chooseSpell(p,spell,prefix+"-cast",scenario.equals("artifact-ability")?"CCCC":scenario.equals("zero-toughness")?"GCC":"GGCCCC");Assert.assertFalse(selected.usesTargeting());record(prefix+"-before","prepared",p);return choice;}
            if(scenario.equals("creature-counter")&&p==other&&cast&&!responseCast&&game.getStack().size()==1)return chooseSpell(p,response,"K-response","UU");
            if(p==actor&&cast&&game.getStack().isEmpty()){
                if(scenario.equals("artifact-ability")){
                    if(!activated){Assert.assertEquals(zone(spell),"Battlefield");record("A-settled","resolved-boundary",p);selected=null;for(SpellAbility candidate:current(spell).getNonManaAbilities())if(candidate.isActivatedAbility()){Assert.assertNull(selected);selected=candidate;}Assert.assertNotNull(selected);Assert.assertTrue(selected.usesTargeting());selected.setActivatingPlayer(p);payer=p;selectedId="A-activated";spend="CCC";floating(p,spend);Assert.assertTrue(selected.canPlay());return java.util.Collections.singletonList(selected);}
                    record("A-damage","resolved-boundary",p);done=true;
                }else {record(prefix+"-settled","resolved-boundary",p);done=true;}
            }
            return null;
        }
        boolean play(Player p,SpellAbility sa){
            Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertSame(game.getPhaseHandler().getPriorityPlayer(),p);Assert.assertTrue(sa.canPlay());final int before=game.getStack().size();final Card source=sa.getHostCard();final String initial=zone(source);final boolean ability=sa.isActivatedAbility();
            Assert.assertEquals(initial,ability?"Battlefield":"Hand");Assert.assertTrue(PlaySpellAbility.playSpellAbility(p.getController(),p,sa));Assert.assertEquals(game.getStack().size(),before+1);
            if(ability){Assert.assertTrue(current(spell).isTapped());activated=true;}else {Assert.assertEquals(zone(source),"Stack");if(p==actor)cast=true;else responseCast=true;}
            actualActions++;record(selectedId,ability?"ability-activation":p==actor?"permanent-cast":"instant-cast",p);
            actions.add(map("kind",ability?"actual-paid-activation":"actual-paid-cast","id",selectedId,"actor",alias(p),"before_stack",before,"after_stack",game.getStack().size(),"post_wrapper_original_cost",sa.getPayCosts().toString(),"selected_spend",spend,"target_count",game.getStack().peekAbility().getTargets().size()));return true;
        }
        SpellAbility select(Player p,Card host,List<SpellAbility> options){Assert.assertSame(p,payer);Assert.assertEquals(options.size(),1);Assert.assertSame(options.get(0),selected);return selected;}
        boolean target(Player p,SpellAbility sa){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);if(p==other){final SpellAbility original=game.getStack().peekAbility(); Assert.assertEquals(original.getHostCard().getId(),spell.getId()); Assert.assertTrue(sa.canTarget(original));sa.getTargets().add(original);}else {Assert.assertTrue(sa.canTarget(current(target)));sa.getTargets().add(current(target));}Assert.assertTrue(sa.isTargetNumberValid());return true;}
        List<CostPart> order(Player p,List<CostPart> costs){Assert.assertSame(p,payer);for(CostPart c:costs)Assert.assertTrue(c instanceof CostPartMana||c instanceof CostTap);actions.add(map("kind","cost-order","actor",alias(p),"parts",costs.stream().map(c->c.getClass().getSimpleName()).toList()));return costs;}
        boolean pay(Player p,ManaCost toPay,CostPartMana cost,SpellAbility sa,String prompt,ManaConversionMatrix matrix,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);actions.add(map("kind","mana-payment","actor",alias(p),"cost",toPay.toString()));return PlaySpellAbility.payManaCost(p.getController(),toPay,cost,sa,p,prompt,matrix,false);}
        boolean apply(Player p,ManaCostBeingPaid toPay,SpellAbility sa,boolean effect){Assert.assertSame(p,payer);Assert.assertSame(sa,selected);Assert.assertFalse(effect);p.pushPaidForSA(sa);sa.setManaCostBeingPaid(toPay);try{for(char c:spend.toCharArray())Assert.assertTrue(p.getManaPool().tryPayCostWithColor(ManaAtom.fromName(String.valueOf(c)),sa,toPay,sa.getPayingMana()));Assert.assertTrue(toPay.isPaid());return true;}finally{sa.setManaCostBeingPaid(null);p.popPaidForSA();}}
        void attackers(Player p,Combat combat){throw new AssertionError("unexpected combat");}
        void blockers(Player p,Combat combat){throw new AssertionError("unexpected combat");}
        @Subscribe public void onEvent(GameEvent event){if(eventFailure!=null)return;try{
            if(event instanceof GameEventPlayerPriority e)events.add(map("kind","priority-event","priority",alias(game.getPlayer(e.priority().getId())),"step",e.phase().name()));
            if(event instanceof GameEventSpellResolved e)events.add(map("kind","resolved","fizzled",e.hasFizzled(),"step",game.getPhaseHandler().getPhase().name()));
            if(event instanceof GameEventCardChangeZone e&&(e.card().getId()==spell.getId()||response!=null&&e.card().getId()==response.getId())){final boolean own=e.card().getId()==spell.getId();final Card c=current(own?spell:response);events.add(map("kind","zone","card",own?"spell":"response","from",e.from()==null?null:e.from().zoneType().name(),"to",e.to()==null?null:e.to().zoneType().name(),"priority_callbacks_before",priorities.size(),"controller",c.isInPlay()||c.isInZone(ZoneType.Stack)?alias(c.getController()):null,"tapped",c.isInPlay()?c.isTapped():null,"power",c.isInPlay()&&c.isCreature()?c.getNetPower():null,"toughness",c.isInPlay()&&c.isCreature()?c.getNetToughness():null));}
        }catch(Throwable e){eventFailure=e;}}
        void validate(){Assert.assertTrue(done);Assert.assertEquals(actualActions,scenario.equals("artifact-ability")||scenario.equals("creature-counter")?2:1);Assert.assertEquals(rows.size(),scenario.equals("artifact-ability")?5:scenario.equals("creature-counter")?4:3);if(scenario.equals("artifact-ability")){Assert.assertEquals(current(target).getDamage(),1);Assert.assertTrue(current(spell).isTapped());}if(scenario.equals("zero-toughness")||scenario.equals("creature-counter"))Assert.assertEquals(zone(spell),"Graveyard");else Assert.assertEquals(zone(spell),"Battlefield");}
    }
    private static final class StrictCosts extends CostDecisionMakerBase {
        private final Script script;
        StrictCosts(Player player,SpellAbility ability,Script script) { super(player,ability.isTrigger(),ability,ability.getHostCard()); this.script=script; }
        @Override public boolean paysRightAfterDecision() {return true;}
        @Override public PaymentDecision visit(CostTap cost) {Assert.assertSame(ability,script.selected); Assert.assertSame(player,script.payer); Assert.assertTrue(cost.canPay(ability,player,false)); script.actions.add(map("kind","tap-cost-decision","actor",script.alias(player))); return PaymentDecision.number(0);}
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
        @Override public void declareAttackers(Player attacker, Combat combat) { script.attackers(getPlayer(),combat); }
        @Override public void declareBlockers(Player defender, Combat combat) { script.blockers(getPlayer(),combat); }
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
