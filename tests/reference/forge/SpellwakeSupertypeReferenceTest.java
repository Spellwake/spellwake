package forge.ai;

import forge.LobbyPlayer;
import forge.card.CardRarity;
import forge.card.CardRules;
import forge.game.Game;
import forge.game.ability.AbilityFactory;
import forge.game.ability.AbilityUtils;
import forge.game.GameRules;
import forge.game.GameType;
import forge.game.GameStage;
import forge.game.Match;
import forge.deck.Deck;
import forge.game.phase.PhaseType;
import forge.game.player.IGameEntitiesFactory;
import forge.game.player.PlayerController;
import forge.game.player.RegisteredPlayer;
import forge.game.GameEntity;
import forge.game.card.Card;
import forge.game.card.CardCollection;
import forge.game.card.CardCollectionView;
import forge.game.player.DelayedReveal;
import forge.game.player.Player;
import forge.game.spellability.SpellAbility;
import forge.game.zone.ZoneType;
import forge.item.PaperCard;
import forge.util.collect.FCollectionView;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import org.testng.Assert;
import org.testng.annotations.Test;

/** Prepared actual state-check probes. Choices are prescribed, never AI policy.
 * Initial timestamps/name records are explicit premises, not normal entry/casting.
 */
public class SpellwakeSupertypeReferenceTest extends AITest {
    private static final class Choices extends StrictRejectingController {
        final Map<Set<Integer>,Integer> keep = new LinkedHashMap<>();
        int selections, arrangements;
        Choices(Game game, Player player, LobbyPlayer lobby) { super(game,player,lobby); }
        protected AssertionError reject(String method,Object... arguments) {
            return new AssertionError("unexpected controller call: "+method);
        }
        void choose(Card selected, Card... offered) {
            Set<Integer> key=new TreeSet<>();for(Card c:offered)key.add(c.getId());
            Assert.assertNull(keep.put(key,selected.getId()),"duplicate choice premise");
        }
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> offered, DelayedReveal reveal, SpellAbility ability, String title, boolean optional, Player related, Map<String,Object> params) {
            Set<Integer> key=new TreeSet<>();for(T entity:offered)key.add(entity.getId());
            Integer selected=keep.remove(key);Assert.assertNotNull(selected,"unprescribed legend domain: "+key);
            selections++;
            System.out.println("SPELLWAKE_LEGEND choice="+selected+" offered="+key);
            for(T entity:offered)if(entity.getId()==selected)return entity;
            throw new AssertionError("selected card not offered");
        }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destination, SpellAbility source) {
            Assert.assertEquals(destination,ZoneType.Graveyard);Assert.assertNull(source);
            List<Card> ordered=new ArrayList<>();for(Card c:cards)ordered.add(c);
            ordered.sort((a,b)->Integer.compare(a.getId(),b.getId()));arrangements++;
            System.out.println("SPELLWAKE_GRAVEYARD prescribed ascending IDs="+ordered.stream().map(Card::getId).toList());
            return new CardCollection(ordered);
        }
        void consumed(int expected) { Assert.assertTrue(keep.isEmpty());Assert.assertEquals(selections,expected); }
    }
    private static final class StrictLobby extends LobbyPlayer implements IGameEntitiesFactory {
        StrictLobby(String name) {super(name);}
        @Override public Player createIngamePlayer(Game game,int id) {
            Player player=new Player(getName(),game,id);player.setFirstController(new Choices(game,player,this));return player;
        }
        @Override public PlayerController createMindSlaveController(Player master,Player slave) {throw new AssertionError("unexpected mind slave");}
        @Override public void hear(LobbyPlayer from,String message) {throw new AssertionError("unexpected lobby message");}
    }
    private Game preparedGame() {
        List<RegisteredPlayer> players=List.of(new RegisteredPlayer(new Deck()).setPlayer(new StrictLobby("A")),new RegisteredPlayer(new Deck()).setPlayer(new StrictLobby("B")));
        GameRules rules=new GameRules(GameType.Constructed);Game game=new Game(players,rules,new Match(rules,players,"Spellwake supertype prepared probe"));
        game.setAge(GameStage.Play);game.getPhaseHandler().devModeSet(PhaseType.MAIN1,game.getPlayers().get(1));game.getPhaseHandler().onStackResolved();return game;
    }
    private Choices strict(Game game, Player player) {
        Assert.assertEquals(player.getController().getGame(),game);
        return (Choices)player.getController();
    }
    private Card seed(Game game, Player owner, String name, String type, long worldAge) {
        Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of(
            "Name:"+name,"ManaCost:0","Types:"+type,"PT:2/3","Oracle:")),"???",CardRarity.Common),owner);
        card.setGameTimestamp(game.getNextTimestamp());
        if(worldAge>=0)card.updateWorldTimestamp(worldAge);
        owner.getZone(ZoneType.Battlefield).add(card);return card;
    }
    private void zone(Card card, ZoneType expected) { Assert.assertEquals(card.getZone().getZoneType(),expected); }

    @Test public void controllerChoosesOneSameNameLegendAndOtherControllerKeepsTheirOwn() {
        Game game=preparedGame();Player p=game.getPlayers().get(1),other=game.getPlayers().get(0);
        Choices choices=strict(game,p);strict(game,other);
        Card first=seed(game,p,"Spellwake Legend","Legendary Creature Elf",-1);
        Card second=seed(game,p,"Spellwake Legend","Legendary Creature Elf",-1);
        Card opponent=seed(game,other,"Spellwake Legend","Legendary Creature Elf",-1);
        choices.choose(second,first,second);game.getAction().checkStateEffects(true);
        choices.consumed(1);zone(first,ZoneType.Graveyard);zone(second,ZoneType.Battlefield);zone(opponent,ZoneType.Battlefield);
    }
    @Test public void chosenLegendCanAlsoBeDoomedBySimultaneousLethalDamage() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);Choices choices=strict(game,p);strict(game,game.getPlayers().get(0));
        Card dying=seed(game,p,"Spellwake Legend","Legendary Creature Elf",-1);
        Card healthy=seed(game,p,"Spellwake Legend","Legendary Creature Elf",-1);
        dying.setDamage(3);choices.choose(dying,dying,healthy);game.getAction().checkStateEffects(true);
        choices.consumed(1);zone(dying,ZoneType.Graveyard);zone(healthy,ZoneType.Graveyard);
    }
    @Test public void overlappingNameGroupsOfferTheAlreadyDoomedBridgeBeforeAnyMoves() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);Choices choices=strict(game,p);strict(game,game.getPlayers().get(0));
        Card a=seed(game,p,"Grizzly Bears","Legendary Creature Bear",-1);
        Card b=seed(game,p,"Runeclaw Bear","Legendary Creature Bear",-1);
        Card bridge=seed(game,p,"Spellwake Bridge","Legendary Creature Bear",-1);
        bridge.addChangedName(null,true,game.getNextTimestamp(),0);
        Assert.assertTrue(bridge.sharesNameWith(a));Assert.assertTrue(bridge.sharesNameWith(b));Assert.assertFalse(a.sharesNameWith(b));
        choices.choose(a,a,bridge);choices.choose(bridge,b,bridge);
        game.getAction().checkStateEffects(true);choices.consumed(2);
        zone(a,ZoneType.Battlefield);zone(b,ZoneType.Graveyard);zone(bridge,ZoneType.Graveyard);
    }
    @Test public void worldRuleKeepsTheSoleShortestTenureAcrossControllers() {
        Game game=preparedGame();Player p=game.getPlayers().get(1),other=game.getPlayers().get(0);strict(game,p);strict(game,other);
        long older=game.getNextTimestamp(),newer=game.getNextTimestamp();
        Card a=seed(game,p,"Spellwake Old World","World Artifact",older),b=seed(game,other,"Spellwake New World","World Artifact",newer);
        game.getAction().checkStateEffects(true);zone(a,ZoneType.Graveyard);zone(b,ZoneType.Battlefield);
        System.out.println("SPELLWAKE_WORLD kept="+b.getId()+" old-age="+a.getWorldTimestamp()+" new-age="+b.getWorldTimestamp());
    }
    @Test public void equalShortestWorldTenuresPutAllWorldsInTheGraveyard() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);Choices choices=strict(game,p);strict(game,game.getPlayers().get(0));
        long same=game.getNextTimestamp();
        Card a=seed(game,p,"Spellwake First World","World Artifact",same),b=seed(game,p,"Spellwake Second World","World Artifact",same);
        Assert.assertNotEquals(a.getGameTimestamp(),b.getGameTimestamp());Assert.assertEquals(a.getWorldTimestamp(),b.getWorldTimestamp());
        game.getAction().checkStateEffects(true);zone(a,ZoneType.Graveyard);zone(b,ZoneType.Graveyard);choices.consumed(0);
    }
    @Test public void cardTypeReplacementPreservesPrintedSupertypes() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);strict(game,p);strict(game,game.getPlayers().get(0));
        Card receiver=seed(game,p,"Spellwake Supertype Receiver","Legendary Snow Creature Elf",-1);
        SpellAbility ability=AbilityFactory.getAbility("DB$ Animate | ValidTgts$ Permanent | Types$ Artifact | RemoveCardTypes$ True | Duration$ Permanent",receiver);
        ability.setActivatingPlayer(p);ability.getTargets().add(receiver);AbilityUtils.resolve(ability);game.getAction().checkStaticAbilities();
        Assert.assertTrue(receiver.getType().isArtifact());Assert.assertFalse(receiver.getType().isCreature());
        Assert.assertTrue(receiver.getType().isLegendary());Assert.assertTrue(receiver.getType().isSnow());
        Assert.assertFalse(receiver.getType().hasSubtype("Elf"));
        System.out.println("SPELLWAKE_SUPERTYPE_TYPE_REPLACE type="+receiver.getType());
    }
    @Test public void losingOneSupertypePreservesTheOthersAndCreatureSubtype() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);strict(game,p);strict(game,game.getPlayers().get(0));
        Card receiver=seed(game,p,"Spellwake Supertype Receiver","Legendary Snow Creature Elf",-1);
        SpellAbility ability=AbilityFactory.getAbility("DB$ Animate | ValidTgts$ Permanent | RemoveTypes$ Snow | Duration$ Permanent",receiver);
        ability.setActivatingPlayer(p);ability.getTargets().add(receiver);AbilityUtils.resolve(ability);game.getAction().checkStaticAbilities();
        Assert.assertFalse(receiver.getType().isSnow());Assert.assertTrue(receiver.getType().isLegendary());
        Assert.assertTrue(receiver.getType().isCreature());Assert.assertTrue(receiver.getType().hasSubtype("Elf"));
        System.out.println("SPELLWAKE_SUPERTYPE_REMOVE type="+receiver.getType());
    }
    @Test public void gainingWorldByAnActualAnimateHandlerUsesTheNewUninterruptedTenure() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);strict(game,p);strict(game,game.getPlayers().get(0));
        Card oldWorld=seed(game,p,"Spellwake Old World","World Artifact",game.getNextTimestamp());
        Card receiver=seed(game,p,"Spellwake Acquiring World","Artifact",-1);
        SpellAbility ability=AbilityFactory.getAbility("DB$ Animate | ValidTgts$ Permanent | Types$ World | Duration$ Permanent",receiver);
        ability.setActivatingPlayer(p);ability.getTargets().add(receiver);AbilityUtils.resolve(ability);game.getAction().checkStaticAbilities();
        System.out.println("SPELLWAKE_WORLD_GAIN before-check old-age="+oldWorld.getWorldTimestamp()+" new-age="+receiver.getWorldTimestamp()+" new-type="+receiver.getType());
        game.getAction().checkStateEffects(true);
        System.out.println("SPELLWAKE_WORLD_GAIN after-check old-zone="+oldWorld.getZone().getZoneType()+" new-zone="+receiver.getZone().getZoneType());
        zone(oldWorld,ZoneType.Graveyard);zone(receiver,ZoneType.Battlefield);
    }

    private Card handSpell(Player owner, String type) {
        Card card=Card.fromPaperCard(new PaperCard(new CardRules.Reader().readCard(List.of(
            "Name:Spellwake Restriction Probe","ManaCost:G","Types:"+type,"Oracle:")),"???",CardRarity.Common),owner);
        owner.getZone(ZoneType.Hand).add(card);return card;
    }
    private SpellAbility restrictionSpell(Card card, Player player) {
        SpellAbility ability=AbilityFactory.getAbility("SP$ GainLife | Cost$ G | LifeAmount$ 3",card);
        ability.setActivatingPlayer(player);return ability;
    }
    private boolean permitted(Card card, SpellAbility ability, Player player) {
        return ability.getRestrictions().checkOtherRestrictions(card,ability,player);
    }
    private void actualEffect(Game game, Card host, Card target, Player player, String definition) {
        SpellAbility ability=AbilityFactory.getAbility(definition,host);ability.setActivatingPlayer(player);
        ability.getTargets().add(target);AbilityUtils.resolve(ability);game.getAction().checkStaticAbilities();
    }
    @Test public void legendaryInstantAndSorceryNeedACurrentControlledLegendCreature() {
        for(String type:List.of("Legendary Instant","Legendary Sorcery")) {
            Game game=preparedGame();Player p=game.getPlayers().get(1),other=game.getPlayers().get(0);
            Card card=handSpell(p,type);SpellAbility ability=restrictionSpell(card,p);
            Assert.assertFalse(permitted(card,ability,p));
            seed(game,p,"Spellwake Ordinary Keeper","Creature Elf",-1);Assert.assertFalse(permitted(card,ability,p));
            seed(game,other,"Spellwake Opposing Keeper","Legendary Creature Elf",-1);Assert.assertFalse(permitted(card,ability,p));
            seed(game,p,"Spellwake Relic","Legendary Artifact",-1);Assert.assertFalse(permitted(card,ability,p));
            seed(game,p,"Spellwake Keeper","Legendary Creature Elf",-1);Assert.assertTrue(permitted(card,ability,p));
            zone(card,ZoneType.Hand);System.out.println("SPELLWAKE_LEGENDARY_CAST type="+type+" matrix=blocked-before-controlled-legend-creature/allowed-after");
        }
    }
    @Test public void actualSupertypeAndCreatureTypeChangesUpdateTheLegendaryCastPrerequisite() {
        Game game=preparedGame();Player p=game.getPlayers().get(1);Card card=handSpell(p,"Legendary Instant");SpellAbility ability=restrictionSpell(card,p);
        Card receiver=seed(game,p,"Spellwake Ordinary Receiver","Creature Elf",-1);Assert.assertFalse(permitted(card,ability,p));
        actualEffect(game,card,receiver,p,"DB$ Animate | ValidTgts$ Permanent | Types$ Legendary | Duration$ Permanent");Assert.assertTrue(receiver.getType().isLegendary());Assert.assertTrue(permitted(card,ability,p));
        actualEffect(game,card,receiver,p,"DB$ Animate | ValidTgts$ Permanent | RemoveTypes$ Legendary | Duration$ Permanent");Assert.assertFalse(receiver.getType().isLegendary());Assert.assertFalse(permitted(card,ability,p));
        Card artifact=seed(game,p,"Spellwake Animated Relic","Legendary Artifact",-1);Assert.assertFalse(permitted(card,ability,p));
        actualEffect(game,card,artifact,p,"DB$ Animate | ValidTgts$ Permanent | Types$ Creature | Power$ 2 | Toughness$ 3 | Duration$ Permanent");Assert.assertTrue(artifact.isCreature());Assert.assertTrue(permitted(card,ability,p));
        actualEffect(game,card,artifact,p,"DB$ Animate | ValidTgts$ Permanent | RemoveTypes$ Creature | Duration$ Permanent");Assert.assertFalse(artifact.isCreature());Assert.assertFalse(permitted(card,ability,p));
    }
    @Test public void actualControlChangesUseTheCurrentControllerForLegendaryCasting() {
        Game game=preparedGame();Player p=game.getPlayers().get(1),other=game.getPlayers().get(0);
        Card card=handSpell(p,"Legendary Sorcery");SpellAbility ability=restrictionSpell(card,p);
        Card keeper=seed(game,other,"Spellwake Opposing Keeper","Legendary Creature Elf",-1);Assert.assertFalse(permitted(card,ability,p));
        actualEffect(game,card,keeper,p,"DB$ GainControl | ValidTgts$ Creature | Duration$ Permanent");Assert.assertEquals(keeper.getController(),p);Assert.assertTrue(permitted(card,ability,p));
        Card otherHost=handSpell(other,"Instant");actualEffect(game,otherHost,keeper,other,"DB$ GainControl | ValidTgts$ Creature | Duration$ Permanent");Assert.assertEquals(keeper.getController(),other);Assert.assertFalse(permitted(card,ability,p));
    }

}
