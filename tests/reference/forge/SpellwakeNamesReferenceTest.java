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

/** Prepared name properties, direct name-state and Clone/ChangeText boundaries.
 * No normal casting/payment/priority, full aliases or complete cards compared.
 */
public class SpellwakeNamesReferenceTest extends AITest {
    private Card seed(Game game, Player player, String name) {
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of(
            "Name:" + name, "ManaCost:0", "Types:Creature Elf", "PT:2/3", "Oracle:")), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp()); player.getZone(ZoneType.Battlefield).add(card); return card;
    }
    private void apply(Game game, Player player, Card host, Card target, String script) {
        SpellAbility ability = AbilityFactory.getAbility(script, host); ability.setActivatingPlayer(player);
        ability.getTargets().add(target); AbilityUtils.resolve(ability); game.getAction().checkStaticAbilities();
    }
    private void names(String point, Card a, Card b) {
        System.out.println("SPELLWAKE_NAMES {\"checkpoint\":\"" + point + "\",\"a\":\"" + a.getName()
            + "\",\"b\":\"" + b.getName() + "\",\"forward\":" + a.sharesNameWith(b)
            + ",\"reverse\":" + b.sharesNameWith(a) + "}");
    }
    @Test public void literalAndNamelessObjectsHaveDistinctMatchingRules() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card a = seed(game, player, "Spellwake Name A"), b = seed(game, player, "Spellwake Name A");
        names("named.same", a, b); Assert.assertTrue(a.sharesNameWith(b));
        a.addChangedName("", false, game.getNextTimestamp(), 0);
        b.addChangedName("", false, game.getNextTimestamp(), 0);
        names("nameless.same", a, b); Assert.assertTrue(a.hasNoName()); Assert.assertFalse(a.sharesNameWith(b));
    }
    @Test public void temporaryOverwriteRestoresAndTypeWordDoesNotRenameProperName() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card a = seed(game, player, "Elf"), b = seed(game, player, "Vampire");
        long timestamp = game.getNextTimestamp(); a.addChangedName("Vampire", false, timestamp, 0);
        names("overwrite.active", a, b); Assert.assertTrue(a.sharesNameWith(b));
        a.removeChangedName(timestamp, 0); Assert.assertEquals(a.getName(), "Elf");
        Card host = addCardToZone("New Blood", player, ZoneType.Hand);
        apply(game, player, host, a, "DB$ ChangeText | ValidTgts$ Creature | ChangeTypeWord$ Elf Vampire | Duration$ Permanent");
        names("type.protected", a, b); Assert.assertEquals(a.getName(), "Elf");
        Assert.assertTrue(a.getType().hasCreatureType("Vampire")); Assert.assertFalse(a.sharesNameWith(b));
        Card protectedName = seed(game, player, "SP//dr, Piloted by Peni");
        Assert.assertEquals(protectedName.getName(), "SP//dr, Piloted by Peni");
    }
    @Test public void copyingCapturesOriginalNameAndExistingCopyWithoutTemporaryOverwrite() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card donor = seed(game, player, "Spellwake Original"), receiver = seed(game, player, "Spellwake Receiver");
        donor.addChangedName("Spellwake Temporary", false, game.getNextTimestamp(), 0);
        apply(game, player, receiver, donor, "DB$ Clone | ValidTgts$ Permanent");
        names("copy.original", receiver, donor); Assert.assertEquals(receiver.getName(), "Spellwake Original");
        Card second = seed(game, player, "Spellwake Second");
        apply(game, player, second, receiver, "DB$ Clone | ValidTgts$ Permanent");
        receiver.addChangedName("Spellwake Later", false, game.getNextTimestamp(), 0);
        names("copy.fixed", second, receiver); Assert.assertEquals(second.getName(), "Spellwake Original");
    }
    @Test public void splitNameOverlapIsSymmetricAtThePreparedHandBoundary() {
        Game game = initAndCreateGame(); Player player = game.getPlayers().get(1);
        Card split = addCardToZone("Fire // Ice", player, ZoneType.Hand);
        Card single = seed(game, player, "Fire");
        names("split.overlap", split, single);
        Assert.assertTrue(split.sharesNameWith("Fire")); Assert.assertTrue(split.sharesNameWith("Ice"));
        Assert.assertTrue(split.sharesNameWith(single)); Assert.assertTrue(single.sharesNameWith(split));
    }
    private Card staticHost(Game game, Player player, String name, String script) {
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of(
            "Name:" + name, "ManaCost:0", "Types:Artifact", "Oracle:", "S:" + script)), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp()); player.getZone(ZoneType.Battlefield).add(card); return card;
    }
    @Test public void actualStaticNameChangeFeedsLaterNameQualifiedPower() {
        Game game=initAndCreateGame(); Player player=game.getPlayers().get(1);
        staticHost(game,player,"Spellwake Name Booster","Mode$ Continuous | Affected$ Creature.namedSpellwake New | AddPower$ 1 | AddToughness$ 1");
        staticHost(game,player,"Spellwake Renamer","Mode$ Continuous | Affected$ Creature.namedSpellwake Old | SetName$ Spellwake New");
        Card subject=seed(game,player,"Spellwake Old");
        game.getAction().checkStaticAbilities();
        System.out.println("SPELLWAKE_STATIC_NAME name="+subject.getName()+" power="+subject.getNetPower()+" toughness="+subject.getNetToughness());
        Assert.assertEquals(subject.getName(),"Spellwake New");
        Assert.assertEquals(subject.getNetPower(),3); Assert.assertEquals(subject.getNetToughness(),4);
    }

}
