package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
import forge.game.Game;
import forge.game.ability.AbilityFactory;
import forge.game.ability.AbilityUtils;
import forge.game.card.*;
import forge.game.player.Player;
import forge.game.spellability.SpellAbility;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import com.google.gson.GsonBuilder;
import java.nio.file.*;
import java.util.*;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Direct physical stack admission and prescribed targetless copied-spell play.
 * Actual CopySpellAbility/Counter handlers and stack resolution. No original
 * casting, costs, ordinary priority, complete card or current noncard CDA claim.
 */
public class SpellwakeKindredSpellCopyTest extends AITest {
    private static final class Controller extends StrictRejectingController {
        final Player copyOwner;final List<SpellAbility> copies=new ArrayList<>();
        Controller(Game game,Player player,Player owner){super(game,player,player.getLobbyPlayer());copyOwner=owner;}
        @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected callback "+method);}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
        @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){
            if(abilities.isEmpty())return;
            Assert.assertSame(getPlayer(),copyOwner);Assert.assertEquals(abilities.size(),1);Assert.assertTrue(copies.size()<2);
            SpellAbility next=abilities.get(0);Assert.assertTrue(next.isCopied());Assert.assertTrue(next.isSpell());
            Assert.assertTrue(next.getHostCard().isCopiedSpell());Assert.assertSame(next.getActivatingPlayer(),getPlayer());
            Assert.assertSame(next.getHostCard().getOwner(),getPlayer());Assert.assertEquals(next.getTargets().size(),0);
            if(!next.getHostCard().isInZone(ZoneType.Stack))next.setHostCard(getGame().getAction().moveToStack(next.getHostCard(),next));
            else getGame().getStackZone().add(next.getHostCard());
            Assert.assertTrue(next.setupNewTargets(getPlayer()));getGame().getStack().add(next);copies.add(getGame().getStack().peekAbility());
        }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType zone,SpellAbility source){Assert.assertTrue(cards.size()<=1);return cards;}
    }
    private Card seed(Game game,Player actor,String name,String type,String... extra){
        List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:0","Types:"+type,"Oracle:"));lines.addAll(List.of(extra));
        Card value=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),actor);
        value.setGameTimestamp(game.getNextTimestamp());actor.getZone(ZoneType.Hand).add(value);return value;
    }
    private void apply(Player actor,Card host,SpellAbility target,String script){
        SpellAbility ability=AbilityFactory.getAbility(script,host);ability.setActivatingPlayer(actor);ability.getTargets().add(target);AbilityUtils.resolve(ability);
    }
    private void capture(List<Map<String,Object>> rows,String point,SpellAbility ability){
        Card value=ability.getHostCard();Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);
        row.put("types",value.getType().getCoreTypes().stream().map(Enum::name).sorted().toList());
        row.put("creature_types",value.getType().getCreatureTypes().stream().sorted().toList());row.put("creature",value.isCreature());
        row.put("copied_spell",value.isCopiedSpell());row.put("zone",value.getZone().getZoneType().name());rows.add(row);
    }
    private void traits(SpellAbility ability){Assert.assertTrue(ability.getHostCard().isKindred());Assert.assertFalse(ability.getHostCard().isCreature());Assert.assertTrue(ability.getHostCard().isInstant());Assert.assertEquals(ability.getHostCard().getType().getCreatureTypes(),Set.of("Elf"));}
    @Test public void copyOfCopyRetainsKindredTraitsAfterOriginalIsCountered() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;Game game=null;Player actor=null;Controller owner=null;
        try {
            game=initAndCreateGame();actor=game.getPlayers().get(1);
            for(Player player:game.getPlayers()){
                Controller controller=new Controller(game,player,actor);player.dangerouslySetController(controller);
                Assert.assertFalse(controller.isAI());if(player==actor)owner=controller;
            }
            Assert.assertNotNull(owner);
            Card originalCard=seed(game,actor,"Original Kindred","Kindred Instant Elf","A:SP$ GainLife | Cost$ 0 | LifeAmount$ 1");
            Card host=seed(game,actor,"Copy and Counter Host","Instant");
            SpellAbility original=originalCard.getFirstSpellAbility();original.setActivatingPlayer(actor);
            game.getStack().addAndUnfreeze(original);original=game.getStack().peekAbility();traits(original);capture(rows,"original-admitted",original);
            apply(actor,host,original,"DB$ CopySpellAbility | ValidTgts$ Spell | Controller$ You");
            Assert.assertEquals(owner.copies.size(),1);SpellAbility first=owner.copies.get(0);traits(first);capture(rows,"first-copy",first);
            apply(actor,host,original,"DB$ Counter | ValidTgts$ Spell");
            Assert.assertEquals(game.getCardState(originalCard).getZone().getZoneType(),ZoneType.Graveyard);
            Assert.assertEquals(game.getStack().size(),1);traits(first);capture(rows,"first-after-original-exit",first);
            apply(actor,host,first,"DB$ CopySpellAbility | ValidTgts$ Spell | Controller$ You");
            Assert.assertEquals(owner.copies.size(),2);SpellAbility second=owner.copies.get(1);
            Assert.assertNotEquals(first.getHostCard().getId(),second.getHostCard().getId());traits(second);capture(rows,"copy-of-copy",second);
            game.getStack().resolveStack();game.getAction().checkStateEffects(true);
            Assert.assertEquals(game.getStack().size(),1);traits(first);capture(rows,"first-after-second-resolved",first);
            game.getStack().resolveStack();game.getAction().checkStateEffects(true);
            Assert.assertTrue(game.getStack().isEmpty());Assert.assertEquals(actor.getLife(),22);
            Assert.assertFalse(game.getCardsInGame().contains(first.getHostCard()));Assert.assertFalse(game.getCardsInGame().contains(second.getHostCard()));
            passed=true;
        } finally {
            Map<String,Object> result=new LinkedHashMap<>();result.put("scenario","owned-spell-copy");result.put("assertions_passed",passed);
            result.put("direct_original_stack_admission",true);result.put("normal_original_casting",false);result.put("costs_or_priority_compared",false);
            result.put("prescribed_targetless_copy_play",true);result.put("controllers_ai",game==null?null:game.getPlayers().stream().anyMatch(p->p.getController().isAI()));
            result.put("copies",owner==null?null:owner.copies.size());result.put("final_life",actor==null?null:actor.getLife());result.put("rows",rows);
            Path dir=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
            Files.writeString(dir.resolve("owned-spell-copy.json"),new GsonBuilder().serializeNulls().create().toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
        }
    }
}
