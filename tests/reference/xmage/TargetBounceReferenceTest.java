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
public class TargetBounceReferenceTest extends CardTestPlayerBase {
    private final List<String> observations = new ArrayList<>();
    private static final PhaseStep MAIN = PhaseStep.PRECOMBAT_MAIN;

    @Test
    public void boltTargetLeavesBeforeResolution() throws IOException {
        prepare(true);
        castSpell(1, MAIN, playerA, "@bolt", "@lion");
        observe("bolt-on-stack", playerA, Arrays.asList("bolt"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerA);
        castSpell(1, MAIN, playerB, "@bounce", "@lion", "Lightning Bolt");
        observe("response-on-stack", playerB, Arrays.asList("bolt", "bounce"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerB, true);
        observe("after-response", playerB, Arrays.asList("bolt"), "HAND");
        waitStackResolved(1, MAIN, playerB);
        observe("after-bolt", playerB, Arrays.asList(), "HAND");
        finish("target-bounce");
    }

    @Test
    public void boltAloneKillsLion() throws IOException {
        prepare(false);
        castSpell(1, MAIN, playerA, "@bolt", "@lion");
        observe("bolt-on-stack", playerA, Arrays.asList("bolt"), "BATTLEFIELD");
        waitStackResolved(1, MAIN, playerA);
        observe("after-bolt", playerA, Arrays.asList(), "GRAVEYARD");
        finish("bolt-control");
    }

    private void prepare(boolean bounce) {
        setStrictChooseMode(true);
        addCard(Zone.BATTLEFIELD, playerA, "M11-Mountain");
        addCard(Zone.BATTLEFIELD, playerB, "M11-Island");
        addCard(Zone.BATTLEFIELD, playerB, "M11-Silvercoat Lion@lion");
        addCard(Zone.HAND, playerA, "M11-Lightning Bolt@bolt");
        if (bounce) addCard(Zone.HAND, playerB, "M11-Unsummon@bounce");
    }

    private void observe(String stage, TestPlayer actor, List<String> expectedStack, String lionZone) {
        runCode(stage, 1, MAIN, actor, (info, player, game) -> {
            List<String> actualStack = new ArrayList<>();
            List<String> entries = new ArrayList<>();
            java.util.Iterator<StackObject> stack = game.getStack().descendingIterator();
            while (stack.hasNext()) {
                StackObject item = stack.next();
                String alias = alias(item.getSourceId());
                actualStack.add(alias);
                List<String> targets = new ArrayList<>();
                item.getStackAbility().getTargets().forEach(target -> target.getTargets()
                        .forEach(id -> targets.add(quote(alias(id)))));
                assertEquals(Arrays.asList(quote("lion")), targets);
                entries.add("{\"card\":" + quote(alias) + ",\"targets\":[" + String.join(",", targets) + "]}");
            }
            assertEquals(stage, expectedStack, actualStack);
            assertEquals(stage, lionZone, game.getState().getZone(playerA.getAliasByName("lion")).name());
            assertEquals(20, game.getPlayer(playerA.getId()).getLife());
            assertEquals(20, game.getPlayer(playerB.getId()).getLife());
            List<String> cards = new ArrayList<>();
            for (String name : Arrays.asList("bolt", "bounce", "lion")) {
                UUID id = playerA.getAliasByName(name);
                if (id == null) continue;
                Card card = game.getCard(id);
                Permanent permanent = game.getPermanent(id);
                String zone = game.getState().getZone(id).name();
                if (!name.equals("lion")) {
                    String expectedZone = expectedStack.contains(name) ? "STACK"
                            : (name.equals("bounce") && stage.equals("bolt-on-stack") ? "HAND" : "GRAVEYARD");
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

    private String alias(UUID id) {
        for (String name : Arrays.asList("bolt", "bounce", "lion")) {
            if (id.equals(playerA.getAliasByName(name))) return name;
        }
        throw new AssertionError("Unrecognized fixture identity");
    }

    private static String quote(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private void finish(String scenario) throws IOException {
        setStopAt(1, PhaseStep.BEGIN_COMBAT);
        execute();
        int expected = scenario.equals("target-bounce") ? 4 : 2;
        assertEquals(expected, observations.size());
        String output = "{\"schema_version\":1,\"scenario\":" + quote(scenario)
                + ",\"reference\":\"xmage\",\"commit\":\"81c2f6d806dbec8eb79ad2a511cb686dff8361e7\",\"strict_choices\":true,\"mana_auto_payment\":true,\"observations\":["
                + String.join(",", observations) + "]}\n";
        Files.write(Paths.get(System.getProperty("spellwake.evidence.dir"), scenario + ".json"), output.getBytes(StandardCharsets.UTF_8));
    }
}
