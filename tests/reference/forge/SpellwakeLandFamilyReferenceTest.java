package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
import forge.card.MagicColor;
import forge.game.Game;
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

/** CR305.6-8/613 probes in prepared synthetic/real-card worlds. Direct mana
 * resolution deliberately does not pay a tap cost or run normal priority/SBAs.
 * Strict non-AI controllers reject every unprescribed choice. No full-card claim.
 */
public class SpellwakeLandFamilyReferenceTest extends AITest {
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
    private Card seed(Game game,Player player,String name,String type,String... extra){
        List<String> lines=new ArrayList<>(List.of("Name:Spellwake "+name,"ManaCost:0","Types:"+type,"PT:2/3","Oracle:"));
        lines.addAll(List.of(extra));Card value=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines),"???",CardRarity.Common),player);
        value.setGameTimestamp(game.getNextTimestamp());player.getZone(ZoneType.Battlefield).add(value);return value;
    }
    private void settle(Game game){game.getAction().checkStaticAbilities();}
    private int nonmanaActivated(Card value){return (int)value.getNonManaAbilities().stream().filter(SpellAbility::isActivatedAbility).count();}
    private List<String> outputs(Card value){return value.getManaAbilities().stream().map(a->a.getManaPart().getOrigProduced()).sorted().toList();}
    private Map<String,Object> observe(String point,Card value){
        Map<String,Object> row=new LinkedHashMap<>();row.put("point",point);
        row.put("land_types",value.getType().getLandTypes().stream().sorted().toList());row.put("mana_outputs",outputs(value));
        row.put("types",value.getType().getCoreTypes().stream().map(Enum::name).sorted().toList());
        row.put("creature_types",value.getType().getCreatureTypes().stream().sorted().toList());row.put("basic",value.getType().isBasic());
        row.put("nonmana_raw_entries",value.getNonManaAbilities().size());row.put("nonmana_activated",nonmanaActivated(value));row.put("statics",value.getStaticAbilities().size());row.put("remove_intrinsic",value.hasRemoveIntrinsic());
        row.put("zone",value.getZone().getZoneType().name());row.put("tapped",value.isTapped());return row;
    }
    private void write(String scenario,boolean passed,List<Map<String,Object>> rows) throws Exception {
        Map<String,Object> value=new LinkedHashMap<>();value.put("scenario",scenario);value.put("assertions_passed",passed);value.put("rows",rows);
        value.put("scope","prepared face-up zones/static reevaluation; direct mana handler only where explicitly exercised; no tap cost, normal land play, priority, SBAs or full cards");
        value.put("controllers_ai",false);Path out=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
        Files.writeString(out.resolve(scenario+".json"),new GsonBuilder().create().toJson(value)+"\n",StandardOpenOption.CREATE_NEW);
    }
    @Test public void allBasicSubsetsOfferSeparateMappedManaAbilities() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            Game game=prepared();Player actor=game.getPlayers().get(1);
            String[] types={"Plains","Island","Swamp","Mountain","Forest"},colors={"W","U","B","R","G"};
            for(int mask=0;mask<32;mask++){
                List<String> present=new ArrayList<>(),expected=new ArrayList<>();
                for(int i=0;i<5;i++)if((mask&(1<<i))!=0){present.add(types[i]);expected.add(colors[i]);}
                Card source=seed(game,actor,"Subset "+mask,"Land "+String.join(" ",present));settle(game);
                rows.add(observe("subset-"+mask,source));Assert.assertEquals(source.getType().getLandTypes(),new HashSet<>(present));
                Assert.assertEquals(outputs(source),expected.stream().sorted().toList());Assert.assertFalse(source.getType().isBasic());
                for(SpellAbility mana:source.getManaAbilities()){
                    String color=mana.getManaPart().getOrigProduced();byte atom=MagicColor.fromName(color);int before=actor.getManaPool().getAmountOfColor(atom);
                    mana.setActivatingPlayer(actor);AbilityUtils.resolve(mana);Assert.assertEquals(actor.getManaPool().getAmountOfColor(atom),before+1);
                    Assert.assertFalse(source.isTapped(),"direct body resolution has no tap-cost premise");
                }
            }
            passed=true;
        } finally {write("basic-subsets",passed,rows);}
    }
    @Test public void addingBasicTypesPreservesTextWhileSettingRemovesItAndRestores() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            Game game=prepared();Player actor=game.getPlayers().get(1);
            Card source=seed(game,actor,"Text Land","Legendary Snow Artifact Creature Land Elf Gate","A:AB$ Mana | Cost$ T | Produced$ C","A:AB$ GainLife | Cost$ T | LifeAmount$ 1");
            settle(game);rows.add(observe("printed",source));Assert.assertEquals(outputs(source),List.of("C"));Assert.assertEquals(nonmanaActivated(source),1);
            Card add=seed(game,actor,"Add Swamp","Enchantment","S:Mode$ Continuous | Affected$ Land | AddType$ Swamp");settle(game);
            rows.add(observe("add",source));Assert.assertEquals(outputs(source),List.of("B","C"));Assert.assertEquals(source.getType().getLandTypes(),Set.of("Gate","Swamp"));Assert.assertEquals(nonmanaActivated(source),1);Assert.assertFalse(source.hasRemoveIntrinsic());
            Card set=seed(game,actor,"Set Mountain","Enchantment","S:Mode$ Continuous | Affected$ Land | AddType$ Mountain | RemoveLandTypes$ True");settle(game);
            rows.add(observe("set",source));Assert.assertEquals(outputs(source),List.of("R"));Assert.assertEquals(source.getType().getLandTypes(),Set.of("Mountain"));Assert.assertEquals(nonmanaActivated(source),0);Assert.assertTrue(source.hasRemoveIntrinsic());
            Assert.assertTrue(source.isCreature());Assert.assertTrue(source.isArtifact());Assert.assertTrue(source.getType().isLegendary());Assert.assertTrue(source.getType().isSnow());Assert.assertEquals(source.getType().getCreatureTypes(),Set.of("Elf"));
            game.getAction().moveToHand(set,null);settle(game);rows.add(observe("set-left",source));Assert.assertEquals(outputs(source),List.of("B","C"));Assert.assertEquals(nonmanaActivated(source),1);
            game.getAction().moveToHand(add,null);settle(game);rows.add(observe("add-left",source));Assert.assertEquals(outputs(source),List.of("C"));Assert.assertEquals(source.getType().getLandTypes(),Set.of("Gate"));passed=true;
        } finally {write("add-set-restore",passed,rows);}
    }
    @Test public void laterAllRemovalClearsBasicManaAndNonmanaRemovalPreservesIt() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            for(boolean all:List.of(false,true)){
                Game game=prepared();Player actor=game.getPlayers().get(1);Card source=seed(game,actor,"Intrinsic","Land Forest Island");
                Card remove=seed(game,actor,"Removal","Enchantment","S:Mode$ Continuous | Affected$ Land | "+(all?"RemoveAllAbilities":"RemoveNonManaAbilities")+"$ True");settle(game);
                rows.add(observe(all?"all":"nonmana",source));Assert.assertEquals(outputs(source),all?List.of():List.of("G","U"));
                game.getAction().moveToHand(remove,null);settle(game);rows.add(observe("restored-"+all,source));Assert.assertEquals(outputs(source),List.of("G","U"));
            }
            passed=true;
        } finally {write("later-removal",passed,rows);}
    }
    @Test public void bloodMoonSuppressesUrborgInBothTimestampOrdersAndDepartureRestores() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            for(boolean moonFirst:List.of(false,true)){
                Game game=prepared();Player actor=game.getPlayers().get(1);Card forest=addCard("Forest",actor);
                Card moon,urborg;
                if(moonFirst){moon=addCard("Blood Moon",actor);urborg=addCard("Urborg, Tomb of Yawgmoth",actor);}else{urborg=addCard("Urborg, Tomb of Yawgmoth",actor);moon=addCard("Blood Moon",actor);}
                settle(game);rows.add(observe("urborg-moonFirst-"+moonFirst,urborg));rows.add(observe("forest-moonFirst-"+moonFirst,forest));
                Assert.assertEquals(urborg.getType().getLandTypes(),Set.of("Mountain"));Assert.assertTrue(urborg.getStaticAbilities().isEmpty());Assert.assertEquals(outputs(urborg),List.of("R"));
                Assert.assertEquals(forest.getType().getLandTypes(),Set.of("Forest"));Assert.assertEquals(outputs(forest),List.of("G"));
                game.getAction().moveToHand(moon,null);settle(game);rows.add(observe("urborg-restored-"+moonFirst,urborg));rows.add(observe("forest-restored-"+moonFirst,forest));
                Assert.assertEquals(urborg.getType().getLandTypes(),Set.of("Swamp"));Assert.assertFalse(urborg.getStaticAbilities().isEmpty());Assert.assertEquals(outputs(urborg),List.of("B"));
                Assert.assertEquals(forest.getType().getLandTypes(),Set.of("Forest","Swamp"));Assert.assertEquals(outputs(forest),List.of("B","G"));
            }
            passed=true;
        } finally {write("moon-urborg",passed,rows);}
    }
}
