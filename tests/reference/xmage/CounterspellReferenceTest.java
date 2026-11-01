package org.mage.test.reference;

import mage.cards.Card;
import mage.constants.PhaseStep;
import mage.constants.Zone;
import mage.game.Game;
import mage.game.permanent.Permanent;
import mage.game.stack.StackObject;
import org.junit.Test;
import org.mage.test.player.TestPlayer;
import org.mage.test.serverside.base.CardTestPlayerBase;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.UUID;

import static org.junit.Assert.*;

/** Original Spellwake research fixture; upstream engine implementation is not copied. */
public class CounterspellReferenceTest extends CardTestPlayerBase {
    private final List<String> observations = new ArrayList<>();
    private static final PhaseStep MAIN = PhaseStep.PRECOMBAT_MAIN;

    @Test
    public void counterBolt() throws IOException {
        prepare(false);
        castSpell(1, MAIN, playerA, "@bolt", "@lion");
        observe("bolt-on-stack", playerA, Arrays.asList("bolt"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerA);
        castSpell(1, MAIN, playerB, "@counter", "Lightning Bolt", "Lightning Bolt");
        observe("counter-on-stack", playerB, Arrays.asList("bolt", "counter"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerB);
        observe("after-counter", playerB, Arrays.asList(), "BATTLEFIELD");
        finish("counter-bolt");
    }

    @Test
    public void counterCounter() throws IOException {
        prepare(true);
        castSpell(1, MAIN, playerA, "@bolt", "@lion");
        observe("bolt-on-stack", playerA, Arrays.asList("bolt"), "BATTLEFIELD");
        castSpell(1, MAIN, playerA, "@rescue", "Counterspell", "Counterspell");
        observe("rescue-on-stack", playerA, Arrays.asList("bolt", "counter", "rescue"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-rescue", playerA, Arrays.asList("bolt"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerA);
        observe("after-bolt", playerA, Arrays.asList(), "GRAVEYARD");
        castSpell(1, MAIN, playerB, "@counter", "Lightning Bolt", "Lightning Bolt");
        observe("counter-on-stack", playerB, Arrays.asList("bolt", "counter"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerB);
        finish("counter-counter");
    }

    private void prepare(boolean rescue) {
        setStrictChooseMode(true);
        addCard(Zone.BATTLEFIELD, playerA, "M11-Mountain");
        if (rescue) addCard(Zone.BATTLEFIELD, playerA, "M11-Island", 2);
        addCard(Zone.BATTLEFIELD, playerB, "M11-Island", 2);
        addCard(Zone.BATTLEFIELD, playerB, "M11-Silvercoat Lion@lion");
        addCard(Zone.HAND, playerA, "M11-Lightning Bolt@bolt");
        addCard(Zone.HAND, playerB, "7ED-Counterspell@counter");
        if (rescue) addCard(Zone.HAND, playerA, "7ED-Counterspell@rescue");
    }

    private void observe(String stage, TestPlayer actor, List<String> expectedStack, String lionZone) {
        runCode(stage, 1, MAIN, actor, (info, player, game) -> {
            List<String> actualStack = new ArrayList<>();
            List<String> entries = new ArrayList<>();
            java.util.Iterator<StackObject> stack = game.getStack().descendingIterator();
            while (stack.hasNext()) {
                StackObject item = stack.next();
                String alias = alias(item.getSourceId(), game);
                actualStack.add(alias);
                List<String> targets = new ArrayList<>();
                item.getStackAbility().getTargets().forEach(target -> target.getTargets()
                        .forEach(id -> targets.add(quote(alias(id, game)))));
                String expectedTarget = alias.equals("bolt") ? "lion" : (alias.equals("counter") ? "bolt" : "counter");
                assertEquals(Arrays.asList(quote(expectedTarget)), targets);
                entries.add("{\"card\":" + quote(alias) + ",\"targets\":[" + String.join(",", targets) + "]}");
            }
            assertEquals(stage, expectedStack, actualStack);
            assertEquals(stage, lionZone, game.getState().getZone(playerA.getAliasByName("lion")).name());
            assertEquals(20, game.getPlayer(playerA.getId()).getLife());
            assertEquals(20, game.getPlayer(playerB.getId()).getLife());
            List<String> cards = new ArrayList<>();
            for (String name : Arrays.asList("bolt", "counter", "rescue", "lion")) {
                UUID id = playerA.getAliasByName(name);
                if (id == null) continue;
                Card card = game.getCard(id);
                Permanent permanent = game.getPermanent(id);
                String zone = game.getState().getZone(id).name();
                if (!name.equals("lion")) {
                    String expectedZone = expectedStack.contains(name) ? "STACK"
                            : ((name.equals("counter") && stage.equals("bolt-on-stack"))
                            || (name.equals("rescue") && (stage.equals("bolt-on-stack") || stage.equals("counter-on-stack")))
                            ? "HAND" : "GRAVEYARD");
                    assertEquals(stage + ": " + name, expectedZone, zone);
                }
                String damage = permanent == null ? "null" : Integer.toString(permanent.getDamage());
                if (permanent != null) {
                    assertEquals(0, permanent.getDamage());
                    assertEquals(2, permanent.getPower().getValue());
                    assertEquals(2, permanent.getToughness().getValue());
                }
                cards.add("{\"alias\":" + quote(name) + ",\"owner\":" + quote(card.getOwnerId().equals(playerA.getId()) ? "A" : "B")
                        + ",\"zone\":" + quote(zone) + ",\"damage\":" + damage
                        + ",\"power\":" + (permanent == null ? "null" : permanent.getPower().getValue())
                        + ",\"toughness\":" + (permanent == null ? "null" : permanent.getToughness().getValue())
                        + ",\"class\":" + quote(card.getClass().getName()) + ",\"set\":" + quote(card.getExpansionSetCode())
                        + ",\"number\":" + quote(card.getCardNumber()) + "}");
            }
            observations.add("{\"checkpoint\":" + quote(stage) + ",\"stack_bottom_first\":[" + String.join(",", entries)
                    + "],\"life\":{\"A\":20,\"B\":20},\"cards\":[" + String.join(",", cards) + "]}");
        });
    }

    private String alias(UUID id, Game game) {
        StackObject targetSpell = game.getStack().getStackObject(id);
        UUID sourceId = targetSpell == null ? id : targetSpell.getSourceId();
        for (String name : Arrays.asList("bolt", "counter", "rescue", "lion")) {
            if (sourceId.equals(playerA.getAliasByName(name))) return name;
        }
        throw new AssertionError("Unrecognized fixture identity");
    }

    private static String quote(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private void finish(String scenario) throws IOException {
        setStopAt(1, PhaseStep.BEGIN_COMBAT);
        execute();
        int expected = scenario.equals("counter-counter") ? 5 : 3;
        assertEquals(expected, observations.size());
        String output = "{\"schema_version\":1,\"scenario\":" + quote(scenario)
                + ",\"reference\":\"xmage\",\"commit\":\"81c2f6d806dbec8eb79ad2a511cb686dff8361e7\",\"strict_choices\":true,\"mana_auto_payment\":true,\"observations\":["
                + String.join(",", observations) + "]}\n";
        Files.write(Paths.get(System.getProperty("spellwake.evidence.dir"), scenario + ".json"), output.getBytes(StandardCharsets.UTF_8));
    }
}
