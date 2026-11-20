package forge.ai;

import forge.card.CardType;
import com.google.gson.GsonBuilder;
import java.nio.file.*;
import java.util.*;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Direct actual CardType parser/normalizer probes. No game, casting, priority,
 * continuous-effect ordering, full copied spell or complete card is qualified.
 */
public class SpellwakeCardTypeFamiliesReferenceTest extends AITest {
    private Map<String,Object> observe(String point, CardType value) {
        Map<String,Object> row=new LinkedHashMap<>();
        row.put("point",point);
        row.put("types",value.getCoreTypes().stream().map(Enum::name).sorted().toList());
        row.put("creature_types",value.getCreatureTypes().stream().sorted().toList());
        row.put("creature",value.isCreature());row.put("kindred",value.isKindred());
        row.put("permanent",value.isPermanent());return row;
    }
    private void write(String scenario,boolean passed,List<Map<String,Object>> rows) throws Exception {
        Map<String,Object> result=new LinkedHashMap<>();result.put("scenario",scenario);
        result.put("assertions_passed",passed);result.put("scope","direct CardType parse/remove/set; no gameplay lifecycle");
        result.put("rows",rows);
        Path directory=Path.of(Objects.requireNonNull(System.getProperty("spellwake.evidence.dir")));
        Files.writeString(directory.resolve(scenario+".json"),new GsonBuilder().create().toJson(result)+"\n",StandardOpenOption.CREATE_NEW);
    }
    @Test public void kindredEnchantmentCarriesCreatureFamilyWithoutBeingCreature() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            CardType value=CardType.parse("Kindred Enchantment Elf",false);rows.add(observe("parsed",value));
            Assert.assertFalse(value.isCreature());Assert.assertTrue(value.isKindred());
            Assert.assertTrue(value.isPermanent());Assert.assertEquals(value.getCreatureTypes(),Set.of("Elf"));
            CardType kindred=CardType.parse("Kindred",false);rows.add(observe("type-category-only",kindred));
            Assert.assertFalse(kindred.isPermanent());passed=true;
        } finally {write("shared-family",passed,rows);}
    }
    @Test public void losingOneCarrierRetainsFamilyAndLosingTheLastClearsIt() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            CardType value=CardType.parse("Kindred Enchantment Creature Elf",false);rows.add(observe("initial",value));
            Assert.assertTrue(value.remove("Creature"));rows.add(observe("lost-creature",value));
            Assert.assertFalse(value.isCreature());Assert.assertEquals(value.getCreatureTypes(),Set.of("Elf"));
            Assert.assertTrue(value.remove("Kindred"));rows.add(observe("lost-last-family",value));
            Assert.assertTrue(value.getCreatureTypes().isEmpty());Assert.assertTrue(value.isEnchantment());passed=true;
        } finally {write("carrier-loss",passed,rows);}
    }
    @Test public void settingCreatureTypesUsesFamilyMembershipRatherThanCreatureBehavior() throws Exception {
        List<Map<String,Object>> rows=new ArrayList<>();boolean passed=false;
        try {
            CardType value=CardType.parse("Kindred Enchantment Elf",false);
            Assert.assertTrue(value.setCreatureTypes(List.of("Goblin")));rows.add(observe("kindred-set",value));
            Assert.assertEquals(value.getCreatureTypes(),Set.of("Goblin"));Assert.assertFalse(value.isCreature());
            CardType ordinary=CardType.parse("Artifact",false);
            Assert.assertFalse(ordinary.setCreatureTypes(List.of("Goblin")));rows.add(observe("no-family-set",ordinary));
            Assert.assertTrue(ordinary.getCreatureTypes().isEmpty());passed=true;
        } finally {write("family-setting",passed,rows);}
    }
}
