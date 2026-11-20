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

/** Prepared face-up battlefield, direct Clone/Animate handlers and real static
 * reevaluation. Does not exercise normal announcement, costs, priority or SBAs.
 */
public class SpellwakePermanentCopyReferenceTest extends AITest {
    private Card seed(Game game, Player player, String name, String types, String pt, String... text) {
        var lines = new ArrayList<>(List.of("Name:" + name, "ManaCost:0", "Types:" + types, "Oracle:"));
        if (pt != null) lines.add("PT:" + pt);
        lines.addAll(List.of(text));
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(lines), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp());
        player.getZone(ZoneType.Battlefield).add(card);
        return card;
    }
    private void apply(Game game, Player player, Card host, Card target, String script) {
        SpellAbility ability = AbilityFactory.getAbility(script, host);
        ability.setActivatingPlayer(player);
        ability.getTargets().add(target);
        AbilityUtils.resolve(ability);
        game.getAction().checkStaticAbilities();
    }
    private void copy(Game game, Player player, Card receiver, Card donor, String exception) {
        apply(game, player, receiver, donor, "DB$ Clone | ValidTgts$ Permanent" + exception);
    }
    private void stats(String point, Card card, int power, int toughness) {
        System.out.println("SPELLWAKE_PERMANENT_COPY {\"checkpoint\":\"" + point
            + "\",\"power\":" + card.getNetPower() + ",\"toughness\":" + card.getNetToughness()
            + ",\"elf\":" + card.getType().hasCreatureType("Elf")
            + ",\"vampire\":" + card.getType().hasCreatureType("Vampire") + "}");
        Assert.assertEquals(card.getNetPower(), power); Assert.assertEquals(card.getNetToughness(), toughness);
    }
    @Test public void donorAnimationIsExcludedWhileReceiverModifierPersists() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card donor = seed(game, player, "Spellwake Copy Donor", "Creature Elf", "2/3");
        Card receiver = seed(game, player, "Spellwake Copy Receiver", "Creature", "1/1");
        Card host = addCardToZone("Giant Growth", player, ZoneType.Hand);
        apply(game, player, host, donor, "DB$ Animate | ValidTgts$ Creature | Power$ 9 | Toughness$ 9 | Types$ Vampire | RemoveCreatureTypes$ True");
        apply(game, player, host, receiver, "DB$ Pump | ValidTgts$ Creature | NumAtt$ 1 | NumDef$ 2");
        stats("donor.animated", donor, 9, 9);
        copy(game, player, receiver, donor, "");
        stats("receiver.copy", receiver, 3, 5);
        Assert.assertTrue(receiver.getType().hasCreatureType("Elf"));
        Assert.assertFalse(receiver.getType().hasCreatureType("Vampire"));
    }
    @Test public void copiesCapturePreviousCopiesWithoutFollowingLaterChanges() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card donor = seed(game, player, "Spellwake First", "Creature Elf", "2/3");
        Card other = seed(game, player, "Spellwake Other", "Creature Vampire", "6/7");
        Card first = seed(game, player, "Spellwake Copier A", "Artifact", null);
        Card second = seed(game, player, "Spellwake Copier B", "Artifact", null);
        copy(game, player, first, donor, ""); copy(game, player, second, first, "");
        copy(game, player, first, other, "");
        stats("fixed.first", first, 6, 7); stats("fixed.second", second, 2, 3);
        Assert.assertTrue(second.getType().hasCreatureType("Elf"));
        copy(game, player, second, second, ""); stats("fixed.self", second, 2, 3);
    }
    @Test public void copiedDefinitionCountsTheReceiversLands() {
        Game game = initAndCreateGame(); Player donorPlayer = game.getPlayers().get(0), receiverPlayer = game.getPlayers().get(1);
        addCard("Forest", donorPlayer); addCard("Island", donorPlayer); addCard("Mountain", receiverPlayer);
        Card donor = seed(game, donorPlayer, "Spellwake Land Count", "Creature", "*/*",
            "S:Mode$ Continuous | CharacteristicDefining$ True | SetPower$ X | SetToughness$ X", "SVar:X:Count$Valid Land.YouCtrl");
        Card receiver = seed(game, receiverPlayer, "Spellwake Counter Copy", "Artifact", null);
        game.getAction().checkStaticAbilities(); stats("count.donor", donor, 2, 2);
        copy(game, receiverPlayer, receiver, donor, ""); stats("count.receiver", receiver, 1, 1);
        Assert.assertEquals(receiver.getBasePowerString(), "*");
    }
    @Test public void ptExceptionDropsTheWholeMultipartDefiningAbility() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        addCard("Forest", player); addCard("Island", player);
        Card donor = seed(game, player, "Spellwake Multipart", "Creature", "*/*",
            "S:Mode$ Continuous | CharacteristicDefining$ True | AddAllCreatureTypes$ True | SetPower$ X | SetToughness$ X", "SVar:X:Count$Valid Land.YouCtrl");
        Card receiver = seed(game, player, "Spellwake Exception", "Artifact", null);
        copy(game, player, receiver, donor, " | SetPower$ 7 | SetToughness$ 7");
        stats("exception.receiver", receiver, 7, 7);
        Assert.assertFalse(receiver.getType().hasCreatureType("Elf"));
        Card next = seed(game, player, "Spellwake Next", "Artifact", null);
        copy(game, player, next, receiver, ""); stats("exception.next", next, 7, 7);
        Assert.assertFalse(next.getType().hasCreatureType("Elf"));
    }
    @Test public void acquiredStaticUsesTheCopyTimestamp() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card receiver = seed(game, player, "Spellwake Old Receiver", "Artifact", null);
        seed(game, player, "Spellwake Red Field", "Artifact", null,
            "S:Mode$ Continuous | Affected$ Creature | SetColor$ Red");
        Card donor = seed(game, player, "Spellwake Blue Self", "Creature", "2/3",
            "S:Mode$ Continuous | Affected$ Card.YouCtrl | SetColor$ Blue");
        copy(game, player, receiver, donor, "");
        game.getAction().moveToHand(donor, null); game.getAction().checkStaticAbilities();
        stats("timestamp.receiver", receiver, 2, 3);
        System.out.println("SPELLWAKE_PERMANENT_COPY {\"checkpoint\":\"timestamp.color\",\"color_bits\":"
            + receiver.getColor().getColor() + ",\"receiver_layer_timestamp\":" + receiver.getLayerTimestamp()
            + ",\"static_timestamp\":" + receiver.getStaticAbilities().get(0).getTimestamp() + "}");
        Assert.assertEquals(receiver.getColor().getColor(), MagicColor.BLUE);
    }
}
