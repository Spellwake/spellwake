"""Source-backed operation descriptions for card review and conservative retrieval."""

from __future__ import annotations

import re
from pathlib import Path
from typing import TypedDict


class Source(TypedDict):
    file: str
    line: int


class OperationLabel(TypedDict):
    label: str
    scope: str
    sources: list[Source]


ENGINE = "crates/engine/src/"
CARDS = "crates/card_programs/src/keyword_creatures/"
PROGRAM = ENGINE + "program.rs"
EFFECTS = ENGINE + "effects.rs"
SEARCH = ENGINE + "effects/library_search.rs"
ENTRY = ENGINE + "effects/entry_counters.rs"
TYPES = ENGINE + "characteristics/types.rs"
EVALUATE = ENGINE + "characteristics/evaluation.rs"
MANA = ENGINE + "mana.rs"
LANDS = CARDS + "basic_lands.rs"

# Anchor the committed behavior or an explicit acceptance guard. Diagnostics name
# the exact fragment that drifted so a fresh code review can update the description.
OPERATIONS: dict[str, tuple[str, str, tuple[tuple[str, str], ...]]] = {
    "KeywordAbility::Flying": (
        "An attacker with flying may be blocked only by a creature with flying or reach.",
        (
            "This printed or granted characteristic restricts blocking the attacker. A blocker "
            "with flying can also block a ground attacker, subject to the other blocking rules. "
            "The keyword does not grant itself to a different permanent."
        ),
        (
            (
                "crates/card_programs/src/keyword_creatures.rs",
                '"flying" => KeywordAbility::Flying',
            ),
            (
                ENGINE + "effects/combat_flow/blocking.rs",
                "if attacker.has_keyword(Flying) && !(blocker.has_keyword(Flying) || blocker.has_keyword(Reach))",
            ),
        ),
    ),
    "KeywordAbility::Reach": (
        "A blocker with reach satisfies the restriction imposed by an attacker with flying.",
        (
            "This characteristic changes which attackers its bearer can block. It gives a "
            "ground attacker no flying evasion of its own."
        ),
        (
            (
                "crates/card_programs/src/keyword_creatures.rs",
                '"reach" => KeywordAbility::Reach',
            ),
            (
                ENGINE + "effects/combat_flow/blocking.rs",
                "if attacker.has_keyword(Flying) && !(blocker.has_keyword(Flying) || blocker.has_keyword(Reach))",
            ),
        ),
    ),
    "KeywordAbility::Haste": (
        "A creature with haste bypasses the continuous-control check for attacking and tapping.",
        (
            "Checked on the creature that attacks or pays its tap cost. Granting haste to other "
            "creatures still needs an effect with a recipient set, a duration and this keyword."
        ),
        (
            (
                "crates/card_programs/src/keyword_creatures.rs",
                '"haste" => KeywordAbility::Haste',
            ),
            (
                ENGINE + "effects/combat_flow.rs",
                "|| current.has_keyword(KeywordAbility::Haste))",
            ),
            (EFFECTS, "&& !current.has_keyword(KeywordAbility::Haste)"),
        ),
    ),
    "Instruction::SearchLibrary": (
        "Search the controller's library, reveal the qualifying card, put it into hand and shuffle.",
        (
            "A qualified search may fail. BasicLand goes to hand. Only the Transfigure filter routes "
            "its qualifying creature to the battlefield. This operation cannot choose a tapped "
            "battlefield destination for an arbitrary search."
        ),
        (
            (PROGRAM, "Search the resolving controller's library for a matching card"),
            (SEARCH, "let to_battlefield=matches!"),
            (SEARCH, "ObjectDestination::Hand,ZoneReason::SearchToHand"),
            (SEARCH, ".search_shuffle("),
        ),
    ),
    "LibrarySearchFilter::BasicLand": (
        "Select a card that has both the Land type and Basic supertype in the searched library.",
        (
            "This is a search predicate. A nonbasic with a basic land subtype does not qualify. "
            "The SearchLibrary resolver moves a found basic to hand, even if the desired text "
            "says to put it onto the battlefield tapped."
        ),
        (
            (SEARCH, "LibrarySearchFilter::BasicLand =>"),
            (SEARCH, "current.current_supertypes.contains(&Supertype::Basic)"),
            (SEARCH, "ObjectDestination::Hand,ZoneReason::SearchToHand"),
        ),
    ),
    "LibrarySearchFilter::Transfigure": (
        "Find a creature card of the specified mana value and put it onto the battlefield.",
        (
            "This destination is tied to the Transfigure filter and sacrificed source's mana value. "
            "It does not generalize to fetching a basic land or to specifying tapped entry."
        ),
        (
            (PROGRAM, "Search for a creature with the sacrificed source's mana value"),
            (SEARCH, "LibrarySearchFilter::Transfigure(value) =>"),
            (
                SEARCH,
                "ObjectDestination::Battlefield{controller:actor},ZoneReason::TransfigureEntry",
            ),
        ),
    ),
    "Instruction::Damage": (
        "Deal the specified amount of damage to a legal player or battlefield creature.",
        (
            "Target validation and damage-recipient types only admit players and creatures. "
            "Any target also needs planeswalkers and battles. The operation does not allocate "
            "divided damage between multiple targets."
        ),
        (
            (EFFECTS, "Instruction::Damage { .. } => match target"),
            (ENGINE + "damage.rs", "pub enum DamageRecipient {"),
            (PROGRAM, "Deliberately excludes planeswalkers and battles"),
        ),
    ),
    "BodyTarget::SingleCreatureOrPlayer": (
        "Choose one player or one battlefield creature for an instruction body.",
        (
            "A target contract for one recipient. Planeswalkers and battles as such are outside it. "
            "The damage resolver imposes the same player-or-creature check."
        ),
        (
            (PROGRAM, "Deliberately excludes planeswalkers and battles"),
            (EFFECTS, "BodyTarget::SingleCreatureOrPlayer => match target"),
            (EFFECTS, "Instruction::Damage { .. } => match target"),
        ),
    ),
    "BodyTarget::SingleSpell": (
        "Choose one spell on the stack that passes the supplied spell filter.",
        (
            "This contract supplies the spell operand to compatible instructions. It neither "
            "moves the spell nor makes the permanent-only return instruction accept it."
        ),
        (
            (EFFECTS, "BodyTarget::SingleSpell(filter) => match target"),
            (PROGRAM, "(BodyTarget::SingleSpell(_), OperandFamily::Spell)"),
            (
                PROGRAM,
                "Instruction::DestroyTarget | Instruction::ReturnToOwnersHand =>",
            ),
        ),
    ),
    "BodyTarget::SingleOwnGraveyardCard": (
        "Choose one card in the resolving controller's graveyard.",
        (
            "It binds a graveyard-card operand and performs no zone change. Multiple cards, a "
            "shared mana-value budget, and an artifact-or-enchantment filter are outside this "
            "contract. The normal return-to-hand instruction accepts a permanent operand."
        ),
        (
            (EFFECTS, "BodyTarget::SingleOwnGraveyardCard =>"),
            (
                PROGRAM,
                "Instruction::DestroyTarget | Instruction::ReturnToOwnersHand =>",
            ),
        ),
    ),
    "Instruction::ReturnToOwnersHand": (
        "Move a targeted battlefield permanent to its owner's hand.",
        (
            "Program validation requires a permanent operand. Returning a spell from the stack "
            "or a card from a graveyard needs a compatible origin and move operation."
        ),
        (
            (
                PROGRAM,
                "Instruction::DestroyTarget | Instruction::ReturnToOwnersHand =>",
            ),
            (EFFECTS, "ObjectDestination::Hand, ZoneReason::ReturnToOwnersHand"),
        ),
    ),
    "TransferKeyword::Soulshift": (
        "Optionally return a targeted Spirit card of mana value at most N from your graveyard to hand.",
        (
            "This is the Soulshift trigger's filtered, optional return. It does not implement "
            "an unconditional return of any card or an unfiltered graveyard-to-hand spell."
        ),
        (
            (ENGINE + "effects/transfers.rs", "TransferKeyword::Soulshift(limit) =>"),
            (ENGINE + "effects/transfers.rs", "TransferKeyword::Soulshift(_) =>"),
        ),
    ),
    "Instruction::DestroyTarget": (
        "Destroy a targeted battlefield permanent by moving it to its owner's graveyard.",
        (
            "Requires a permanent operand. A plain SinglePermanent contract admits every "
            "permanent type. An artifact-or-planeswalker condition needs an additional disjunctive "
            "target filter. Indestructible prevents the destruction."
        ),
        (
            (
                PROGRAM,
                "Instruction::DestroyTarget | Instruction::ReturnToOwnersHand =>",
            ),
            (EFFECTS, "ObjectDestination::Graveyard, ZoneReason::Destroyed"),
            (
                EFFECTS,
                "view.zone == ObjectZone::Battlefield && !view.has_keyword(KeywordAbility::Indestructible)",
            ),
        ),
    ),
    "BodyTarget::SinglePermanent": (
        "Choose one object on the battlefield.",
        (
            "This plain target contract checks zone. It requires additional restrictions for "
            "artifact-only, planeswalker-only, or artifact-or-planeswalker selection."
        ),
        ((EFFECTS, "BodyTarget::SinglePermanent => matches!(target"),),
    ),
    "EntryTap::Always": (
        "A permanent's printed static ability makes that permanent enter the battlefield tapped.",
        (
            "The shared entry pipeline reads the entering permanent's entry_tap property before "
            "entry observation. The land compiler binds unconditional land entry text. The simple "
            "mana-artifact compiler lacks a binding for the artifact's enters-tapped clause. "
            "An instruction putting a different object onto the battlefield tapped needs its own "
            "entry modifier. Face selection and play from exile are separate."
        ),
        (
            (LANDS, '"" => EntryTap::Always'),
            (ENTRY, "EntryTap::Always=>true"),
            (ENTRY, "self.apply_entry_status(object,before)?"),
            (CARDS + "mana_permanents.rs", "} else if rest.is_empty() {"),
            (
                CARDS + "mana_permanents.rs",
                "ordinary_triggers::compile(&reduced, vocabulary)",
            ),
        ),
    ),
    "GameCommand::PlayLand": (
        "Use a land play to put a land from the acting player's hand onto the battlefield.",
        (
            "The command requires a land program and its object in the actor's hand. It cannot "
            "exercise Adventure permission from exile or select a land face on a modal card."
        ),
        (
            (ENGINE + "game.rs", "(None, GameCommand::PlayLand { land }) =>"),
            (EFFECTS, "fn play_land("),
            (EFFECTS, "Some(program) if program.has_type(CardType::Land)"),
        ),
    ),
    "ControllerSelector::SourceController": (
        "Select affected permanents currently controlled by the source's controller.",
        (
            "Compares the candidate's controller to the source's controller. Combine this "
            "predicate with a creature type filter and an abilities-layer grant for a static "
            "effect on creatures you control."
        ),
        (
            (TYPES, "pub controller: ControllerSelector,"),
            (
                EVALUATE,
                "ControllerSelector::SourceController => object.controller == Some(controller)",
            ),
        ),
    ),
    "StaticPart::ControlSourceController": (
        "Set affected permanents' controller to the source's current controller.",
        (
            "This changes game state in the control layer. An effect applying only to creatures "
            "you already control needs ControllerSelector::SourceController as a predicate."
        ),
        (
            (
                TYPES,
                "Set the affected permanents' controller to the source's current controller.",
            ),
            (
                EVALUATE,
                "StaticPart::ControlSourceController => ResolvedPart::Control(controller)",
            ),
        ),
    ),
    "StaticPart::Abilities": (
        "Apply abilities-layer changes, including granting specified keywords, to selected permanents.",
        (
            "The selector defines the affected set independently. For a continuous grant of haste "
            "to creatures you control, filter by creature type and source controller and grant the "
            "Haste keyword. An until-end-of-turn one-shot grant has distinct duration and set rules."
        ),
        (
            (
                EVALUATE,
                "StaticPart::Abilities(change) => ResolvedPart::Abilities(change.clone())",
            ),
            (TYPES, "pub granted_keywords: BTreeSet<KeywordAbility>,"),
        ),
    ),
    "Instruction::CreateContinuousEffect": (
        "Apply specified continuous-effect parts to one targeted permanent for the supplied duration.",
        (
            "The resolving instruction requires one object target. It can apply keyword grants "
            "until end of turn to that permanent. A nontargeted grant to all creatures controlled "
            "at resolution still needs selection of that whole set."
        ),
        (
            (EFFECTS, "Instruction::CreateContinuousEffect { parts, duration } =>"),
            (
                EFFECTS,
                "let parts = parts.materialize(locals.locals(), entry.controller(), owner)?",
            ),
        ),
    ),
    "Instruction::CreateTokens": (
        "Create tokens using an authored token definition and a specified quantity.",
        (
            "The token template defines its permanent characteristics, abilities and entry behavior. "
            "Creating a named token does not by itself supply that token's mana ability or validate "
            "spending restrictions such as the Powerstone nonartifact-spell restriction."
        ),
        (
            (EFFECTS, "self.begin_token_entries(&creations, None, false)?"),
            (ENGINE + "token.rs", "pub struct TokenCreation {"),
            (ENGINE + "token.rs", "pub struct TokenDefinition {"),
            (
                ENGINE + "effects/token_entry_flow.rs",
                "self.apply_entry_counters(&before, *member",
            ),
            (MANA, "Restricted mana, hybrid/snow/Phyrexian/X"),
        ),
    ),
    "ManaOutput::Fixed": (
        "Add a fixed positive amount of one or more mana types from an authored tap ability.",
        (
            "The output maps mana types to amounts. A restriction on how that mana may be spent "
            "is outside this representation. Timing and tap legality are checked separately."
        ),
        (
            (MANA, "Fixed(BTreeMap<ManaType, RulesInteger>),"),
            (EFFECTS, "ManaOutput::Fixed(produced) =>"),
            (MANA, "Restricted mana, hybrid/snow/Phyrexian/X"),
        ),
    ),
}

