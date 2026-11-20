package forge.ai;

import com.google.common.collect.*;
import forge.LobbyPlayer;
import forge.card.*;
import forge.card.mana.*;
import forge.deck.*;
import forge.game.*;
import forge.game.ability.effects.RollDiceEffect;
import forge.game.card.*;
import forge.game.card.sticker.Sticker;
import forge.game.combat.*;
import forge.game.cost.*;
import forge.game.keyword.KeywordInterface;
import forge.game.mana.*;
import forge.game.phase.PhaseType;
import forge.game.player.*;
import forge.game.replacement.ReplacementEffect;
import forge.game.spellability.*;
import forge.game.staticability.StaticAbility;
import forge.game.trigger.*;
import forge.game.zone.*;
import forge.item.*;
import forge.util.*;
import forge.util.collect.FCollectionView;
import org.apache.commons.lang3.tuple.*;
import java.util.*;
import java.util.function.*;

/** Shared fail-closed interface defaults. Every admitted reply belongs to the subclass protocol. */
public abstract class StrictRejectingController extends PlayerController {
    protected StrictRejectingController(Game game, Player player, LobbyPlayer lobby) { super(game, player, lobby); }
    protected abstract AssertionError reject(String method, Object... arguments);
    @Override public SpellAbility getAbilityToPlay(Card host, List<SpellAbility> abilities, ITriggerEvent event) { throw reject("getAbilityToPlay",host,abilities,event); }
    @Override public void playSpellAbilityNoStack(SpellAbility ability, boolean mayChooseTargets) { throw reject("playSpellAbilityNoStack",ability,mayChooseTargets); }
    @Override public void orderAndPlaySimultaneousSa(List<SpellAbility> abilities) { throw reject("orderAndPlaySimultaneousSa",abilities); }
    @Override public TargetChoices chooseNewTargetsFor(SpellAbility ability, Predicate<GameObject> filter, boolean optional) { throw reject("chooseNewTargetsFor",ability,filter,optional); }
    @Override public boolean chooseTargetsFor(SpellAbility ability) { throw reject("chooseTargetsFor",ability); }
    @Override public void notifyOfValue(SpellAbility ability, GameObject related, String value) { throw reject("notifyOfValue",ability,related,value); }
    @Override public String chooseSomeType(String kind, SpellAbility ability, Collection<String> types, boolean optional) { throw reject("chooseSomeType",kind,ability,types,optional); }
    @Override public List<SpellAbility> chooseSpellAbilityToPlay() { throw reject("chooseSpellAbilityToPlay"); }
    @Override public boolean playChosenSpellAbility(SpellAbility ability) { throw reject("playChosenSpellAbility",ability); }
    @Override public ReplacementEffect chooseSingleReplacementEffect(List<ReplacementEffect> effects) { throw reject("chooseSingleReplacementEffect",effects); }
    @Override public boolean payManaCost(ManaCost cost, CostPartMana part, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { throw reject("payManaCost",cost,part,ability,prompt,matrix,effect); }
    @Override public boolean applyManaToCost(ManaCostBeingPaid cost, SpellAbility ability, String prompt, ManaConversionMatrix matrix, boolean effect) { throw reject("applyManaToCost",cost,ability,prompt,matrix,effect); }
    @Override public CostDecisionMakerBase getCostDecisionMaker(Player payer, SpellAbility ability, boolean effect, String prompt) { throw reject("getCostDecisionMaker",payer,ability,effect,prompt); }
        @Override protected boolean orderedMoveToTopOfLibrary(ZoneType destination,SpellAbility source){throw reject("orderedMoveToTopOfLibrary",destination,source);}
        @Override public void tempShowCards(Iterable<Card> cards){throw reject("tempShowCards",cards);}
        @Override public void endTempShowCards(){throw reject("endTempShowCards");}
        @Override public boolean sideboardsByItself(){throw reject("sideboardsByItself");}
        @Override public List<String> getSideboardingReport(){throw reject("getSideboardingReport");}
        @Override public GameOutcome.AnteResult getAnteResult(){throw reject("getAnteResult");}
        @Override public CardCollectionView cheatShuffle(CardCollectionView list){ throw reject("cheatShuffle",list); }
        @Override public Map<DeckSection,List<? extends PaperCard>> complainCardsCantPlayWell(Deck deck){ throw reject("complainCardsCantPlayWell",deck); }
        @Override public void resetAtEndOfTurn(){throw reject("resetAtEndOfTurn");}
        @Override public List<SpellAbility> orderSimultaneousSa(List<SpellAbility> activePlayerSAs) { throw reject("orderSimultaneousSa",activePlayerSAs); }
        @Override public boolean playTrigger(Card host, WrappedAbility wrapperAbility, boolean isMandatory) { throw reject("playTrigger",host,wrapperAbility,isMandatory); }
        @Override public boolean playSaFromPlayEffect(SpellAbility tgtSA) { throw reject("playSaFromPlayEffect",tgtSA); }
        @Override public List<PaperCard> sideboard(final Deck deck, GameType gameType, String message) { throw reject("sideboard",deck,gameType,message); }
        @Override public List<PaperCard> chooseCardsYouWonToAddToDeck(List<PaperCard> losses) { throw reject("chooseCardsYouWonToAddToDeck",losses); }
        @Override public Map<Card, Integer> assignCombatDamage(Card attacker, CardCollectionView blockers, CardCollectionView remaining, int damageDealt, GameEntity defender, boolean overrideOrder) { throw reject("assignCombatDamage",attacker,blockers,remaining,damageDealt,defender,overrideOrder); }
        @Override public Map<GameEntity, Integer> divideShield(Card effectSource, Map<GameEntity, Integer> affected, int shieldAmount) { throw reject("divideShield",effectSource,affected,shieldAmount); }
        @Override public Map<Byte, Integer> specifyManaCombo(SpellAbility sa, ColorSet colorSet, int manaAmount, boolean different) { throw reject("specifyManaCombo",sa,colorSet,manaAmount,different); }
        @Override public CardCollectionView choosePermanentsToSacrifice(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw reject("choosePermanentsToSacrifice",sa,min,max,validTargets,message); }
        @Override public CardCollectionView choosePermanentsToDestroy(SpellAbility sa, int min, int max, CardCollectionView validTargets, String message) { throw reject("choosePermanentsToDestroy",sa,min,max,validTargets,message); }
        @Override public Integer announceRequirements(SpellAbility ability, int min, int max, String announce) { throw reject("announceRequirements",ability,min,max,announce); }
        @Override public Pair<SpellAbilityStackInstance, GameObject> chooseTarget(SpellAbility sa, List<Pair<SpellAbilityStackInstance, GameObject>> allTargets) { throw reject("chooseTarget",sa,allTargets); }
        @Override public boolean helpPayForAssistSpell(ManaCostBeingPaid cost, SpellAbility sa, int max, int requested) { throw reject("helpPayForAssistSpell",cost,sa,max,requested); }
        @Override public Player choosePlayerToAssistPayment(FCollectionView<Player> optionList, SpellAbility sa, String title, int max) { throw reject("choosePlayerToAssistPayment",optionList,sa,title,max); }
        @Override public CardCollectionView chooseCardsForEffect(CardCollectionView sourceList, SpellAbility sa, String title, int min, int max, boolean isOptional, Map<String, Object> params) { throw reject("chooseCardsForEffect",sourceList,sa,title,min,max,isOptional,params); }
        @Override public CardCollection chooseCardsForEffectMultiple(Map<String, CardCollection> validMap, SpellAbility sa, String title, boolean isOptional) { throw reject("chooseCardsForEffectMultiple",validMap,sa,title,isOptional); }
        @Override public <T extends GameEntity> T chooseSingleEntityForEffect(FCollectionView<T> optionList, DelayedReveal delayedReveal, SpellAbility sa, String title, boolean isOptional, Player relatedPlayer, Map<String, Object> params) { throw reject("chooseSingleEntityForEffect",optionList,delayedReveal,sa,title,isOptional,relatedPlayer,params); }
        @Override public <T extends GameEntity> List<T> chooseEntitiesForEffect(FCollectionView<T> optionList, int min, int max, DelayedReveal delayedReveal, SpellAbility sa, String title, Player relatedPlayer, Map<String, Object> params) { throw reject("chooseEntitiesForEffect",optionList,min,max,delayedReveal,sa,title,relatedPlayer,params); }
        @Override public List<SpellAbility> chooseSpellAbilitiesForEffect(List<SpellAbility> spells, SpellAbility sa, String title, int num, Map<String, Object> params) { throw reject("chooseSpellAbilitiesForEffect",spells,sa,title,num,params); }
        @Override public SpellAbility chooseSingleSpellForEffect(List<SpellAbility> spells, SpellAbility sa, String title, Map<String, Object> params) { throw reject("chooseSingleSpellForEffect",spells,sa,title,params); }
        @Override public boolean confirmAction(SpellAbility sa, PlayerActionConfirmMode mode, String message, List<String> options, Card cardToShow, Map<String, Object> params) { throw reject("confirmAction",sa,mode,message,options,cardToShow,params); }
        @Override public boolean confirmBidAction(SpellAbility sa, PlayerActionConfirmMode bidlife, String string, int bid, Player winner) { throw reject("confirmBidAction",sa,bidlife,string,bid,winner); }
        @Override public boolean confirmReplacementEffect(ReplacementEffect replacementEffect, SpellAbility effectSA, GameEntity affected, String question) { throw reject("confirmReplacementEffect",replacementEffect,effectSA,affected,question); }
        @Override public boolean confirmStaticApplication(Card hostCard, PlayerActionConfirmMode mode, String message, String logic) { throw reject("confirmStaticApplication",hostCard,mode,message,logic); }
        @Override public boolean confirmTrigger(WrappedAbility sa) { throw reject("confirmTrigger",sa); }
        @Override public List<Card> exertAttackers(List<Card> attackers) { throw reject("exertAttackers",attackers); }
        @Override public List<Card> enlistAttackers(List<Card> attackers) { throw reject("enlistAttackers",attackers); }
        @Override public void declareAttackers(Player attacker, Combat combat) { throw reject("declareAttackers",attacker,combat); }
        @Override public void declareBlockers(Player defender, Combat combat) { throw reject("declareBlockers",defender,combat); }
        @Override public CardCollection orderBlockers(Card attacker, CardCollection blockers) { throw reject("orderBlockers",attacker,blockers); }
        @Override public CardCollection orderBlocker(final Card attacker, final Card blocker, final CardCollection oldBlockers) { throw reject("orderBlocker",attacker,blocker,oldBlockers); }
        @Override public CardCollection orderAttackers(Card blocker, CardCollection attackers) { throw reject("orderAttackers",blocker,attackers); }
        @Override public void reveal(CardCollectionView cards, ZoneType zone, Player owner, String messagePrefix, boolean addMsgSuffix) { throw reject("reveal",cards,zone,owner,messagePrefix,addMsgSuffix); }
        @Override public void reveal(List<CardView> cards, ZoneType zone, PlayerView owner, String messagePrefix, boolean addMsgSuffix) { throw reject("reveal",cards,zone,owner,messagePrefix,addMsgSuffix); }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForScry(CardCollection topN) { throw reject("arrangeForScry",topN); }
        @Override public ImmutablePair<CardCollection, CardCollection> arrangeForSurveil(CardCollection topN) { throw reject("arrangeForSurveil",topN); }
        @Override public boolean willPutCardOnTop(Card c) { throw reject("willPutCardOnTop",c); }
        @Override public CardCollectionView orderMoveToZoneList(CardCollectionView cards, ZoneType destinationZone, SpellAbility source) { throw reject("orderMoveToZoneList",cards,destinationZone,source); }
        @Override public CardCollectionView chooseCardsToDiscardFrom(Player playerDiscard, SpellAbility sa, CardCollection validCards, int min, int max, CardCollectionView visibleToChooser) { throw reject("chooseCardsToDiscardFrom",playerDiscard,sa,validCards,min,max,visibleToChooser); }
        @Override public CardCollectionView chooseCardsToDiscardUnlessType(int min, CardCollectionView hand, String[] unlessTypes, SpellAbility sa) { throw reject("chooseCardsToDiscardUnlessType",min,hand,unlessTypes,sa); }
        @Override public CardCollectionView chooseCardsToDiscardToMaximumHandSize(int numDiscard) { throw reject("chooseCardsToDiscardToMaximumHandSize",numDiscard); }
        @Override public CardCollectionView chooseCardsToDelve(int genericAmount, CardCollection grave) { throw reject("chooseCardsToDelve",genericAmount,grave); }
        @Override public Map<Card, ManaCostShard> chooseCardsForConvokeOrImprovise(SpellAbility sa, ManaCost manaCost, CardCollectionView untappedCards, boolean artifacts, boolean creatures, Integer maxReduction) { throw reject("chooseCardsForConvokeOrImprovise",sa,manaCost,untappedCards,artifacts,creatures,maxReduction); }
        @Override public List<Card> chooseCardsForSplice(SpellAbility sa, List<Card> cards) { throw reject("chooseCardsForSplice",sa,cards); }
        @Override public CardCollectionView chooseCardsToRevealFromHand(int min, int max, CardCollectionView valid) { throw reject("chooseCardsToRevealFromHand",min,max,valid); }
        @Override public List<SpellAbility> chooseSaToActivateFromOpeningHand(List<SpellAbility> usableFromOpeningHand) { throw reject("chooseSaToActivateFromOpeningHand",usableFromOpeningHand); }
        @Override public Player chooseStartingPlayer(boolean isFirstGame) { throw reject("chooseStartingPlayer",isFirstGame); }
        @Override public PlayerZone chooseStartingHand(List<PlayerZone> zones) { throw reject("chooseStartingHand",zones); }
        @Override public Mana chooseManaFromPool(List<Mana> manaChoices) { throw reject("chooseManaFromPool",manaChoices); }
        @Override public String chooseSector(Card assignee, String ai, List<String> sectors) { throw reject("chooseSector",assignee,ai,sectors); }
        @Override public Sticker chooseSticker(List<Sticker> options, Card target, SpellAbility sa, boolean isOptional) { throw reject("chooseSticker",options,target,sa,isOptional); }
        @Override public int chooseStickerNamePosition(Sticker sticker, Card target) { throw reject("chooseStickerNamePosition",sticker,target); }
        @Override public Card chooseCardToKeepStickers(CardCollectionView options) { throw reject("chooseCardToKeepStickers",options); }
        @Override public List<Card> chooseContraptionsToCrank(List<Card> contraptions) { throw reject("chooseContraptionsToCrank",contraptions); }
        @Override public int chooseSprocket(Card assignee, List<Integer> sprockets) { throw reject("chooseSprocket",assignee,sprockets); }
        @Override public PlanarDice choosePDRollToIgnore(List<PlanarDice> rolls) { throw reject("choosePDRollToIgnore",rolls); }
        @Override public Integer chooseRollToIgnore(List<Integer> rolls) { throw reject("chooseRollToIgnore",rolls); }
        @Override public List<Integer> chooseDiceToReroll(List<Integer> rolls) { throw reject("chooseDiceToReroll",rolls); }
        @Override public Integer chooseRollToModify(List<Integer> rolls) { throw reject("chooseRollToModify",rolls); }
        @Override public RollDiceEffect.DieRollResult chooseRollToSwap(List<RollDiceEffect.DieRollResult> rolls) { throw reject("chooseRollToSwap",rolls); }
        @Override public String chooseRollSwapValue(List<String> swapChoices, Integer currentResult, int power, int toughness) { throw reject("chooseRollSwapValue",swapChoices,currentResult,power,toughness); }
        @Override public Object vote(SpellAbility sa, String prompt, List<Object> options, ListMultimap<Object, Player> votes, Player forPlayer, boolean optional) { throw reject("vote",sa,prompt,options,votes,forPlayer,optional); }
        @Override public boolean mulliganKeepHand(Player player, int cardsToReturn) { throw reject("mulliganKeepHand",player,cardsToReturn); }
        @Override public CardCollectionView tuckCardsViaMulligan(CardCollectionView hand, int cardsToReturn) { throw reject("tuckCardsViaMulligan",hand,cardsToReturn); }
        @Override public List<AbilitySub> chooseModeForAbility(SpellAbility sa, List<AbilitySub> possible, int min, int num, boolean allowRepeat) { throw reject("chooseModeForAbility",sa,possible,min,num,allowRepeat); }
        @Override public int chooseNumberForCostReduction(final SpellAbility sa, final int min, final int max) { throw reject("chooseNumberForCostReduction",sa,min,max); }
        @Override public int chooseNumberForKeywordCost(SpellAbility sa, Cost cost, KeywordInterface keyword, String prompt, int max) { throw reject("chooseNumberForKeywordCost",sa,cost,keyword,prompt,max); }
        @Override public int chooseNumber(SpellAbility sa, String title, int min, int max) { throw reject("chooseNumber",sa,title,min,max); }
        @Override public int chooseNumber(SpellAbility sa, String title, List<Integer> values, Player relatedPlayer) { throw reject("chooseNumber",sa,title,values,relatedPlayer); }
        @Override public boolean chooseBinary(SpellAbility sa, String question, BinaryChoiceType kindOfChoice, Boolean defaultChoice) { throw reject("chooseBinary",sa,question,kindOfChoice,defaultChoice); }
        @Override public boolean chooseFlipResult(SpellAbility sa, Player flipper, boolean call) { throw reject("chooseFlipResult",sa,flipper,call); }
        @Override public byte chooseColor(String message, SpellAbility sa, ColorSet colors) { throw reject("chooseColor",message,sa,colors); }
        @Override public byte chooseColorAllowColorless(String message, Card c, ColorSet colors) { throw reject("chooseColorAllowColorless",message,c,colors); }
        @Override public ColorSet chooseColors(String message, SpellAbility sa, int min, int max, ColorSet options) { throw reject("chooseColors",message,sa,min,max,options); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, String message, Predicate<ICardFace> cpp, String name) { throw reject("chooseSingleCardFace",sa,message,cpp,name); }
        @Override public ICardFace chooseSingleCardFace(SpellAbility sa, List<ICardFace> faces, String message) { throw reject("chooseSingleCardFace",sa,faces,message); }
        @Override public CardState chooseSingleCardState(SpellAbility sa, List<CardState> states, String message, Map<String, Object> params) { throw reject("chooseSingleCardState",sa,states,message,params); }
        @Override public boolean chooseCardsPile(SpellAbility sa, CardCollectionView pile1, CardCollectionView pile2, String faceUp) { throw reject("chooseCardsPile",sa,pile1,pile2,faceUp); }
        @Override public CounterType chooseCounterType(List<CounterType> options, SpellAbility sa, String prompt, Map<String, Object> params) { throw reject("chooseCounterType",options,sa,prompt,params); }
        @Override public String chooseKeywordForPump(List<String> options, SpellAbility sa, String prompt, Card tgtCard) { throw reject("chooseKeywordForPump",options,sa,prompt,tgtCard); }
        @Override public boolean confirmPayment(CostPart costPart, String string, SpellAbility sa) { throw reject("confirmPayment",costPart,string,sa); }
        @Override public StaticAbility chooseSingleStaticAbility(List<StaticAbility> possibleReplacers) { throw reject("chooseSingleStaticAbility",possibleReplacers); }
        @Override public String chooseProtectionType(SpellAbility sa, List<String> choices) { throw reject("chooseProtectionType",sa,choices); }
        @Override public void revealAnte(String message, Multimap<Player, PaperCard> removedAnteCards) { throw reject("revealAnte",message,removedAnteCards); }
        @Override public void revealAISkipCards(String message, Map<Player, Map<DeckSection, List<? extends PaperCard>>> deckCards) { throw reject("revealAISkipCards",message,deckCards); }
        @Override public void revealUnsupported(Map<Player, List<PaperCard>> unsupported) { throw reject("revealUnsupported",unsupported); }
        @Override public List<OptionalCostValue> chooseOptionalCosts(SpellAbility choosen, List<OptionalCostValue> optionalCostValues) { throw reject("chooseOptionalCosts",choosen,optionalCostValues); }
        @Override public List<CostPart> orderCosts(List<CostPart> costs) { throw reject("orderCosts",costs); }
        @Override public boolean payCostToPreventEffect(Cost cost, SpellAbility sa, boolean alreadyPaid, FCollectionView<Player> allPayers) { throw reject("payCostToPreventEffect",cost,sa,alreadyPaid,allPayers); }
        @Override public boolean payCostDuringRoll(Cost cost, SpellAbility sa) { throw reject("payCostDuringRoll",cost,sa); }
        @Override public boolean payCombatCost(Card card, Cost cost, SpellAbility sa, String prompt) { throw reject("payCombatCost",card,cost,sa,prompt); }
        @Override public CardCollectionView chooseCardsForCost(CardCollectionView optionList, SpellAbility sa, CostPartWithList cpl, int amount, boolean isOptional, String prompt) { throw reject("chooseCardsForCost",optionList,sa,cpl,amount,isOptional,prompt); }
        @Override public String chooseCardName(SpellAbility sa, Predicate<ICardFace> cpp, String valid, String message) { throw reject("chooseCardName",sa,cpp,valid,message); }
        @Override public String chooseCardName(SpellAbility sa, List<ICardFace> faces, String message) { throw reject("chooseCardName",sa,faces,message); }
        @Override public Card chooseSingleCardForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, DelayedReveal delayedReveal, String selectPrompt, boolean isOptional, Player decider) { throw reject("chooseSingleCardForZoneChange",destination,origin,sa,fetchList,delayedReveal,selectPrompt,isOptional,decider); }
        @Override public List<Card> chooseCardsForZoneChange(ZoneType destination, List<ZoneType> origin, SpellAbility sa, CardCollection fetchList, int min, int max, DelayedReveal delayedReveal, String selectPrompt, Player decider) { throw reject("chooseCardsForZoneChange",destination,origin,sa,fetchList,min,max,delayedReveal,selectPrompt,decider); }
        @Override public void autoPassCancel() { throw reject("autoPassCancel"); }
        @Override public void awaitNextInput() { throw reject("awaitNextInput"); }
        @Override public void cancelAwaitNextInput() { throw reject("cancelAwaitNextInput"); }

