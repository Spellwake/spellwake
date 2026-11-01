package forge.ai;

import java.util.Collection;

import forge.game.Game;
import forge.game.ability.AbilityUtils;
import forge.game.card.Card;
import forge.game.player.Player;
import forge.game.spellability.SpellAbility;
import forge.game.zone.ZoneType;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Original research fixture: resolves the real card program without casting or paying costs. */
public class NewBloodResolutionResearchTest extends AITest {
    @Test
    public void newBloodResolutionChangesExecutableElfFilter() {
        final Game game = initAndCreateGame();
        final Player actor = game.getPlayers().get(1);
        final Player opponent = game.getPlayers().get(0);
        final int[] creatureTypeChoices = {0};
        actor.addController(game.getNextTimestamp(), actor,
                new PlayerControllerAi(game, actor, actor.getLobbyPlayer()) {
            @Override
            public String chooseSomeType(String kind, SpellAbility ability,
                    Collection<String> validTypes, boolean optional) {
                Assert.assertEquals(kind, "Creature", "unexpected choice category");
                Assert.assertTrue(validTypes.contains("Elf"), "Elf must be a legal explicit choice");
                Assert.assertEquals(++creatureTypeChoices[0], 1, "unexpected extra type prompt");
                System.out.println("RESEARCH choice creature-type=Elf replacement=Vampire");
                return "Elf";
            }
        }, false);
        final Card archdruid = addCard("Elvish Archdruid", opponent);
        final Card opposingElf = addCard("Llanowar Elves", opponent);
        final Card ownElf = addCard("Llanowar Elves", actor);
        final Card vampire = addCard("Vampire Interloper", actor);
        final Card newBlood = addCardToZone("New Blood", actor, ZoneType.Hand);
        game.getAction().checkStaticAbilities();
        Assert.assertEquals(opposingElf.getNetPower(), 2);
        Assert.assertEquals(ownElf.getNetPower(), 1);
        Assert.assertEquals(vampire.getNetPower(), 2);
        System.out.println("RESEARCH before opposingElf=2 ownElf=1 vampire=2 controller=opponent");

        final SpellAbility ability = newBlood.getSpellAbilities().get(0);
        ability.setActivatingPlayer(actor);
        ability.getTargets().add(archdruid);
        Assert.assertTrue(ability.canTarget(archdruid));
        AbilityUtils.resolve(ability);
        game.getAction().checkStaticAbilities();

        Assert.assertEquals(creatureTypeChoices[0], 1);
        Assert.assertEquals(archdruid.getController(), actor);
        Assert.assertEquals(archdruid.getName(), "Elvish Archdruid");
        Assert.assertTrue(archdruid.getType().hasSubtype("Vampire"));
        Assert.assertFalse(archdruid.getType().hasSubtype("Elf"));
        Assert.assertEquals(opposingElf.getNetPower(), 1);
        Assert.assertEquals(ownElf.getNetPower(), 1);
        Assert.assertEquals(vampire.getNetPower(), 3);
        Assert.assertEquals(vampire.getNetToughness(), 2);
        System.out.println("RESEARCH after opposingElf=1 ownElf=1 vampire=3/2 controller=actor name=Elvish Archdruid subtype=Vampire");
    }
}