# These seven branches are the specific conditional predicates the land compiler
# authors and the entry resolver checks against the snapshot before the entry.
ENTRY_CONDITIONS = {
    "UnlessTwoOtherLands": (
        "you control two or more other lands",
        "two or more other lands under its controller's control",
    ),
    "UnlessTwoOrFewerOtherLands": (
        "you control two or fewer other lands",
        "two or fewer other lands under its controller's control",
    ),
    "UnlessTwoBasicLands": (
        "you control two or more basic lands",
        "two or more basic lands under its controller's control",
    ),
    "UnlessTwoOpponents": (
        "you have two or more opponents",
        "its controller having at least two opponents",
    ),
    "UnlessOpponentLandsEight": (
        "your opponents control eight or more lands",
        "the opponents collectively controlling at least eight lands",
    ),
    "UnlessPlayerLifeThirteenOrLess": (
        "a player has 13 or less life",
        "any player having 13 or less life",
    ),
}
for variant, (clause, exception) in ENTRY_CONDITIONS.items():
    OPERATIONS[f"EntryTap::{variant}"] = (
        f"Land entry starts tapped except when the pre-entry state satisfies the condition of {exception}.",
        (
            "The original land text must bind this specific exception. The entering land is "
            "absent from the pre-entry snapshot used to decide its initial tapped state."
        ),
        ((LANDS, f'" unless {clause}"'), (ENTRY, f"EntryTap::{variant}=>")),
    )
