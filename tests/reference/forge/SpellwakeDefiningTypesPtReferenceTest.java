package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
import forge.game.Game;
import forge.game.card.Card;
import forge.game.player.Player;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import java.util.List;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Prepared zones and actual static evaluation. No casting, payment, priority,
 * complete card support or identical vocabulary publications are compared.
 */
public class SpellwakeDefiningTypesPtReferenceTest extends AITest {
    private Card seed(Game game, Player player, List<String> lines) {
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp());
        player.getZone(ZoneType.Battlefield).add(card);
        return card;
    }
    private Card inZone(Game game, Player player, String name, ZoneType zone) {
        if (zone != ZoneType.Stack) return addCardToZone(name, player, zone);
        Card card = createCard(name, player);
        card.setGameTimestamp(game.getNextTimestamp());
        game.getStackZone().add(card);
        return card;
    }
    private List<ZoneType> zones() {
        return List.of(ZoneType.Library, ZoneType.Hand, ZoneType.Graveyard, ZoneType.Battlefield,
            ZoneType.Exile, ZoneType.Command, ZoneType.Stack);
    }
    private void row(String point, Card card) {
        System.out.println("SPELLWAKE_CDA {\"checkpoint\":\"" + point
            + "\",\"power\":" + card.getNetPower() + ",\"toughness\":" + card.getNetToughness()
            + ",\"printed_power\":\"" + card.getBasePowerString()
            + "\",\"printed_toughness\":\"" + card.getBaseToughnessString()
            + "\",\"elf\":" + card.getType().hasCreatureType("Elf")
            + ",\"vampire\":" + card.getType().hasCreatureType("Vampire") + "}");
    }
    private void removal(Game game, Player player) {
        seed(game, player, List.of("Name:Spellwake Trait Removal", "ManaCost:0", "Types:Artifact", "Oracle:",
            "S:Mode$ Continuous | Affected$ Creature | RemoveAllAbilities$ True"));
    }
    private Card counted(Game game, Player player, boolean multiPart) {
        return seed(game, player, List.of("Name:Spellwake Count Definition", "ManaCost:0", "Types:Creature", "PT:*/1+*", "Oracle:",
            "S:Mode$ Continuous | CharacteristicDefining$ True | SetPower$ X | SetToughness$ Y"
                + (multiPart ? " | AddAllCreatureTypes$ True" : ""),
            "SVar:X:Count$Valid Land.YouCtrl", "SVar:Y:SVar$X/Plus.1"));
    }
    @Test public void allCreatureTypesWorkAcrossPhysicalZones() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        for (ZoneType zone : zones()) {
            Card card = inZone(game, player, "Mistform Ultimus", zone);
            game.getAction().checkStaticAbilities();
            Assert.assertTrue(card.getType().hasCreatureType("Elf"));
            Assert.assertTrue(card.getType().hasCreatureType("Vampire"));
            Assert.assertTrue(card.getType().hasCreatureType("Illusion"));
            row("types." + zone, card);
        }
    }
    @Test public void realDakkonCountsItsOwnersLandsAcrossZones() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        addCard("Forest", player); addCard("Island", player);
        addCard("Mountain", game.getPlayers().get(0));
        for (ZoneType zone : zones()) {
            Card card = inZone(game, player, "Dakkon Blackblade", zone);
            game.getAction().checkStaticAbilities();
            Assert.assertEquals(card.getNetPower(), 2);
            Assert.assertEquals(card.getNetToughness(), 2);
            Assert.assertEquals(card.getBasePowerString(), "*");
            row("pt." + zone, card);
        }
    }
    @Test public void removedPtDefinitionFallsBackToPrintedStarOffsets() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        addCard("Forest", player); addCard("Island", player);
        Card card = counted(game, player, false);
        game.getAction().checkStaticAbilities();
        Assert.assertEquals(card.getNetPower(), 2); Assert.assertEquals(card.getNetToughness(), 3);
        row("pt.before-removal", card);
        removal(game, player); game.getAction().checkStaticAbilities();
        row("pt.after-removal", card);
        Assert.assertEquals(card.getNetPower(), 0); Assert.assertEquals(card.getNetToughness(), 1);
        Assert.assertEquals(card.getBasePowerString(), "*");
        Assert.assertEquals(card.getBaseToughnessString(), "1+*");
    }
    @Test public void earlierTypePartContinuesIntoPtAfterAbilityRemoval() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        addCard("Forest", player); addCard("Island", player);
        Card card = counted(game, player, true);
        removal(game, player); game.getAction().checkStaticAbilities();
        row("multi.after-removal", card);
        Assert.assertTrue(card.getType().hasCreatureType("Elf"));
        Assert.assertEquals(card.getNetPower(), 2); Assert.assertEquals(card.getNetToughness(), 3);
    }
    @Test public void olderOrdinaryTypeSetterAppliesAfterNewerDefiningTypes() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        seed(game, player, List.of("Name:Spellwake Goblin Field", "ManaCost:0", "Types:Artifact", "Oracle:",
            "S:Mode$ Continuous | Affected$ Creature | AddType$ Goblin | RemoveCreatureTypes$ True"));
        Card card = addCard("Mistform Ultimus", player);
        game.getAction().checkStaticAbilities();
        row("types.ordinary-override", card);
        Assert.assertEquals(card.getType().getCreatureTypes(), java.util.Set.of("Goblin"));
    }
}
