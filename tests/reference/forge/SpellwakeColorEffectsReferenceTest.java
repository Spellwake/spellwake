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
import java.util.ArrayList;
import java.util.List;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Prepared-state direct Animate color handlers, live static evaluation and EOT
 * callbacks. Casting/payment/priority/full-game equivalence is not compared.
 */
public class SpellwakeColorEffectsReferenceTest extends AITest {
    private Card seed(Game game, Player player, boolean target) {
        var lines = new ArrayList<>(target
            ? List.of("Name:Spellwake Color Target", "ManaCost:G", "Types:Creature", "PT:3/3", "Oracle:")
            : List.of("Name:Spellwake Blue Lord", "ManaCost:0", "Types:Artifact", "Oracle:",
                "S:Mode$ Continuous | Affected$ Creature.Blue | AddPower$ 1 | AddToughness$ 1"));
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp());
        player.getZone(ZoneType.Battlefield).add(card);
        return card;
    }
    private void change(Game game, Player player, Card target, String colors, boolean overwrite, boolean permanent) {
        Card source = addCardToZone("Moonlace", player, ZoneType.Hand);
        SpellAbility ability = AbilityFactory.getAbility("DB$ Animate | ValidTgts$ Creature | Colors$ " + colors
            + (overwrite ? " | OverwriteColors$ True" : "") + (permanent ? " | Duration$ Permanent" : ""), source);
        ability.setActivatingPlayer(player);
        ability.getTargets().add(target);
        AbilityUtils.resolve(ability);
        game.getAction().checkStaticAbilities();
    }
    private void row(String checkpoint, Card card) {
        var colors = new ArrayList<String>();
        if (card.getColor().hasWhite()) colors.add("White");
        if (card.getColor().hasBlue()) colors.add("Blue");
        if (card.getColor().hasBlack()) colors.add("Black");
        if (card.getColor().hasRed()) colors.add("Red");
        if (card.getColor().hasGreen()) colors.add("Green");
        String array = colors.isEmpty() ? "[]" : "[\"" + String.join("\",\"", colors) + "\"]";
        System.out.println("SPELLWAKE_COLOR {\"checkpoint\":\"" + checkpoint + "\",\"colors\":" + array
            + ",\"power\":" + card.getNetPower() + ",\"toughness\":" + card.getNetToughness() + "}");
    }
    @Test public void additiveColorExpiresOverAnOlderSetColorAndFeedsBlueStatics() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        Card target = seed(game, player, true);
        seed(game, player, false);
        game.getAction().checkStaticAbilities();
        row("add.before", target);
        Assert.assertTrue(target.getColor().getColor() == MagicColor.GREEN);
        Assert.assertEquals(target.getNetPower(), 3);
        change(game, player, target, "Blue", true, true);
        row("add.set", target);
        Assert.assertTrue(target.getColor().getColor() == MagicColor.BLUE);
        Assert.assertEquals(target.getNetPower(), 4);
        change(game, player, target, "Red", false, false);
        row("add.both", target);
        Assert.assertTrue(target.getColor().getColor() == (MagicColor.BLUE | MagicColor.RED));
        Assert.assertEquals(target.getNetPower(), 4);
        game.getEndOfTurn().executeUntil();
        game.getAction().checkStaticAbilities();
        row("add.expired", target);
        Assert.assertTrue(target.getColor().getColor() == MagicColor.BLUE);
        Assert.assertEquals(target.getNetPower(), 4);
    }
    @Test public void colorlessExpiresBackToTheManaCostDefinedColor() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        Card target = seed(game, player, true);
        game.getAction().checkStaticAbilities();
        row("clear.before", target);
        Assert.assertTrue(target.getColor().getColor() == MagicColor.GREEN);
        change(game, player, target, "Colorless", true, false);
        row("clear.changed", target);
        Assert.assertTrue(target.getColor().isColorless());
        Assert.assertEquals(target.getNetPower(), 3);
        game.getEndOfTurn().executeUntil();
        game.getAction().checkStaticAbilities();
        row("clear.expired", target);
        Assert.assertTrue(target.getColor().getColor() == MagicColor.GREEN);
        Assert.assertEquals(target.getNetPower(), 3);
    }
}