OPERATIONS["EntryTap::UnlessLandType"] = (
    "Land enters tapped unless its controller already controls a land of either specified basic land type.",
    (
        "The two alternatives test current land subtypes on the pre-entry battlefield. The parser "
        "accepts the two named basic land types. A land bearing either subtype can qualify even "
        "when it lacks the Basic supertype."
    ),
    (
        (LANDS, "EntryTap::UnlessLandType(basic(a)?, basic(b)?)"),
        (ENTRY, "EntryTap::UnlessLandType(a,b)=>"),
    ),
)


def operation_labels(root: Path) -> dict[str, OperationLabel]:
    """Resolve all reviewed evidence anchors to one-based lines, failing on source drift."""
    files: dict[str, str] = {}
    result: dict[str, OperationLabel] = {}
    for symbol, (label, scope, anchors) in OPERATIONS.items():
        sources: list[Source] = []
        for file, fragment in anchors:
            if file not in files:
                files[file] = (root / file).read_text(encoding="utf-8")
            content = files[file]
            if content.count(fragment) != 1:
                raise ValueError(
                    f"{symbol}: expected one source anchor in {file}: {fragment!r}"
                )
            line = content.count("\n", 0, content.index(fragment)) + 1
            sources.append({"file": file, "line": line})
        result[symbol] = {"label": label, "scope": scope, "sources": sources}
    return result


