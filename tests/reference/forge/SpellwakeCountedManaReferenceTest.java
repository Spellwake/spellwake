package forge.ai;

import forge.card.CardRarity;
import forge.card.CardRules;
import forge.card.MagicColor;
import forge.game.Game;
import forge.game.ability.AbilityUtils;
import forge.game.card.Card;
import forge.game.player.Player;
import forge.game.spellability.SpellAbility;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import java.util.Collection;
import java.util.List;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Direct mana-body and New Blood resolution in prepared state. Mana tap/payment,
 * spell casting, priority and whole-game equivalence are not compared.
 */
public class SpellwakeCountedManaReferenceTest extends AITest {
    private Card peer(Game game, Player player, String type) {
        Card card = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of(
            "Name:Spellwake " + type + " Peer", "ManaCost:0", "Types:Creature " + type, "PT:2/2", "Oracle:")), "???", CardRarity.Common), player);
        card.setGameTimestamp(game.getNextTimestamp());
        player.getZone(ZoneType.Battlefield).add(card);
        return card;
    }
    private void mana(Card source, Player actor) {
        Assert.assertEquals(source.getManaAbilities().size(), 1);
        SpellAbility ability = source.getManaAbilities().get(0);
        Assert.assertTrue(ability.isManaAbility());
        ability.setActivatingPlayer(actor);
        AbilityUtils.resolve(ability);
    }
    @Test public void countsTheSourceAndOnlyItsControllersBattlefieldElves() {
        Game game = initAndCreateGame();
        Player actor = game.getPlayers().get(1), other = game.getPlayers().get(0);
        Card source = addCard("Elvish Archdruid", actor);
        peer(game, actor, "Elf");
        peer(game, actor, "Goblin");
        peer(game, other, "Elf");
        addCardToZone("Llanowar Elves", actor, ZoneType.Hand);
        game.getAction().checkStaticAbilities();
        Assert.assertTrue(actor.getManaPool().isEmpty());
        mana(source, actor);
        int green = actor.getManaPool().getAmountOfColor(MagicColor.GREEN);
        Assert.assertEquals(green, 2);
        Assert.assertTrue(other.getManaPool().isEmpty());
        System.out.println("SPELLWAKE_COUNTED {\"checkpoint\":\"ordinary\",\"green\":" + green + "}");
    }
    @Test public void newBloodChangesBothPrintedBoostAndManaCount() {
        Game game = initAndCreateGame();
        Player actor = game.getPlayers().get(1), other = game.getPlayers().get(0);
        final int[] choices = {0};
        actor.addController(game.getNextTimestamp(), actor, new PlayerControllerAi(game, actor, actor.getLobbyPlayer()) {
            @Override public String chooseSomeType(String kind, SpellAbility ability, Collection<String> valid, boolean optional) {
                Assert.assertEquals(kind, "Creature");
                Assert.assertTrue(valid.contains("Elf"));
                Assert.assertEquals(++choices[0], 1);
                return "Elf";
            }
        }, false);
        Card source = addCard("Elvish Archdruid", actor);
        Card elf = peer(game, actor, "Elf");
        Card vampire = peer(game, actor, "Vampire");
        Card vampire2 = peer(game, actor, "Vampire");
        Card opposing = peer(game, other, "Vampire");
        Card spell = addCardToZone("New Blood", actor, ZoneType.Hand);
        game.getAction().checkStaticAbilities();
        Assert.assertEquals(elf.getNetPower(), 3);
        Assert.assertEquals(vampire.getNetPower(), 2);
        SpellAbility ability = spell.getSpellAbilities().get(0);
        ability.setActivatingPlayer(actor);
        ability.getTargets().add(source);
        AbilityUtils.resolve(ability);
        game.getAction().checkStaticAbilities();
        Assert.assertEquals(choices[0], 1);
        Assert.assertSame(source.getController(), actor);
        Assert.assertTrue(source.getType().hasSubtype("Vampire"));
        Assert.assertTrue(source.getType().hasSubtype("Druid"));
        Assert.assertFalse(source.getType().hasSubtype("Elf"));
        Assert.assertEquals(elf.getNetPower(), 2);
        Assert.assertEquals(vampire.getNetPower(), 3);
        Assert.assertEquals(vampire2.getNetPower(), 3);
        Assert.assertEquals(opposing.getNetPower(), 2);
        mana(source, actor);
        int green = actor.getManaPool().getAmountOfColor(MagicColor.GREEN);
        Assert.assertEquals(green, 3);
        Assert.assertTrue(other.getManaPool().isEmpty());
        System.out.println("SPELLWAKE_COUNTED {\"checkpoint\":\"text\",\"green\":" + green + ",\"elf_power\":" + elf.getNetPower()
            + ",\"vampire_powers\":[" + vampire.getNetPower() + "," + vampire2.getNetPower() + "," + opposing.getNetPower() + "]}");
    }
    @Test public void aCurrentlyZeroCountStillIsAManaAbilityAndAddsNoMana() {
        Game game = initAndCreateGame();
        Player actor = game.getPlayers().get(1);
        Card source = Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of(
            "Name:Spellwake Zero Counter", "ManaCost:0", "Types:Artifact", "Oracle:",
            "A:AB$ Mana | Cost$ T | Produced$ G | Amount$ X", "SVar:X:Count$Valid Elf.YouCtrl")), "???", CardRarity.Common), actor);
        source.setGameTimestamp(game.getNextTimestamp());
        actor.getZone(ZoneType.Battlefield).add(source);
        game.getAction().checkStaticAbilities();
        mana(source, actor);
        Assert.assertTrue(actor.getManaPool().isEmpty());
        System.out.println("SPELLWAKE_COUNTED {\"checkpoint\":\"zero\",\"green\":0}");
    }
}
