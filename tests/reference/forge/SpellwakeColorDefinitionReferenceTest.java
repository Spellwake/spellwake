package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
import forge.card.MagicColor;
import forge.game.Game;
import forge.game.ability.AbilityFactory;
import forge.game.ability.AbilityUtils;
import forge.game.card.Card;
import forge.game.player.Player;
import forge.game.spellability.SpellAbility;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import java.util.List;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Prepared physical zones and direct static/Animate evaluation only. These
 * observations do not compare casting, costs, priority or complete cards/games.
 */
public class SpellwakeColorDefinitionReferenceTest extends AITest {
    private Card synthetic(Game game, Player player, List<String> lines) {
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp());
        player.getZone(ZoneType.Battlefield).add(card);
        return card;
    }
    private void row(String checkpoint, Card card) {
        System.out.println("SPELLWAKE_COLOR_DEFINITION {\"checkpoint\":\"" + checkpoint
            + "\",\"color_bits\":" + card.getColor().getColor() + "}");
    }
    @Test public void realDevoidWorksInAllRepresentedPhysicalZones() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        for (ZoneType zone : List.of(ZoneType.Library, ZoneType.Hand, ZoneType.Graveyard,
                ZoneType.Battlefield, ZoneType.Exile, ZoneType.Command, ZoneType.Stack)) {
            Card card;
            if (zone == ZoneType.Stack) {
                card = createCard("Mist Intruder", player);
                card.setGameTimestamp(game.getNextTimestamp());
                game.getStackZone().add(card);
            } else {
                card = addCardToZone("Mist Intruder", player, zone);
            }
            game.getAction().checkStaticAbilities();
            Assert.assertTrue(card.getColor().isColorless(), zone.toString());
            row("zone." + zone, card);
        }
        Card instant = addCardToZone("Kozilek's Return", player, ZoneType.Hand);
        game.getAction().checkStaticAbilities();
        Assert.assertTrue(instant.getColor().isColorless());
        row("instant.hand", instant);
    }
    @Test public void olderOrdinaryColorEffectAppliesAfterNewerDefinition() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        synthetic(game, player, List.of("Name:Spellwake Red Field", "ManaCost:0", "Types:Artifact", "Oracle:",
            "S:Mode$ Continuous | Affected$ Creature | SetColor$ Red"));
        Card card = synthetic(game, player, List.of("Name:Spellwake Blue Definition", "ManaCost:G", "Types:Creature", "PT:3/3", "Oracle:",
            "S:Mode$ Continuous | Affected$ Card.Self | CharacteristicDefining$ True | EffectZone$ All | SetColor$ Blue"));
        game.getAction().checkStaticAbilities();
        Assert.assertEquals(card.getColor().getColor(), MagicColor.RED);
        row("older-ordinary-wins", card);
    }
    @Test public void losingDevoidDoesNotUndoEarlierLayerColor() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        Card card = addCard("Mist Intruder", player);
        game.getAction().checkStaticAbilities();
        Assert.assertTrue(card.getColor().isColorless());
        Card source = addCardToZone("Turn to Frog", player, ZoneType.Hand);
        SpellAbility ability = AbilityFactory.getAbility(
            "DB$ Animate | ValidTgts$ Creature | RemoveAllAbilities$ True", source);
        ability.setActivatingPlayer(player);
        ability.getTargets().add(card);
        AbilityUtils.resolve(ability);
        row("direct-remove.before-recheck", card);
        game.getAction().checkStaticAbilities();
        row("direct-remove.after-recheck", card);
        Assert.assertFalse(card.hasKeyword("Devoid"));
        Assert.assertTrue(card.getColor().isColorless());
        row("removed-still-colorless", card);
    }
    @Test public void ordinaryStaticRemovalKeepsDevoidColor() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        Card card = addCard("Mist Intruder", player);
        addCard("Humility", player);
        game.getAction().checkStaticAbilities();
        row("static-remove.observed", card);
        Assert.assertFalse(card.hasKeyword("Devoid"));
        Assert.assertTrue(card.getColor().isColorless());
    }
}