# Origin and destination both matter. Lead with the requested origin on a move
# between zones so the scope of a superficially similar operation is visible.
INTENTS = (
    (
        r"\bsearch (?:your|their|that player's|target player's) library for (?:an? |up to \w+ )?basic land",
        ("LibrarySearchFilter::BasicLand", "Instruction::SearchLibrary"),
    ),
    (
        r"\bsearch (?:your|their|that player's|target player's) library",
        ("Instruction::SearchLibrary",),
    ),
    (
        r"\btransfigure\b",
        ("LibrarySearchFilter::Transfigure", "Instruction::SearchLibrary"),
    ),
    (
        r"\breturn (?:up to [^.;]*? )?target spell\b[^.;]*?\bhand\b",
        ("BodyTarget::SingleSpell", "Instruction::ReturnToOwnersHand"),
    ),
    (
        r"\breturn\b[^.;]*?\bgraveyard\b[^.;]*?\bhand\b",
        ("BodyTarget::SingleOwnGraveyardCard", "Instruction::ReturnToOwnersHand"),
    ),
    (
        r"\bsoulshift\b",
        ("TransferKeyword::Soulshift", "BodyTarget::SingleOwnGraveyardCard"),
    ),
    (
        r"\breturn\b[^.;]*?\b(?:target|each|all)\b[^.;]*?\b(?:permanent|creature|artifact|enchantment|land)s?\b[^.;]*?\bhand\b",
        ("Instruction::ReturnToOwnersHand",),
    ),
    (
        r"\b(?:deals?|dealt)\b[^.;]*?\bdamage\b[^.;]*?\b(?:any|another) target\b",
        ("Instruction::Damage", "BodyTarget::SingleCreatureOrPlayer"),
    ),
    (
        r"\bdestroy target\b",
        ("Instruction::DestroyTarget", "BodyTarget::SinglePermanent"),
    ),
    (
        r"\b(?:play|plays) (?:it|the land|that (?:card|land))[^.;]*?\b(?:from exile|exiled)\b",
        ("GameCommand::PlayLand",),
    ),
    (
        r"\bcreatures? you control (?:gain|gains)\b[^.;]*?until end of turn",
        ("Instruction::CreateContinuousEffect",),
    ),
    (
        r"\bcreatures? you control (?:have|has)\b",
        ("ControllerSelector::SourceController", "StaticPart::Abilities"),
    ),
    (
        r"\b(?:you control|gain control of) (?:enchanted|equipped|target) (?:creature|permanent|artifact)\b",
        ("StaticPart::ControlSourceController",),
    ),
    (
        r"\bcreate\b[^.;]*?\bpowerstone tokens?\b",
        ("Instruction::CreateTokens", "ManaOutput::Fixed"),
    ),
    (r"\bcreate\b[^.;]*?\btokens?\b", ("Instruction::CreateTokens",)),
    (r"\{t\}: add (?:\{[cwubrg]\})+\.", ("ManaOutput::Fixed",)),
)