    /** Cost visitors reject unless the finite owning protocol overrides them. */
    public abstract static class Costs extends CostDecisionMakerBase {
        protected Costs(Player player, SpellAbility ability, boolean effect) { super(player, effect, ability, ability.getHostCard()); }
        protected abstract AssertionError reject(String method, Object... arguments);
        @Override public boolean paysRightAfterDecision() { throw reject("paysRightAfterDecision"); }
        @Override public PaymentDecision visit(CostTap cost) { throw reject("visit_CostTap",cost); }
        @Override public PaymentDecision visit(CostPartMana cost) { throw reject("visit_CostPartMana",cost); }
        @Override public PaymentDecision visit(CostSacrifice cost) { throw reject("visit_CostSacrifice",cost); }
        @Override public PaymentDecision visit(CostBehold cost) { throw reject("visit_CostBehold",cost); }
        @Override public PaymentDecision visit(CostBeholdExile cost) { throw reject("visit_CostBeholdExile",cost); }
        @Override public PaymentDecision visit(CostGainControl cost) { throw reject("visit_CostGainControl",cost); }
        @Override public PaymentDecision visit(CostChooseColor cost) { throw reject("visit_CostChooseColor",cost); }
        @Override public PaymentDecision visit(CostChooseCreatureType cost) { throw reject("visit_CostChooseCreatureType",cost); }
        @Override public PaymentDecision visit(CostCollectEvidence cost) { throw reject("visit_CostCollectEvidence",cost); }
        @Override public PaymentDecision visit(CostDiscard cost) { throw reject("visit_CostDiscard",cost); }
        @Override public PaymentDecision visit(CostDamage cost) { throw reject("visit_CostDamage",cost); }
        @Override public PaymentDecision visit(CostDraw cost) { throw reject("visit_CostDraw",cost); }
        @Override public PaymentDecision visit(CostExile cost) { throw reject("visit_CostExile",cost); }
        @Override public PaymentDecision visit(CostExileFromStack cost) { throw reject("visit_CostExileFromStack",cost); }
        @Override public PaymentDecision visit(CostExiledMoveToGrave cost) { throw reject("visit_CostExiledMoveToGrave",cost); }
        @Override public PaymentDecision visit(CostExert cost) { throw reject("visit_CostExert",cost); }
        @Override public PaymentDecision visit(CostEnlist cost) { throw reject("visit_CostEnlist",cost); }
        @Override public PaymentDecision visit(CostFlipCoin cost) { throw reject("visit_CostFlipCoin",cost); }
        @Override public PaymentDecision visit(CostForage cost) { throw reject("visit_CostForage",cost); }
        @Override public PaymentDecision visit(CostRollDice cost) { throw reject("visit_CostRollDice",cost); }
        @Override public PaymentDecision visit(CostMill cost) { throw reject("visit_CostMill",cost); }
        @Override public PaymentDecision visit(CostAddMana cost) { throw reject("visit_CostAddMana",cost); }
        @Override public PaymentDecision visit(CostPayLife cost) { throw reject("visit_CostPayLife",cost); }
        @Override public PaymentDecision visit(CostPayEnergy cost) { throw reject("visit_CostPayEnergy",cost); }
        @Override public PaymentDecision visit(CostGainLife cost) { throw reject("visit_CostGainLife",cost); }
        @Override public PaymentDecision visit(CostPromiseGift cost) { throw reject("visit_CostPromiseGift",cost); }
        @Override public PaymentDecision visit(CostPutCardToLib cost) { throw reject("visit_CostPutCardToLib",cost); }
        @Override public PaymentDecision visit(CostReturn cost) { throw reject("visit_CostReturn",cost); }
        @Override public PaymentDecision visit(CostReveal cost) { throw reject("visit_CostReveal",cost); }
        @Override public PaymentDecision visit(CostRevealChosen cost) { throw reject("visit_CostRevealChosen",cost); }
        @Override public PaymentDecision visit(CostRemoveAnyCounter cost) { throw reject("visit_CostRemoveAnyCounter",cost); }
        @Override public PaymentDecision visit(CostRemoveCounter cost) { throw reject("visit_CostRemoveCounter",cost); }
        @Override public PaymentDecision visit(CostPutCounter cost) { throw reject("visit_CostPutCounter",cost); }
        @Override public PaymentDecision visit(CostPutCounterYou cost) { throw reject("visit_CostPutCounterYou",cost); }
        @Override public PaymentDecision visit(CostUntapType cost) { throw reject("visit_CostUntapType",cost); }
        @Override public PaymentDecision visit(CostUntap cost) { throw reject("visit_CostUntap",cost); }
        @Override public PaymentDecision visit(CostUnattach cost) { throw reject("visit_CostUnattach",cost); }
        @Override public PaymentDecision visit(CostTapType cost) { throw reject("visit_CostTapType",cost); }
        @Override public PaymentDecision visit(CostPayShards cost) { throw reject("visit_CostPayShards",cost); }
        @Override public PaymentDecision visit(CostBlight cost) { throw reject("visit_CostBlight",cost); }
    }
}
