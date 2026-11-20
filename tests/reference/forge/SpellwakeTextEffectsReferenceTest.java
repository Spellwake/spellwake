package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
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

/** Direct text-handler and static-evaluator probes. Prepared state and EOT
 * callbacks are explicit trusted boundaries; casting is not compared.
 */
public class SpellwakeTextEffectsReferenceTest extends AITest {
    private Card seed(Game game, Player player, String name, String type, boolean lord) {
        var lines = new java.util.ArrayList<>(List.of(
            "Name:" + name, "ManaCost:0", "Types:Creature " + type, "PT:3/3", "Oracle:"));
        if (lord) lines.add("S:Mode$ Continuous | Affected$ Creature.Elf+Other+YouCtrl | AddPower$ 1 | AddToughness$ 1");
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp());
        player.getZone(ZoneType.Battlefield).add(card);
        return card;
    }
    private void change(Game game, Player player, Card target, String from, String to, boolean permanent) {
        Card source = addCardToZone("New Blood", player, ZoneType.Hand);
        SpellAbility ability = AbilityFactory.getAbility("DB$ ChangeText | ValidTgts$ Creature | ChangeTypeWord$ " + from + " " + to
            + (permanent ? " | Duration$ Permanent" : ""), source);
        ability.setActivatingPlayer(player);
        ability.getTargets().add(target);
        AbilityUtils.resolve(ability);
        game.getAction().checkStaticAbilities();
    }
    private void row(String checkpoint, Card source, Card elf, Card vampire, Card goblin) {
        System.out.println("SPELLWAKE_TEXT {\"checkpoint\":\"" + checkpoint
            + "\",\"elf\":" + source.getType().hasSubtype("Elf")
            + ",\"vampire\":" + source.getType().hasSubtype("Vampire")
            + ",\"goblin\":" + source.getType().hasSubtype("Goblin")
            + ",\"name\":\"" + source.getName() + "\",\"powers\":["
            + elf.getNetPower() + "," + vampire.getNetPower() + "," + goblin.getNetPower() + "]}");
    }
    @Test public void typeLineAndPrintedSelectorChangeTogetherThenExpire() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        Card lord = seed(game, player, "Elf", "Elf", true);
        Card elf = seed(game, player, "Text Probe A", "Elf", false);
        Card vampire = seed(game, player, "Text Probe B", "Vampire", false);
        Card goblin = seed(game, player, "Text Probe C", "Goblin", false);
        game.getAction().checkStaticAbilities();
        row("expire.before", lord, elf, vampire, goblin);
        Assert.assertEquals(elf.getNetPower(), 4);
        change(game, player, lord, "Elf", "Vampire", false);
        row("expire.changed", lord, elf, vampire, goblin);
        Assert.assertEquals(lord.getName(), "Elf");
        Assert.assertTrue(lord.getType().hasSubtype("Vampire"));
        Assert.assertFalse(lord.getType().hasSubtype("Elf"));
        Assert.assertEquals(elf.getNetPower(), 3);
        Assert.assertEquals(vampire.getNetPower(), 4);
        game.getEndOfTurn().executeUntil();
        game.getAction().checkStaticAbilities();
        row("expire.after", lord, elf, vampire, goblin);
        Assert.assertTrue(lord.getType().hasSubtype("Elf"));
        Assert.assertEquals(elf.getNetPower(), 4);
        Assert.assertEquals(vampire.getNetPower(), 3);
    }
    @Test public void chainedWordsReevaluateTheRemainingEffectAfterExpiry() {
        Game game = initAndCreateGame();
        Player player = game.getPlayers().get(1);
        Card lord = seed(game, player, "Elf", "Elf", true);
        Card elf = seed(game, player, "Text Probe A", "Elf", false);
        Card vampire = seed(game, player, "Text Probe B", "Vampire", false);
        Card goblin = seed(game, player, "Text Probe C", "Goblin", false);
        change(game, player, lord, "Elf", "Vampire", true);
        row("chain.first", lord, elf, vampire, goblin);
        change(game, player, lord, "Vampire", "Goblin", false);
        row("chain.second", lord, elf, vampire, goblin);
        Assert.assertTrue(lord.getType().hasSubtype("Goblin"));
        Assert.assertEquals(goblin.getNetPower(), 4);
        Assert.assertEquals(vampire.getNetPower(), 3);
        game.getEndOfTurn().executeUntil();
        game.getAction().checkStaticAbilities();
        row("chain.expired", lord, elf, vampire, goblin);
        Assert.assertTrue(lord.getType().hasSubtype("Vampire"));
        Assert.assertEquals(vampire.getNetPower(), 4);
        Assert.assertEquals(goblin.getNetPower(), 3);
    }
}
