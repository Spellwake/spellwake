package org.mage.test.reference;

import mage.cards.Card;
import mage.constants.PhaseStep;
import mage.constants.Zone;
import mage.game.Game;
import mage.game.permanent.Permanent;
import mage.game.stack.Spell;
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

/** Original Spellwake adapter: real casting, Twincast copying, and explicit scripted decisions. */
public class SpellCopyReferenceTest extends CardTestPlayerBase {
    private static final PhaseStep MAIN = PhaseStep.PRECOMBAT_MAIN;
    private static final List<String> CARDS = Arrays.asList("bolt", "twincast", "counter", "bounce", "lion1", "lion2");
    private final List<String> observations = new ArrayList<>();
    private String copyTarget;
    private String counterTarget;
    private UUID copySourceId;
    private UUID copyStackId;

    @Test
    public void copyRedirect() throws IOException {
        prepare("lion2", null);
        startOriginal();
        waitStackResolved(1, MAIN, playerA);
        startTwincast();
        waitStackResolved(1, MAIN, playerB, true);
        observe("after-copy", playerB, "bolt,bolt-copy", "BATTLEFIELD", "BATTLEFIELD", "twincast");
        waitStackResolved(1, MAIN, playerB, true);
        observe("after-copy-resolution", playerB, "bolt", "BATTLEFIELD", "GRAVEYARD", "twincast");
        waitStackResolved(1, MAIN, playerB);
        observe("finished", playerB, "", "GRAVEYARD", "GRAVEYARD", "bolt,twincast");
        finish("copy-redirect", 5);
    }

    @Test
    public void counterOriginal() throws IOException {
        counterCase(false);
    }

    @Test
    public void counterCopy() throws IOException {
        counterCase(true);
    }

