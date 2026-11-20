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
import java.util.HexFormat;
import java.security.MessageDigest;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Focused control-handler comparison. Prepared main phase, direct ability
 * resolution and explicit EOT callbacks are trusted test boundaries. No casting,
 * priority-loop, whole-game, AI-policy or complete-card equivalence is claimed.
 */
public class SpellwakeControlEffectsReferenceTest extends AITest {
    private void runtimeInfo() throws Exception {
        for (Class<?> type : List.of(Card.class, forge.game.ability.effects.ControlGainEffect.class)) {
            byte[] bytes;
            try (var stream = type.getResourceAsStream("/" + type.getName().replace('.', '/') + ".class")) {
                if (stream == null) throw new AssertionError("Missing loaded class bytes");
                bytes = stream.readAllBytes();
            }
            String sha = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
            System.out.println("FORGE_CONTROL_RUNTIME " + type.getName() + " " + sha + " " + type.getProtectionDomain().getCodeSource().getLocation());
        }
    }
    private Card seed(Game game, Player owner, Player controller, boolean creature) {
        List<String> lines = creature
            ? List.of("Name:Spellwake Control Target", "ManaCost:0", "Types:Creature", "PT:4/4", "Oracle:")
            : List.of("Name:Spellwake Control Source", "ManaCost:0", "Types:Instant", "Oracle:");
        PaperCard paper = new PaperCard(new CardRules.Reader().readCard(lines), "???", CardRarity.Common);
        Card card = Card.fromPaperCard(paper, owner);
        card.setGameTimestamp(game.getNextTimestamp());
        card.setController(controller, game.getNextTimestamp());
        controller.getZone(creature ? ZoneType.Battlefield : ZoneType.Hand).add(card);
        return card;
    }

    private void gain(Game game, Player actor, Card target, boolean untilEndOfTurn) {
        Card source = seed(game, actor, actor, false);
        String script = "DB$ GainControl | ValidTgts$ Creature" + (untilEndOfTurn ? " | LoseControl$ EOT" : "");
        SpellAbility ability = AbilityFactory.getAbility(script, source);
        ability.setActivatingPlayer(actor);
        ability.getTargets().add(target);
        AbilityUtils.resolve(ability);
    }

    private void row(String checkpoint, Card card, Player a, Player b) {
        String owner = card.getOwner() == a ? "A" : "B";
        String controller = card.getController() == a ? "A" : card.getController() == b ? "B" : "UNKNOWN";
        System.out.println("SPELLWAKE_CONTROL {\"checkpoint\":\"" + checkpoint + "\",\"owner\":\"" + owner + "\",\"controller\":\"" + controller + "\"}");
    }

    @Test public void expiryRestoresTheDefaultControllerInsteadOfTheOwner() throws Exception {
        runtimeInfo();
        Game game = initAndCreateGame();
        Player a = game.getPlayers().get(1), b = game.getPlayers().get(0);
        Card target = seed(game, a, b, true);
        row("default-diff.before", target, a, b);
        Assert.assertSame(target.getOwner(), a);
        Assert.assertSame(target.getController(), b);
        gain(game, a, target, true);
        row("default-diff.controlled", target, a, b);
        Assert.assertSame(target.getController(), a);
        game.getEndOfTurn().executeUntil();
        row("default-diff.expired", target, a, b);
        Assert.assertSame(target.getController(), b);
        Assert.assertSame(target.getOwner(), a);
    }

    @Test public void expiryRevealsAnOlderControlEffect() throws Exception {
        runtimeInfo();
        Game game = initAndCreateGame();
        Player a = game.getPlayers().get(1), b = game.getPlayers().get(0);
        Card target = seed(game, a, a, true);
        gain(game, b, target, false);
        row("older.first", target, a, b);
        Assert.assertSame(target.getController(), b);
        gain(game, a, target, true);
        row("older.latest", target, a, b);
        Assert.assertSame(target.getController(), a);
        game.getEndOfTurn().executeUntil();
        row("older.expired", target, a, b);
        Assert.assertSame(target.getController(), b);
        Assert.assertSame(target.getOwner(), a);
    }
}
