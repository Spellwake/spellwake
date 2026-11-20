package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
import forge.card.MagicColor;
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

/** Prepared face-up zones and direct Mana/Animate/Clone handlers with actual
 * static reevaluation. Strict non-AI controllers reject unexpected decisions.
 * No casting, cost payment, priority, SBAs or complete cards are compared.
 */
public class SpellwakeCardTypeInteractionTest extends AITest {
    private Game prepared(){
        Game game=initAndCreateGame();
        for(Player player:game.getPlayers()){
            player.dangerouslySetController(new StrictRejectingController(game,player,player.getLobbyPlayer()){
                @Override protected AssertionError reject(String method,Object...args){return new AssertionError("Unexpected callback "+method);}
                @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());return abilities;}
                @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities){Assert.assertTrue(abilities.isEmpty());}
                @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards,ZoneType destination,SpellAbility cause){Assert.assertTrue(cards.size()<=1);return cards;}
            });
            Assert.assertFalse(player.getController().isAI());
        }
        return game;
    }
    private Card seed(Game game,Player player,String name,String type,ZoneType zone,String... extra){
        List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:0","Types:"+type,"PT:2/3","Oracle:"));
        lines.addAll(List.of(extra));Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);
        card.setGameTimestamp(game.getNextTimestamp());player.getZone(zone).add(card);return card;
    }
    private Map<String,Object> card(String point,Card value){
        Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);
        row.put("types",value.getType().getCoreTypes().stream().map(Enum::name).sorted().toList());
        row.put("creature_types",value.getType().getCreatureTypes().stream().sorted().toList());
        row.put("creature",value.isCreature());row.put("zone",value.getZone().getZoneType().name());row.put("tapped",value.isTapped());return row;
    }
    private void apply(Game game,Player actor,Card host,Card target,String script){
        SpellAbility ability=AbilityFactory.getAbility(script,host);ability.setActivatingPlayer(actor);ability.getTargets().add(target);
        AbilityUtils.resolve(ability);game.getAction().checkStaticAbilities();
    }
    private void write(String scenario,boolean passed,List<Map<String,Object>> rows) throws Exception {
        Map<String,Object> result=new LinkedHashMap<>();result.put("scenario",scenario);result.put("assertions_passed",passed);
        result.put("prepared_direct_handlers",true);result.put("normal_casting",false);result.put("cost_payment_compared",false);
        result.put("priority_or_sbas_compared",false);result.put("controllers_ai",false);result.put("rows",rows);
        Path dir=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
        Files.writeString(dir.resolve(scenario+".json"),new GsonBuilder().create().toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
    }
    @Test public void countedManaIncludesNoncreatureKindredAndReevaluatesFamilyLoss() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            Game game=prepared();Player actor=game.getPlayers().get(1),other=game.getPlayers().get(0);
            Card source=seed(game,actor,"Kindred Counter","Kindred Artifact Elf",ZoneType.Battlefield,
                "A:AB$ Mana | Cost$ T | Produced$ G | Amount$ X","SVar:X:Count$Valid Elf.YouCtrl");
            Card kindred=seed(game,actor,"Kindred Enchantment","Kindred Enchantment Elf",ZoneType.Battlefield);
            seed(game,actor,"Creature Elf","Creature Elf",ZoneType.Battlefield);
            seed(game,other,"Opposing Elf","Kindred Enchantment Elf",ZoneType.Battlefield);
            Card host=seed(game,actor,"Hand Elf","Kindred Instant Elf",ZoneType.Hand);
            game.getAction().checkStaticAbilities();Assert.assertTrue(actor.getManaPool().isEmpty());
            SpellAbility mana=source.getManaAbilities().get(0);mana.setActivatingPlayer(actor);AbilityUtils.resolve(mana);
            Map<String,Object> first=card("first-production",source);first.put("green",actor.getManaPool().getAmountOfColor(MagicColor.GREEN));rows.add(first);
            Assert.assertEquals(actor.getManaPool().getAmountOfColor(MagicColor.GREEN),3);Assert.assertTrue(other.getManaPool().isEmpty());
            Assert.assertFalse(source.isTapped());
            apply(game,actor,host,kindred,"DB$ Animate | ValidTgts$ Permanent | RemoveTypes$ Kindred | Duration$ Permanent");
            rows.add(card("kindred-lost",kindred));Assert.assertFalse(kindred.isKindred());Assert.assertTrue(kindred.getType().getCreatureTypes().isEmpty());
            mana=source.getManaAbilities().get(0);mana.setActivatingPlayer(actor);AbilityUtils.resolve(mana);
            Map<String,Object> second=card("second-production",source);second.put("green",actor.getManaPool().getAmountOfColor(MagicColor.GREEN));rows.add(second);
            Assert.assertEquals(actor.getManaPool().getAmountOfColor(MagicColor.GREEN),5);Assert.assertFalse(source.isCreature());passed=true;
        } finally {write("counted-mana",passed,rows);}
    }
    @Test public void physicalCopyCapturesKindredDefinitionWithoutLaterAnimation() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            Game game=prepared();Player actor=game.getPlayers().get(1);
            Card donor=seed(game,actor,"Donor","Kindred Enchantment Elf",ZoneType.Battlefield);
            Card receiver=seed(game,actor,"Receiver","Artifact",ZoneType.Battlefield);
            Card host=seed(game,actor,"Animation Host","Instant",ZoneType.Hand);
            apply(game,actor,host,donor,"DB$ Animate | ValidTgts$ Permanent | Types$ Creature,Goblin | RemoveTypes$ Kindred | RemoveCreatureTypes$ True | Power$ 2 | Toughness$ 3 | Duration$ Permanent");
            rows.add(card("donor-current",donor));Assert.assertTrue(donor.isCreature());Assert.assertFalse(donor.isKindred());Assert.assertEquals(donor.getType().getCreatureTypes(),Set.of("Goblin"));
            apply(game,actor,receiver,donor,"DB$ Clone | ValidTgts$ Permanent");
            rows.add(card("receiver-copied",receiver));Assert.assertTrue(receiver.isKindred());Assert.assertFalse(receiver.isCreature());Assert.assertEquals(receiver.getType().getCreatureTypes(),Set.of("Elf"));
            game.getAction().moveToHand(donor,null);game.getAction().checkStaticAbilities();
            rows.add(card("receiver-after-donor-exit",receiver));Assert.assertTrue(receiver.isKindred());Assert.assertFalse(receiver.isCreature());Assert.assertEquals(receiver.getType().getCreatureTypes(),Set.of("Elf"));passed=true;
        } finally {write("physical-copy",passed,rows);}
    }
}