def preferred_operations(text: str) -> list[str]:
    """Return ordered candidate symbols. A match may expose only part of the clause."""
    normalized = " ".join(text.casefold().replace("’", "'").split())
    selected: dict[str, None] = {}
    entry = re.search(
        r"(?:^|[.!?] )this (?:permanent|land|artifact(?: creature)?|creature|enchantment) "
        r"enters (?:the battlefield )?tapped(?: unless ([^.;(]+))?(?=[.;(]|$)",
        normalized,
    )
    if entry:
        exception = entry.group(1)
        if exception is None:
            selected["EntryTap::Always"] = None
        else:
            for variant, (clause, _) in ENTRY_CONDITIONS.items():
                if exception.strip() == clause:
                    selected[f"EntryTap::{variant}"] = None
            land_type = r"(?:a|an) (?:forest|island|mountain|plains|swamp)"
            if re.fullmatch(
                rf"you control {land_type} or {land_type}", exception.strip()
            ):
                selected["EntryTap::UnlessLandType"] = None
    for pattern, symbols in INTENTS:
        if re.search(pattern, normalized):
            selected.update(dict.fromkeys(symbols))
    # Ability lists and explicit keyword grants are positive signals. A subject
    # qualified by "without flying" or "creature with haste" is a different role.
    for keyword in ("Flying", "Haste", "Reach"):
        word = keyword.lower()
        if re.search(rf"(?:^|[,;]\s*){word}\b", normalized) or re.search(
            rf"\b(?:has|have|gain|gains)\b[^.;:()]*?\b{word}\b", normalized
        ):
            selected[f"KeywordAbility::{keyword}"] = None
    return list(selected)