    private void counterCase(boolean counterCopy) throws IOException {
        prepare("lion2", counterCopy ? "bolt-copy" : "bolt");
        startOriginal();
        waitStackResolved(1, MAIN, playerA, true);
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-copy", playerA, "bolt,bolt-copy", "BATTLEFIELD", "BATTLEFIELD", "twincast");
        castSpell(1, MAIN, playerA, "@counter", counterCopy ? "Lightning Bolt[only copy]" : "Lightning Bolt[no copy]",
                "Lightning Bolt", StackClause.WHILE_COPY_ON_STACK);
        observe("counter-on-stack", playerA, "bolt,bolt-copy,counter", "BATTLEFIELD", "BATTLEFIELD", "twincast");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-counter", playerA, counterCopy ? "bolt" : "bolt-copy", "BATTLEFIELD", "BATTLEFIELD",
                counterCopy ? "twincast,counter" : "bolt,twincast,counter");
        waitStackResolved(1, MAIN, playerA);
        observe("finished", playerA, "", counterCopy ? "GRAVEYARD" : "BATTLEFIELD",
                counterCopy ? "BATTLEFIELD" : "GRAVEYARD", "bolt,twincast,counter");
        startTwincast();
        waitStackResolved(1, MAIN, playerB);
        finish(counterCopy ? "counter-copy" : "counter-original", 6);
    }

    @Test
    public void copyRetainMissingTarget() throws IOException {
        missingTargetCase(false);
    }

    @Test
    public void copyRetargetMissingTarget() throws IOException {
        missingTargetCase(true);
    }

    private void missingTargetCase(boolean retarget) throws IOException {
        prepare(retarget ? "lion2" : "lion1", null);
        startOriginal();
        castSpell(1, MAIN, playerA, "@bounce", "@lion1", "Twincast");
        observe("bounce-on-stack", playerA, "bolt,twincast,bounce", "BATTLEFIELD", "BATTLEFIELD", "");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-bounce", playerA, "bolt,twincast", "HAND", "BATTLEFIELD", "bounce");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-copy", playerA, "bolt,bolt-copy", "HAND", "BATTLEFIELD", "twincast,bounce");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-copy-resolution", playerA, "bolt", "HAND", retarget ? "GRAVEYARD" : "BATTLEFIELD", "twincast,bounce");
        waitStackResolved(1, MAIN, playerA);
        observe("finished", playerA, "", "HAND", retarget ? "GRAVEYARD" : "BATTLEFIELD", "bolt,twincast,bounce");
        startTwincast();
        waitStackResolved(1, MAIN, playerB);
        finish(retarget ? "copy-retarget-missing-target" : "copy-retain-missing-target", 7);
    }

    @Test
    public void copyTargetDeparts() throws IOException {
        prepare("lion1", null);
        startOriginal();
        waitStackResolved(1, MAIN, playerA, true);
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-copy", playerA, "bolt,bolt-copy", "BATTLEFIELD", "BATTLEFIELD", "twincast");
        castSpell(1, MAIN, playerA, "@bounce", "@lion1", "Lightning Bolt", StackClause.WHILE_COPY_ON_STACK);
        observe("bounce-on-stack", playerA, "bolt,bolt-copy,bounce", "BATTLEFIELD", "BATTLEFIELD", "twincast");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-bounce", playerA, "bolt,bolt-copy", "HAND", "BATTLEFIELD", "twincast,bounce");
        waitStackResolved(1, MAIN, playerA, true);
        observe("after-copy-resolution", playerA, "bolt", "HAND", "BATTLEFIELD", "twincast,bounce");
        waitStackResolved(1, MAIN, playerA);
        observe("finished", playerA, "", "HAND", "BATTLEFIELD", "bolt,twincast,bounce");
        startTwincast();
        waitStackResolved(1, MAIN, playerB);
        finish("copy-target-departs", 7);
    }

    private void prepare(String copyTarget, String counterTarget) {
        this.copyTarget = copyTarget;
        this.counterTarget = counterTarget;
        setStrictChooseMode(true);
        addCard(Zone.BATTLEFIELD, playerA, "M11-Mountain");
        addCard(Zone.BATTLEFIELD, playerA, "M11-Island", 3);
        addCard(Zone.BATTLEFIELD, playerB, "M11-Island", 2);
        addCard(Zone.BATTLEFIELD, playerB, "M11-Silvercoat Lion@lion1");
        addCard(Zone.BATTLEFIELD, playerA, "M11-Silvercoat Lion@lion2");
        addCard(Zone.HAND, playerA, "M11-Lightning Bolt@bolt");
        addCard(Zone.HAND, playerB, "M10-Twincast@twincast");
        addCard(Zone.HAND, playerA, "7ED-Counterspell@counter");
        addCard(Zone.HAND, playerA, "M11-Unsummon@bounce");
        setChoice(playerB, copyTarget.equals("lion2"));
        if (copyTarget.equals("lion2")) addTarget(playerB, "@lion2");
    }

    private void startOriginal() {
        castSpell(1, MAIN, playerA, "@bolt", "@lion1");
        observe("bolt-on-stack", playerA, "bolt", "BATTLEFIELD", "BATTLEFIELD", "");
    }

    private void startTwincast() {
        castSpell(1, MAIN, playerB, "@twincast", "Lightning Bolt[no copy]", "Lightning Bolt");
        observe("twincast-on-stack", playerB, "bolt,twincast", "BATTLEFIELD", "BATTLEFIELD", "");
    }

    private static List<String> names(String commaSeparated) {
        return commaSeparated.isEmpty() ? new ArrayList<>() : Arrays.asList(commaSeparated.split(","));
    }

    private void observe(String checkpoint, TestPlayer actor, String stackAliases, String lion1Zone,
                         String lion2Zone, String graveyardAliases) {
        runCode(checkpoint, 1, MAIN, actor, (info, player, game) -> {
            List<String> expectedStack = names(stackAliases);
            List<String> actualStack = new ArrayList<>();
            List<String> entries = new ArrayList<>();
            java.util.Iterator<StackObject> stack = game.getStack().descendingIterator();
            while (stack.hasNext()) {
                StackObject item = stack.next();
                String alias = stackAlias(item, game);
                actualStack.add(alias);
                assertTrue("Only spell stack entries are in this fixture", item instanceof Spell);
                String owner = seat(((Spell) item).getOwnerId());
                String controller = seat(item.getControllerId());
                String expectedSeat = alias.equals("twincast") || alias.equals("bolt-copy") ? "B" : "A";
                assertEquals(checkpoint + ": owner " + alias, expectedSeat, owner);
                assertEquals(checkpoint + ": controller " + alias, expectedSeat, controller);
                List<String> targets = new ArrayList<>();
                item.getStackAbility().getTargets().forEach(target -> target.getTargets()
                        .forEach(id -> targets.add(identity(id, game))));
                String expectedTarget = alias.equals("bolt") || alias.equals("bounce") ? "lion1"
                        : alias.equals("twincast") ? "bolt" : alias.equals("counter") ? counterTarget : copyTarget;
                assertEquals(checkpoint + ": targets " + alias, Arrays.asList(expectedTarget), targets);
                entries.add("{\"alias\":" + quote(alias) + ",\"kind\":" + quote(item.isCopy() ? "spell-copy" : "card-spell")
                        + ",\"owner\":" + quote(owner) + ",\"controller\":" + quote(controller)
                        + ",\"targets\":[" + quote(targets.get(0)) + "]}");
            }
            assertEquals(checkpoint, expectedStack, actualStack);
            if (copySourceId != null && !actualStack.contains("bolt-copy")) {
                assertNull(checkpoint + ": departed copy stack object", game.getStack().getStackObject(copyStackId));
                assertEquals(checkpoint + ": copy SBA cleanup", Zone.OUTSIDE, game.getState().getZone(copySourceId));
                assertTrue(checkpoint + ": active copy removed", game.getState().getCopiedCards().stream()
                        .noneMatch(card -> card.getId().equals(copySourceId)));
                assertNull(game.getPermanent(copySourceId));
                game.getPlayers().values().forEach(seat -> {
                    assertFalse(seat.getHand().contains(copySourceId));
                    assertFalse(seat.getGraveyard().contains(copySourceId));
                    assertNull(seat.getLibrary().getCard(copySourceId, game));
                });
            }
            List<String> cards = new ArrayList<>();
            for (String alias : CARDS) {
                UUID id = playerA.getAliasByName(alias);
                Card card = game.getCard(id);
                Permanent permanent = game.getPermanent(id);
                String zone = game.getState().getZone(id).name();
                String expectedZone = alias.equals("lion1") ? lion1Zone : alias.equals("lion2") ? lion2Zone
                        : expectedStack.contains(alias) ? "STACK" : names(graveyardAliases).contains(alias) ? "GRAVEYARD" : "HAND";
                assertEquals(checkpoint + ": " + alias, expectedZone, zone);
                String owner = seat(card.getOwnerId());
                assertEquals(alias.equals("twincast") || alias.equals("lion1") ? "B" : "A", owner);
                if (permanent != null) {
                    assertEquals(0, permanent.getDamage());
                    assertEquals(2, permanent.getPower().getValue());
                    assertEquals(2, permanent.getToughness().getValue());
                }
                cards.add("{\"alias\":" + quote(alias) + ",\"owner\":" + quote(owner) + ",\"zone\":" + quote(zone)
                        + ",\"damage\":" + (permanent == null ? "null" : permanent.getDamage())
                        + ",\"power\":" + (permanent == null ? "null" : permanent.getPower().getValue())
                        + ",\"toughness\":" + (permanent == null ? "null" : permanent.getToughness().getValue())
                        + ",\"class\":" + quote(card.getClass().getName()) + ",\"set\":" + quote(card.getExpansionSetCode())
                        + ",\"number\":" + quote(card.getCardNumber()) + "}");
            }
            assertEquals(20, game.getPlayer(playerA.getId()).getLife());
            assertEquals(20, game.getPlayer(playerB.getId()).getLife());
            observations.add("{\"checkpoint\":" + quote(checkpoint) + ",\"stack_bottom_first\":[" + String.join(",", entries)
                    + "],\"life\":{\"A\":20,\"B\":20},\"cards\":[" + String.join(",", cards) + "]}");
        });
    }

    private String stackAlias(StackObject item, Game game) {
        if (item.isCopy()) {
            assertEquals("Lightning Bolt", item.getName());
            if (copySourceId == null) {
                copySourceId = item.getSourceId();
                copyStackId = item.getId();
            }
            assertEquals(copySourceId, item.getSourceId());
            assertEquals(copyStackId, item.getId());
            for (String alias : CARDS) assertNotEquals(playerA.getAliasByName(alias), copySourceId);
            return "bolt-copy";
        }
        for (String alias : CARDS) if (item.getSourceId().equals(playerA.getAliasByName(alias))) return alias;
        throw new AssertionError("Unrecognized physical spell source: " + item.getSourceId());
    }

    private String identity(UUID id, Game game) {
        StackObject item = game.getStack().getStackObject(id);
        if (item != null) return stackAlias(item, game);
        for (String alias : CARDS) if (id.equals(playerA.getAliasByName(alias))) return alias;
        throw new AssertionError("Unrecognized fixture identity: " + id);
    }

    private String seat(UUID id) {
        if (id.equals(playerA.getId())) return "A";
        if (id.equals(playerB.getId())) return "B";
        throw new AssertionError("Unrecognized seat: " + id);
    }

    private static String quote(String value) {
        return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private void finish(String scenario, int count) throws IOException {
        setStopAt(1, PhaseStep.BEGIN_COMBAT);
        execute();
        assertEquals(count, observations.size());
        assertNotNull("Actual Twincast must create a copy", copySourceId);
        String output = "{\"schema_version\":2,\"scenario\":" + quote(scenario)
                + ",\"reference\":\"xmage\",\"commit\":\"81c2f6d806dbec8eb79ad2a511cb686dff8361e7\",\"strict_choices\":true,\"mana_auto_payment\":true,\"observations\":["
                + String.join(",", observations) + "]}\n";
        Files.write(Paths.get(System.getProperty("spellwake.evidence.dir"), scenario + ".json"), output.getBytes(StandardCharsets.UTF_8));
    }
}
