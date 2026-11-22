"""Build a queryable ledger of Oracle requirements, CR references and scoped evidence."""
import argparse
import collections
import csv
import hashlib
import json
import re
import sqlite3
import shutil
import tempfile
from pathlib import Path

from card_oracle_paragraphs import clean_reminder

ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / "work/catalogue/card-support-audit-v1"
RULES = ROOT / "work/reference-research/forge-power-toughness/pinned-sources/MagicCompRules-20260925.txt"
WAVE = ROOT / "work/catalogue/keyword-wave-v6/rulebook.csv"

SCHEMA = """
PRAGMA foreign_keys=ON;
CREATE TABLE cr(rule_id TEXT PRIMARY KEY, parent_rule TEXT REFERENCES cr, source_line INTEGER,
               heading TEXT, body TEXT) WITHOUT ROWID;
CREATE TABLE historical_evidence(rule_id TEXT REFERENCES cr, source TEXT, evidence_json TEXT);
CREATE TABLE source_rule_reference(rule_id TEXT REFERENCES cr,source_path TEXT,source_line INTEGER,role TEXT,excerpt TEXT,
    PRIMARY KEY(rule_id,source_path,source_line)) WITHOUT ROWID;
CREATE TABLE game_receipt(card_id INTEGER REFERENCES card,source_file TEXT,observed_status TEXT,source_row TEXT,
    PRIMARY KEY(card_id,source_file)) WITHOUT ROWID;
CREATE TABLE card(id INTEGER PRIMARY KEY, oracle_id TEXT UNIQUE, name TEXT, layout TEXT, formats TEXT,
    support TEXT, processor TEXT, reason TEXT, evidence TEXT, prior_scopes TEXT, remaining_work TEXT,
    catalogue_line INTEGER, source_line_sha256 TEXT, semantic_sha256 TEXT, source_attributes TEXT);
CREATE INDEX card_by_name ON card(name COLLATE NOCASE);
CREATE TABLE face(id INTEGER PRIMARY KEY, card_id INTEGER REFERENCES card, face_index INTEGER, name TEXT,
    oracle_text TEXT, attributes TEXT, UNIQUE(card_id,face_index));
CREATE TABLE paragraph(id INTEGER PRIMARY KEY, face_id INTEGER REFERENCES face, ordinal INTEGER,
    start_char INTEGER,end_char INTEGER, text TEXT, probe_status TEXT,probe_context TEXT, UNIQUE(face_id,ordinal));
CREATE TABLE executable(card_id INTEGER PRIMARY KEY REFERENCES card, definition TEXT, details TEXT);
CREATE TABLE requirement(id INTEGER PRIMARY KEY, kind TEXT, label TEXT, UNIQUE(kind,label));
CREATE TABLE requirement_rule(req_id INTEGER REFERENCES requirement, rule_id TEXT REFERENCES cr,
    basis TEXT, PRIMARY KEY(req_id,rule_id,basis)) WITHOUT ROWID;
CREATE TABLE occurrence(id INTEGER PRIMARY KEY, face_id INTEGER REFERENCES face, paragraph_id INTEGER REFERENCES paragraph,
    req_id INTEGER REFERENCES requirement, field TEXT, start_char INTEGER, end_char INTEGER, evidence_scope TEXT);
CREATE TABLE global_requirement(req_id INTEGER PRIMARY KEY REFERENCES requirement, applicability TEXT);
CREATE TABLE blocker(card_id INTEGER REFERENCES card,req_id INTEGER REFERENCES requirement, basis TEXT,
    PRIMARY KEY(card_id,req_id)) WITHOUT ROWID;
CREATE VIEW card_requirements AS
    SELECT c.oracle_id,c.name,f.face_index,p.ordinal AS paragraph,r.kind,r.label,r.id AS req_id,
           o.field,o.start_char,o.end_char,o.evidence_scope FROM occurrence o JOIN face f ON f.id=o.face_id
    JOIN card c ON c.id=f.card_id JOIN requirement r ON r.id=o.req_id LEFT JOIN paragraph p ON p.id=o.paragraph_id
    UNION ALL
    SELECT c.oracle_id,c.name,NULL,NULL,r.kind,r.label,r.id,NULL,NULL,NULL,'game context; no per-card test attribution'
    FROM card c CROSS JOIN global_requirement g JOIN requirement r ON r.id=g.req_id
    WHERE g.applicability='all' OR (g.applicability='Commander games only' AND c.formats LIKE '%commander%');
CREATE VIEW card_rules AS SELECT v.*,m.rule_id,m.basis FROM card_requirements v JOIN requirement_rule m USING(req_id);
CREATE VIEW remaining_by_card AS SELECT c.oracle_id,c.name,r.id AS req_id,r.kind,r.label,b.basis,
    (SELECT COUNT(*) FROM blocker x WHERE x.card_id=b.card_id)=1 AS only_identified_blocker
    FROM blocker b JOIN card c ON c.id=b.card_id JOIN requirement r ON r.id=b.req_id;
"""

TYPE_RULE = dict(Artifact="301",Creature="302",Enchantment="303",Instant="304",Land="305",
                 Planeswalker="306",Sorcery="307",Kindred="308",Dungeon="309",Battle="310")
LAYOUT_RULE = dict(transform="712",modal_dfc="712",adventure="715",split="709",saga="714",
                   prepare="722",leveler="711",meld="701.42",flip="710",class_="716",
                   prototype="718",case="719",mutate="702.140")
LAYOUT_RULE["class"]=LAYOUT_RULE.pop("class_")
GLOBAL = {
    "Game setup and outcomes":("100","103","104"),
    "Players, objects and zone identity":("102","108","109","400","400.7"),
    "Priority and turn structure":("117","500"),
    "State-based actions":("704",),
    "Replacement and prevention interactions when applicable":("614","615","616"),
    "Continuous effects and layer ordering when applicable":("611","613"),
    "Handling illegal actions":("733",),
    "Commander deck construction, color identity and designation when playing Commander":("903",),
}
# These identify candidate *topics* in printed prose. They neither parse a whole ability nor prove it works.
SIGNALS = [
    (r"\b(?:when|whenever|at the beginning of|at the end of combat)\b","Trigger condition","603.1"),
    (r"\b(?:enters|enter the battlefield)\b","Battlefield entry event","603.6a"),
    (r"\b(?:dies|die)\b","Creature death","700.4"),
    (r"\btarget(?:s|ed)?\b","Target selection and legality","115"),
    (r"\b(?:deal|deals|dealt)\b[^.]{0,55}?\bdamage\b","Damage","120"),
    (r"\b(?:gain|gains|lose|loses|lost|pay|pays)\b[^.;:]{0,40}?\blife\b","Life totals and payments","119"),
    (r"\bdraw(?:s)?\b[^.]{0,40}?\bcards?\b","Drawing cards","121"),
    (r"\b(?:put|remove|move|double|has|with)\b[^.;:]{0,55}?\bcounters?\b","Counters on objects or players","122"),
    (r"\b(?:add|adds)\b[^.;:]{0,40}?(?:\bmana\b|\{[WUBRGC]\})","Mana production","106"),
    (r"\bactivate only\b","Activation restrictions","602.1b"),
    (r"\bas an additional cost\b","Additional cost","118.8"),
    (r"\brather than pay\b|\bwithout paying (?:its|their|that spell.s) mana cost\b","Alternative cost","118.9"),
    (r"\bcosts?\b[^.]{0,50}?\b(?:less|more)\b","Cost increase or reduction","601.2f"),
    (r"\binstead\b","Replacement effect","614.1a"),
    (r"\bskip(?:s)?\b","Skip replacement effect","614.1b"),
    (r"\bas (?:this|\w+) [^,.]{0,36}?enters\b|\benters (?:tapped|with|as)\b","Replacement as permanent enters","614.12"),
    (r"\bprevent(?:s)?\b[^.]{0,70}?\bdamage\b","Damage prevention","615"),
    (r"\bchoose (?:one|two|three|one or more)\b|^•","Mode choices (context required)","700.2"),
    (r"\b(?:until end of turn|for as long as|as long as)\b","Effect duration or condition","611"),
    (r"\b(?:gets?|have|has) [+-][X\d]+/[+-][X\d]+","Power/toughness modification","613.4"),
    (r"\b(?:base power and toughness|base power|base toughness)\b","Set base power/toughness","613.4"),
    (r"\b(?:gains? control|exchange control)\b","Control-changing effect","613.1b"),
    (r"\b(?:copy|copies|copied)\b","Copy rules","707"),
    (r"\b(?:tokens?|token copies)\b","Token objects","111"),
    (r"\b(?:attack|attacks|attacking|attacked|attackers?)\b","Attacking creatures","508"),
    (r"\b(?:block|blocks|blocking|blocked|blockers?)\b","Blocking creatures","509"),
    (r"\bcombat damage\b","Combat damage step","510"),
    (r"\bflash as though|as though it had flash\b","Timing permissions","117.1a"),
    (r"\b(?:private|secretly|face down|face-down)\b","Hidden information or face-down objects","708"),
    (r"\bcommander(?:s)?\b","Commander-specific use","903"),
    (r"\bopponents?\b","Player and opponent relationships","102"),
    (r"\b(?:beginning of|during) (?:your |each |the )?(?:next )?(?:end step|upkeep|draw step|combat|main phase)","Turn step and phase timing","500"),
    (r"\bdelayed triggered ability\b|\bat the beginning of the next\b","Delayed trigger","603.7"),
    (r"\b(?:from|into|in|to|of|onto) (?:your |its owner's |their |the |a |an opponent's )?(library|libraries)\b","Library zone","401"),
    (r"\b(?:graveyard|graveyards)\b","Graveyard zone","404"),
    (r"\bhand\b","Hand zone","402"),
    (r"\bbattlefield\b","Battlefield zone","403"),
    (r"\bstack\b","Stack zone","405"),
]
SIGNALS = [(re.compile(p,re.I), label, cr) for p,label,cr in SIGNALS]

def table(path, sep=","):
    with path.open(encoding="utf8",newline="") as file:
        yield from csv.DictReader(file,delimiter=sep)

def jsonlines(path):
    with path.open(encoding="utf8") as file:
        for line in file:
            yield json.loads(line)

def compact(data):
    return json.dumps(data,ensure_ascii=False,separators=(",",":"))

def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file,"sha256").hexdigest()

def parse_rules(path):
    """Index only the normative body of the pinned CR, excluding the repeated TOC and glossary."""
    text=path.read_text(encoding="utf-8-sig").split("\n")
    start=next(i for i,l in enumerate(text) if l.startswith("100.1."))
    start=max(i for i in range(start) if text[i].strip()=="1. Game Concepts")
    end=next(i for i in range(start,len(text)) if text[i].strip()=="Glossary")
    entries=[]
    for i in range(start,end):
        m=re.match(r"^(\d{1,3}(?:\.\d+[a-z]?)?)\.?\s+(\S.*)",text[i])
        if m:
            rid,head=m.groups()
            parent=(rid[:-1] if rid[-1].isalpha() else rid.partition(".")[0] if "." in rid else rid[0] if len(rid)==3 else None)
            entries.append((rid,parent,i+1,head))
    rows={}
    for n,(rid,parent,line,heading) in enumerate(entries):
        stop=entries[n+1][2]-1 if n+1<len(entries) else end
        if rid in rows or (parent and parent not in rows):
            raise ValueError(f"Malformed pinned CR hierarchy at line {line}, {rid}")
        rows[rid]=(rid,parent,line,heading,"\n".join(text[line-1:stop]).rstrip())
    return rows

def symbol_rule(symbol):
    if symbol=="{X}": return "107.3"
    if symbol=="{0}": return "107.4d"
    if symbol=="{C}": return "107.4c"
    if symbol=="{S}": return "107.4h"
    if symbol=="{T}": return "107.5"
    if symbol=="{Q}": return "107.6"
    if symbol=="{E}": return "107.14"
    if symbol in ("{W}","{U}","{B}","{R}","{G}"): return "107.4a"
    if "/P" in symbol: return "107.4f"
    if "/" in symbol: return "107.4e"
    if symbol[1:-1].isdigit(): return "107.4b"
    return "107"

class Ledger:
    def __init__(self,conn,rules):
        self.db=conn;self.rules=rules;self.known={};self.links=set()
        self.occ=[];self.blocks=set()
        lexicon={}
        for rid,(_,_,_,title,_) in rules.items():
            if re.fullmatch(r"70[12]\.\d+",rid) and len(title)<50 and not title.endswith("."):
                lexicon[title.casefold()]=(title,rid)
        # Rule 702.29 also defines typecycling and basic landcycling variants.
        for alias,target in {"basic landcycling":"cycling","megamorph":"morph",
                             "multikicker":"kicker","hexproof from":"hexproof"}.items():
            if target in lexicon:lexicon[alias]=lexicon[target]
        self.lexicon=lexicon
        self.words=re.compile(r"(?<![\w-])(?:"+"|".join(re.escape(term) for term in sorted(lexicon,key=len,reverse=True))+r")(?![\w-])",re.I)

    def req(self,kind,label,rules=(),basis="CR topic inferred from source"):
        key=(kind,label)
        if key not in self.known:
            self.known[key]=len(self.known)+1
            self.db.execute("INSERT INTO requirement VALUES(?,?,?)",(self.known[key],kind,label))
        rid=self.known[key]
        for rule in rules:
            if rule not in self.rules: raise ValueError(f"Unknown CR {rule} for {label}")
            record=(rid,rule,basis)
            if record not in self.links:
                self.db.execute("INSERT INTO requirement_rule VALUES(?,?,?)",record);self.links.add(record)
        return rid

    def mark(self,f,p,k,label,field,a,b,scope,rules=(),basis="CR topic inferred from source"):
        req=self.req(k,label,rules,basis)
        self.occ.append((f,p,req,field,a,b,scope))
        return req

    def flush(self):
        self.db.executemany("INSERT INTO occurrence(face_id,paragraph_id,req_id,field,start_char,end_char,evidence_scope) VALUES(?,?,?,?,?,?,?)",self.occ)
        self.occ.clear()
        self.db.executemany("INSERT INTO blocker VALUES(?,?,?)",self.blocks);self.blocks.clear()

    def printed(self,card,face,f,scope):
        def base(kind,label,field,a,b,cr):
            return self.mark(f,None,kind,label,field,a,b,scope,cr,"printed characteristic or normal rules for this face")
        for field,cr in [("name","201"),("power","208"),("toughness","208"),("loyalty","209"),("defense","210"),("color_indicator","204"),("colors","105")]:
            if field in face:
                value=face[field]
                base("characteristic",field.replace("_"," "),field,0,len(value) if isinstance(value,str) else None,(cr,))
        t=face.get("type_line","")
        base("characteristic","type line","type_line",0,len(t),("205.1",))
        lhs,sep,rhs=t.partition(" — ")
        cs=set(lhs.split())
        for m in re.finditer(r"\S+",lhs):
            word=m.group()
            base("card type" if word in TYPE_RULE else "supertype",word,"type_line",*m.span(),
                 (TYPE_RULE[word],"205.2a") if word in TYPE_RULE else ("205.4a",))
        if sep:
            base("characteristic","printed subtypes","type_line",len(lhs)+len(sep),len(t),("205.3",))
            if "Land" in cs:
                for m in re.finditer(r"\b(?:Forest|Swamp|Island|Plains|Mountain)\b",rhs):
                    base("intrinsic mana",m.group(),"type_line",len(lhs)+len(sep)+m.start(),len(lhs)+len(sep)+m.end(),("305.6","605"))
        if "Land" in cs:
            base("normal play","land play and allowance","type_line",0,len(lhs),("305.1","305.2"))
        elif cs & TYPE_RULE.keys():
            base("normal play","spell casting, eligibility and resolution depend on face and layout","type_line",0,len(lhs),("117.1a","601","608"))
        cost=face.get("mana_cost","")
        base("printed cost","mana cost" if cost else "no printed mana cost","mana_cost",0,len(cost),("202.1","118") if cost else ("202.1b","118.6"))
        for m in re.finditer(r"\{[^}]+\}",cost):
            base("mana cost component",m.group(),"mana_cost",*m.span(),(symbol_rule(m.group()),))
        layout=card["layout"]
        base("layout",layout,"layout",0,len(layout), (LAYOUT_RULE[layout],) if layout in LAYOUT_RULE else ("108",))

    def paragraph(self,cid,f,p,start,text,face,scope,unmatched):
        mask,key=clean_reminder(text)
        def note(kind,label,lo,hi,rules=(),basis="CR topic inferred from source"):
            return self.mark(f,p,kind,label,"oracle_text",start+lo,start+hi,scope,rules,basis)
        refs=set()
        for pattern,label,cr in SIGNALS:
            for m in pattern.finditer(mask):
                note("prose topic",label,*m.span(),(cr,));refs.add(cr)
        colon=mask.find(":")
        if colon!=-1 and not mask[:colon].strip().startswith(("•","“",'"')) and '"' not in mask[:colon] and "“" not in mask[:colon]:
            label="activation cost syntax" if not re.match(r"^[+−-]?(?:[0-9]+|X)$",mask[:colon].strip()) else "loyalty activation cost syntax"
            cr="602.1a" if label.startswith("activation") else "606"
            note("ability cost",label,0,colon,(cr,));refs.add(cr)
        for m in self.words.finditer(mask):
            label,cr=self.lexicon[m.group().casefold()]
            # 'counter' is also a physical counter; prefer an explicit spell/ability counter context.
            if label=="Counter" and not re.match(r"\s+(?:target|that|it\b|each|all|the)",mask[m.end():],re.I):continue
            # 'double strike' is a keyword; never turn the noun 'ward' in a name into a tested ability.
            note("keyword or action mention",label,*m.span(),(cr,),"lexical reference; grant, restriction or named ability depends on context")
            refs.add(cr)
        for m in re.finditer(r"\{[^}]+\}",mask):
            cr=symbol_rule(m.group());note("ability symbol",m.group(),*m.span(),(cr,));refs.add(cr)
        if not key: return None
        shape=" ".join(key.replace(face["name"],"CARDNAME").split())
        # No broad replacements of amounts, colors, target filters, durations or conditions.
        label=face["type_line"].split(" — ")[0]+" | "+shape
        rid=note("Oracle paragraph",label,0,len(text),tuple(sorted(refs)),"topics present in paragraph; complete semantic derivation unreviewed")
        if unmatched:self.blocks.add((cid,rid,"isolated full paragraph rejected; engine support and compiler admission not distinguished"))
        return rid

def write_cards(ledger,folder):
    db=ledger.db
    cards={row["oracle_id"]:row for row in table(folder/"cards.csv")}
    if len(cards)!=32155:raise ValueError("expected the complete 32,155-card audit")
    programs={p["oracle_id"]:p for p in jsonlines(folder/"programs.jsonl")}
    probes=collections.defaultdict(collections.deque)
    for row in table(folder/"clauses.tsv","\t"):
        probes[(row["oracle_id"],row["face_name"],row["clause"])].append((row["status"],row["context"]))
    f=p=0
    counts=collections.Counter()
    for cid,card in enumerate(jsonlines(folder/"ai-review-input.jsonl"),1):
        oid=card["oracle_id"];meta=cards.pop(oid)
        if meta["name"]!=card["name"] or meta["layout"]!=card["layout"]:raise ValueError(f"source identity mismatch {oid}")
        program=programs.pop(oid,None)
        if (meta["current_processor"]=="Executable")!=bool(program):raise ValueError(f"missing or orphan program {oid}")
        bound=program is not None
        cols=("name","layout","formats","support","current_processor","reason","behavior_evidence","earlier_profile_scopes","remaining_work_families")
        attrs={k:v for k,v in card.items() if k not in ("faces","name","oracle_id","layout","source_line_sha256","catalogue_line")}
        db.execute("INSERT INTO card VALUES("+",".join("?" for _ in range(15))+")",(
            cid,oid,*(meta[k] for k in cols),card["catalogue_line"],card["source_line_sha256"],meta["semantic_sha256"],compact(attrs)))
        if bound:
            if program["name"]!=card["name"] or not program["complete_engine_definition"]:raise ValueError(oid)
            definition=program.pop("complete_engine_definition")
            db.execute("INSERT INTO executable VALUES(?,?,?)",(cid,definition,compact(program)))
        base_scope="represented by whole-card executable; component not independently proved" if bound else "source characteristic; runtime applicability unverified"
        if not card["faces"]:raise ValueError(f"no source faces {oid}")
        observed_gaps=set()
        for face in card["faces"]:
            f+=1
            rules_text=face.get("oracle_text","")
            db.execute("INSERT INTO face VALUES(?,?,?,?,?,?)",(f,cid,face["face_index"],face["name"],rules_text,compact({k:v for k,v in face.items() if k!="oracle_text"})))
            ledger.printed(card,face,f,base_scope)
            # Split only on the Oracle LF separator. Preserve blank paragraphs, reminder-only text and punctuation.
            pos=0;nonempty=0
            for ordinal,text in enumerate(rules_text.split("\n"),1):
                key=clean_reminder(text)[1]
                context=None
                if key and not bound:
                    queue=probes.get((oid,face["name"],key))
                    if not queue:raise ValueError(f"missing isolated paragraph {oid}, {face['name']}, {key!r}")
                    probe,context=queue.popleft()
                    scope="compiled as an isolated paragraph" if probe=="compiled" else "isolated paragraph rejected; residual wording retained"
                    nonempty+=1
                elif not key:
                    probe="no rules text" if not text else "reminder or whitespace only"
                    scope="no printed executable instruction in this paragraph"
                else:
                    probe="not isolated"
                    scope="in whole-card game receipt; individual branch not isolated" if meta["support"]=="Fully supported" else "in executable card definition; individual branch not verified"
                p+=1
                end=pos+len(text)
                db.execute("INSERT INTO paragraph VALUES(?,?,?,?,?,?,?,?)",(p,f,ordinal,pos,end,text,probe,context))
                rid=ledger.paragraph(cid,f,p,pos,text,face,scope,probe=="unmatched")
                if probe=="unmatched":observed_gaps.add(rid)
                counts[probe]+=1
                pos=end+1
            if not nonempty and not bound:
                queue=probes.get((oid,face["name"],""))
                if not queue or queue.popleft()[0]!="empty":raise ValueError(f"missing empty paragraph receipt {oid}")
        if not bound:
            # These are the audit's structural diagnostics. The remaining text families overlap the exact paragraphs.
            for gap in meta["remaining_work_families"].split(" | "):
                if gap.startswith(("Layout:","Variable, hybrid","Variable printed","Planeswalker characteristics","Subtype vocabulary")):
                    if gap.startswith("Layout:"):
                        rules=(LAYOUT_RULE[card["layout"]],) if card["layout"] in LAYOUT_RULE else ()
                    elif gap.startswith("Variable, hybrid"):rules=("107.3","107.4","601.2b")
                    elif gap.startswith("Variable printed"):rules=("208",)
                    elif gap.startswith("Planeswalker"):rules=("306","606")
                    else:rules=("205.3",)
                    rid=ledger.req("structural gap",gap,rules)
                    ledger.blocks.add((cid,rid,"processor-audit structural diagnostic; independent implementation status unverified"))
                    observed_gaps.add(rid)
            if not observed_gaps:
                rid=ledger.req("assembly gap","Combine validated clauses or support remaining characteristics")
                ledger.blocks.add((cid,rid,"whole card rejected despite no unmatched text or diagnosed structural dependency"))
        if cid%100==0:ledger.flush()
    ledger.flush()
    if cards or programs or any(probes.values()):raise ValueError("unconsumed source identities, programs or isolated probes")
    return counts

def export_query(db,path,query):
    with path.open("w",encoding="utf8",newline="") as file:
        cursor=db.execute(query);writer=csv.writer(file)
        writer.writerow([c[0] for c in cursor.description]);writer.writerows(cursor)

README="""# Magic card rule ledger

The SQLite database covers every required Oracle identity and every face of the pinned snapshot. Use `card` for card-level support, `face` and `paragraph` for the exact wording, `executable` for the full processor-emitted Rust definition, and `cr` for the pinned rule text and source lines. Rows are joined by integer IDs inside the database. Each card also exposes its permanent Oracle UUID. All source offsets are zero-based Unicode code points with an exclusive end. Oracle paragraph offsets refer to the complete face text; characteristic offsets refer to the named field. For arrays the entire source field is retained.

`card_requirements` includes shared game context, face characteristics, printed costs, ability symbols, keyword/action mentions, candidate rule topics and complete substantive paragraphs. `card_rules` links those requirements to CR numbers and labels why each link exists. Shared game context identifies contingencies such as continuous effects or Commander. Whether they apply depends on the actual format and game state. The CR inventory includes all numbered rules and subrules. A rule can have zero indexed mentions. The `parent_rule` hierarchy supports expansion from a subsection to its detailed exceptions. Glossary entries and the table of contents are excluded.

`paragraph.probe_status` distinguishes an isolated compiled paragraph, an isolated rejection, executable-card wording without an isolated test, and reminder-only or empty text. Plain printed keyword clauses and keyword-family references also link to `keyword_family_evidence` independently of whole-card certification. For Flying, Reach and Vigilance, the dated Serra Angel versus Giant Spider and a grounded blocker game is linked directly. The historical keyword evidence comes from the recorded sixth keyword wave and preserves its narrower scope and date. Meaningful prose that lacks a topic match remains in the database as a complete Oracle paragraph. A lexical match may refer to a restricted, granted or removed ability. An executable program or a receipt has its stated scope. Neither receipt membership nor a keyword match constitutes formal proof of every clause, combination, subrule or interaction. No row asserts formal verification.

`remaining_by_card` lists independent *observed* unresolved requirements of each processor-blocked card. Paragraph groups preserve type, actual mana, amounts, target wording and durations. Only whitespace, reminders and the card's own name are normalized. `blocker-priorities.csv` ranks requirements by the cards where this is the only identified blocker and separately counts all affected cards. The first count is a candidate upper bound from a static audit. Composition, interactions and new game tests can reveal more work. CR topics on a rejected paragraph do not identify which individual CR rule caused rejection. `cr-impact.csv` reports overlapping candidate associations and distinct cards, including rules with no indexed gap. Detailed identities and exact text are available by querying the indexed database. `rule-evidence.csv` shows dated keyword-family scopes and counts explicit CR references in native source and test comments. See `source_rule_reference` for file, line and cited wording. These are navigation references. `game_receipt` retains the original card-specific audit rows including observed outcomes, qualifications and source hashes where supplied.

Read one card or rule without writing SQL:

```sh
python3 scripts/card-rule-ledger.py --card "Giant Growth"
python3 scripts/card-rule-ledger.py --rule 702.9 --limit 5
```

The card query accepts an Oracle ID as well as the exact English name. The rule query totals all linked cards and separates shared game-context links, face requirements and blocked paragraphs. It includes descendants. A subrule without direct references may still be relevant through a broader heading listed in the result.

Examples with the SQLite CLI:

```sql
-- Exact source and compiler outcomes for every face.
SELECT c.name,f.face_index,f.name,p.ordinal,p.text,p.probe_status
FROM card c JOIN face f ON f.card_id=c.id JOIN paragraph p ON p.face_id=f.id
WHERE c.name='Nissa, Worldsoul Speaker' ORDER BY f.face_index,p.ordinal;

-- Card to rule, including source position and the evidence boundary on each hit.
SELECT face_index,paragraph,kind,label,field,start_char,end_char,rule_id,basis,evidence_scope
FROM card_rules WHERE name='Blackcleave Cliffs';

-- Rule to distinct blocked cards, retaining the full residual requirement.
SELECT DISTINCT b.oracle_id,b.name,b.label,b.only_identified_blocker
FROM remaining_by_card b JOIN requirement_rule r ON r.req_id=b.req_id
WHERE r.rule_id='603.1' ORDER BY b.only_identified_blocker DESC,b.name;

-- Inspect the complete executable as emitted by the current native registry.
SELECT c.name,e.definition FROM executable e JOIN card c ON c.id=e.card_id
WHERE c.name='Blackcleave Cliffs';
```

`requirement-status.csv` reports the support status for each exact reusable paragraph, cost, characteristic, gap and topic. Its counts deduplicate repeated occurrences on the same card. A card may supply more than one kind of evidence. Each row includes representative IDs and a game receipt where available. A supported context covers the conditions exercised by the cited card games. Partial contexts identify executable or isolated coverage and can also include rejected cases. Missing from the current processor means that the observed exact pattern or structural requirement has no successful current compilation in the indexed evidence. General topics and shared rules without direct evidence remain unassessed.

Regenerate with `python3 scripts/card-rule-ledger.py`. The summary records exact input hashes, results and verification checks. After a receipt-only change, run the same command with `--refresh-evidence`. Changes to clause probes, Oracle sources, native definitions or the CR require a rebuild.
"""

def source_references(db,rules):
    """Locate explicit CR citations in native source and tests. A comment alone establishes no implementation."""
    clause = re.compile(r"\b(?:CR|[Rr]ules?)\s+(\d{3}(?:\.\d+[a-z]?)?(?:\s*(?:,|and|through|[–-])\s*\d{3}(?:\.\d+[a-z]?)?)*)")
    number = re.compile(r"\b\d{3}(?:\.\d+[a-z]?)?")
    hashes = {}
    for base in ("crates/engine/src","crates/engine/tests","crates/card_programs/src","crates/card_programs/tests"):
        for path in sorted((ROOT/base).rglob("*.rs")):
            relative=str(path.relative_to(ROOT))
            lines=path.read_text().splitlines()
            rows=[]
            role="test source citation" if "tests" in path.parts or path.stem.endswith("_tests") else "native source citation"
            for index,line in enumerate(lines,1):
                if "//" not in line:continue
                ids={item for group in clause.findall(line) for item in number.findall(group) if item in rules}
                for rid in ids:rows.append((rid,relative,index,role,line.strip()))
            if rows:
                db.executemany("INSERT INTO source_rule_reference VALUES(?,?,?,?,?)",rows)
                hashes[relative]=digest(path)
    return hashes


def detailed_receipts(db,folder):
    log=(folder/"verified-games.log").read_text()
    for cid,evidence in db.execute("SELECT id,evidence FROM card WHERE evidence LIKE '%[verified-games.log:%'").fetchall():
        match=re.search(r"\[verified-games\.log: ([^\]]+)\]",evidence)
        if not match or match.group(1)+" ... ok" not in log:raise ValueError(f"missing joined game witness for {cid}")
        db.execute("INSERT INTO game_receipt VALUES(?,?,?,?)",(cid,"verified-games.log","passed identified test; scope in original receipt",evidence))
    for filename in ("ordinary-cards.tsv","ordinary-spells.tsv"):
        for row in table(folder/filename,"\t"):
            result=db.execute("SELECT id,name,semantic_sha256 FROM card WHERE oracle_id=?",(row["oracle_id"],)).fetchone()
            if result is None or result[1]!=row["name"] or (row.get("semantic_sha256") and result[2]!=row["semantic_sha256"]):
                raise ValueError(f"Unmatched card game receipt: {row['oracle_id']}")
            db.execute("INSERT INTO game_receipt VALUES(?,?,?,?)",(result[0],filename,row.get("status","verified"),compact(row)))


def build(args,temp):
    paths=[args.input_dir/s for s in ("cards.csv","ai-review-input.jsonl","clauses.tsv","programs.jsonl","summary.json","ordinary-cards.tsv","ordinary-spells.tsv","verified-games.log","card-games.log")]
    paths.extend([args.rules,args.rulebook,Path(__file__).resolve()])
    hashes={str(path):digest(path) for path in paths}
    snapshot=temp/"input"
    snapshot.mkdir()
    for path in paths:
        if path.parent==args.input_dir:
            saved=snapshot/path.name
            shutil.copyfile(path,saved)
            if digest(saved)!=hashes[str(path)]:raise ValueError(f"source changed during capture: {path}")
    rules=parse_rules(args.rules)
    db=sqlite3.connect(temp/"ledger.sqlite3")
    db.executescript("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;"+SCHEMA)
    db.executemany("INSERT INTO cr VALUES(?,?,?,?,?)",rules.values())
    for row in table(args.rulebook):
        db.execute("INSERT INTO historical_evidence VALUES(?,?,?)",(row["rule"],str(args.rulebook),compact(row)))
    ledger=Ledger(db,rules)
    for label,rids in GLOBAL.items():
        req=ledger.req("global game context",label,rids,"shared game dependency; application depends on game state/format")
        db.execute("INSERT INTO global_requirement VALUES(?,?)",(req,"Commander games only" if label.startswith("Commander") else "all"))
    source_hashes=source_references(db,rules)
    count=write_cards(ledger,snapshot)
    detailed_receipts(db,snapshot)
    db.executescript("""
        CREATE INDEX refs_by_rule ON requirement_rule(rule_id,req_id);
        CREATE INDEX occurrence_by_face ON occurrence(face_id,req_id);
        CREATE INDEX occurrence_by_requirement ON occurrence(req_id,face_id);
        CREATE INDEX gaps_by_requirement ON blocker(req_id,card_id);
        CREATE INDEX face_by_name ON face(name);
        CREATE TEMP TABLE bcount AS SELECT card_id,count(*) AS n FROM blocker GROUP BY card_id;
        CREATE UNIQUE INDEX bcount_i ON bcount(card_id);
        CREATE TEMP TABLE rule_gaps AS SELECT DISTINCT m.rule_id,b.card_id,n.n
            FROM blocker b JOIN requirement_rule m ON b.req_id=m.req_id JOIN bcount n ON n.card_id=b.card_id;
        CREATE INDEX rule_gaps_i ON rule_gaps(rule_id);
    """)
    export_query(db,temp/"blocker-priorities.csv","""
        SELECT r.id AS req_id,r.kind,r.label,count(*) AS all_affected_cards,
        sum(n.n=1) AS only_identified_requirement,count(*)-sum(n.n=1) AS overlap_cards
        FROM blocker b JOIN bcount n USING(card_id) JOIN requirement r ON r.id=b.req_id
        GROUP BY r.id ORDER BY only_identified_requirement DESC,all_affected_cards DESC,r.kind,r.label""")
    export_query(db,temp/"cr-impact.csv","""
        WITH t AS (SELECT rule_id,count(*) AS cnt,sum(n=1) AS sole FROM rule_gaps GROUP BY rule_id)
        SELECT c.rule_id,c.parent_rule,c.source_line,c.heading,coalesce(cnt,0) AS blocked_cards_with_this_topic,
        coalesce(sole,0) AS cards_with_one_observed_requirement_linked_to_topic,
        coalesce(cnt-sole,0) AS cards_with_multiple_requirements FROM cr c LEFT JOIN t USING(rule_id)
        ORDER BY blocked_cards_with_this_topic DESC,c.source_line""")
    summarized=requirement_status(db,temp,snapshot)
    def one(q):return db.execute(q).fetchone()[0]
    def ensure(condition,reason):
        if not condition:raise ValueError(reason)
    verify={
        "sqlite_integrity":one("PRAGMA integrity_check"),
        "dangling_foreign_keys":len(db.execute("PRAGMA foreign_key_check").fetchall()),
        "incorrect_paragraph_spans":one("SELECT count(*) FROM paragraph p JOIN face f ON f.id=p.face_id WHERE substr(f.oracle_text,p.start_char+1,p.end_char-p.start_char)!=p.text"),
        "incorrect_occurrence_spans":one("SELECT count(*) FROM occurrence o JOIN paragraph p ON p.id=o.paragraph_id WHERE o.face_id!=p.face_id OR o.start_char<p.start_char OR o.end_char>p.end_char OR o.start_char>o.end_char"),
        "cards_missing_faces":one("SELECT count(*) FROM card c WHERE NOT EXISTS(SELECT 1 FROM face f WHERE f.card_id=c.id)"),
        "reconstructed_face_text_mismatches":one("""SELECT count(*) FROM face f WHERE oracle_text!=(SELECT group_concat(text,char(10))
            FROM (SELECT text FROM paragraph WHERE face_id=f.id ORDER BY ordinal))"""),
        "blocked_cards_without_requirements":one("SELECT count(*) FROM card c LEFT JOIN bcount b ON b.card_id=c.id WHERE c.processor='Blocked' AND b.card_id IS NULL"),
    }
    ensure(verify["sqlite_integrity"]=="ok" and all(not value for key,value in verify.items() if key!="sqlite_integrity"),verify)
    totals={t:one("SELECT count(*) FROM "+t) for t in ("card","face","paragraph","executable","requirement","occurrence","requirement_rule","cr","historical_evidence","source_rule_reference","game_receipt","blocker")}
    expected=json.loads((snapshot/"summary.json").read_text())
    ensure(totals["card"]==expected["required"]==32155 and totals["executable"]==expected["current_executable"],"wrong audit universe")
    ensure(dict(db.execute("SELECT support,count(*) FROM card GROUP BY support"))==expected["status"],"receipt counts disagree with snapshot summary")
    shutil.rmtree(snapshot)
    summary=dict(totals=totals,requirement_evidence_status=summarized,card_support=dict(db.execute("SELECT support,count(*) FROM card GROUP BY support")),
                 paragraph_evidence=dict(count),input_sha256=hashes,cited_source_sha256=source_hashes,verification=verify,
                 unresolved_card_count=one("SELECT count(*) FROM bcount"),
                 cards_with_one_identified_blocker=one("SELECT count(*) FROM bcount WHERE n=1"),
                 rules_revision="2026-09-25")
    db.commit();db.close()
    (temp/"summary.json").write_text(json.dumps(summary,indent=2,ensure_ascii=False)+"\n")
    (temp/"README.md").write_text(README,encoding="utf8")
    return summary

def keyword_scope(db,source_folder):
    """Attach tested keyword forms to plain printed keyword lines without certifying granted, quoted or modified forms."""
    db.executescript("""
        DROP TABLE IF EXISTS requirement_family;
        DROP TABLE IF EXISTS keyword_family_evidence;
        CREATE TABLE keyword_family_evidence(rule_id TEXT PRIMARY KEY REFERENCES cr,keyword TEXT,reported_scope TEXT,
            covered_form TEXT,historical_source TEXT,focused_game TEXT);
        CREATE TABLE requirement_family(req_id INTEGER REFERENCES requirement,rule_id TEXT REFERENCES keyword_family_evidence,
            relationship TEXT,PRIMARY KEY(req_id,rule_id)) WITHOUT ROWID;
    """)
    test="serra_angel_stays_untapped_and_only_giant_spider_can_block_it"
    log=(source_folder/"card-games.log").read_text().splitlines()
    line=next(n for n,text in enumerate(log,1) if text.startswith("test ") and text.endswith(test+" ... ok"))
    current={
        "Flying":"Serra Angel attacked; a creature without flying or reach could not block her.",
        "Reach":"Giant Spider could legally block Serra Angel and the blocking declaration succeeded.",
        "Vigilance":"Serra Angel attacked and remained untapped.",
    }
    recognized={}
    for rule,location,raw in db.execute("SELECT * FROM historical_evidence").fetchall():
        evidence=json.loads(raw);name=evidence["keyword"]
        verified=current.get(name)
        if verified:verified+=f" [card-games.log:{line}, {test}]"
        db.execute("INSERT INTO keyword_family_evidence VALUES(?,?,?,?,?,?)",(rule,name,evidence["scope"],evidence["implemented_form"],location,verified))
        recognized[name.casefold()]=rule
    records=[]
    for rid,kind,label in db.execute("SELECT id,kind,label FROM requirement WHERE kind IN ('Oracle paragraph','keyword or action mention')"):
        if kind=="keyword or action mention":
            direct=recognized.get(label.casefold())
            if direct:records.append((rid,direct,"keyword family reference; role in source varies"))
            continue
        parts=label.partition(" | ")[2].strip().casefold().split(", ")
        if all(part in recognized for part in parts):
            records.extend((rid,rule,"plain printed keyword or list of plain keywords") for rule in {recognized[part] for part in parts})
    db.executemany("INSERT INTO requirement_family VALUES(?,?,?)",records)
    db.executescript("""
        UPDATE requirement_status AS s
        SET status=CASE WHEN kind='keyword or action mention' THEN 'Partial'
                        WHEN cards_with_unmatched_context>0 THEN 'Partial'
                        WHEN cards_with_complete_game_receipt>0 OR EXISTS (
                            SELECT 1 FROM requirement_family f JOIN keyword_family_evidence e USING(rule_id)
                            WHERE f.req_id=s.req_id AND e.focused_game IS NOT NULL) THEN 'Supported in tested contexts'
                        ELSE 'Partial' END,
            evidence_boundary=CASE WHEN kind='keyword or action mention'
                THEN 'Keyword family has tested forms. A reference on this card may grant, remove or restrict the keyword; inspect exact wording.'
                ELSE 'Printed keyword form has family-level game evidence. Any unmatched contexts need card composition, admission and verification.' END
        WHERE EXISTS (SELECT 1 FROM requirement_family f WHERE f.req_id=s.req_id)
          AND NOT EXISTS (SELECT 1 FROM requirement_family f JOIN keyword_family_evidence e USING(rule_id)
                WHERE f.req_id=s.req_id AND e.reported_scope NOT LIKE 'Tested forms%' AND e.focused_game IS NULL);
    """)


def requirement_status(db,folder,source_folder):
    """Summarize observed contexts by distinct Oracle identity, without treating a topic as an implementation."""
    db.executescript("""
        DROP TABLE IF EXISTS temp.context_by_card;
        DROP TABLE IF EXISTS requirement_status;
        CREATE TEMP TABLE context_by_card AS
        SELECT req_id,card_id,max(verified) verified,max(executable) executable,max(isolated) isolated,
               max(unmatched) unmatched,max(prior) prior
        FROM (
          SELECT o.req_id,f.card_id,c.support='Fully supported' verified,c.processor='Executable' executable,
                 coalesce(p.probe_status='compiled',0) isolated,coalesce(p.probe_status='unmatched',0) unmatched,
                 c.prior_scopes!='' prior
            FROM occurrence o JOIN face f ON f.id=o.face_id JOIN card c ON c.id=f.card_id LEFT JOIN paragraph p ON p.id=o.paragraph_id
          UNION ALL
          SELECT b.req_id,b.card_id,0,0,0,1,c.prior_scopes!='' FROM blocker b JOIN card c ON c.id=b.card_id
        ) hits GROUP BY req_id,card_id;
        CREATE TABLE requirement_status AS
        SELECT r.id AS req_id,r.kind,r.label,
          CASE WHEN r.kind IN ('global game context','prose topic','keyword or action mention') THEN 'Unassessed'
               WHEN sum(x.verified)>0 AND sum(x.unmatched)=0 THEN 'Supported in tested contexts'
               WHEN sum(x.executable)>0 OR sum(x.isolated)>0 THEN 'Partial'
               WHEN sum(x.unmatched)>0 THEN 'Missing from current processor in observed contexts'
               ELSE 'Unassessed' END AS status,
          CASE WHEN r.kind='global game context' THEN 'Shared game context; individual application depends on the game and format'
               WHEN r.kind IN ('prose topic','keyword or action mention') THEN 'Context counts come from linked source paragraphs; mention or grant alone cannot establish support for the topic'
               WHEN sum(x.verified)>0 AND sum(x.unmatched)>0 THEN 'Seen both on game-tested cards and in rejected contexts'
               WHEN sum(x.verified)>0 THEN 'Observed on cards with complete printed-behavior receipts; see receipts for exercised conditions and limits'
               WHEN sum(x.executable)>0 THEN 'Occurs in an emitted whole-card program; this requirement needs behavioral verification'
               WHEN sum(x.isolated)>0 THEN 'Compiled and validated in isolation; remaining composition or game testing is open'
               WHEN sum(x.unmatched)>0 THEN 'Unmatched paragraph or structural gap; missing definition and missing native rule require separate diagnosis'
               ELSE 'Printed source alone; component behavior has not been established' END AS evidence_boundary,
          count(x.card_id) AS cards_with_source_or_diagnostic,
          coalesce(sum(x.verified),0) AS cards_with_complete_game_receipt,
          coalesce(sum(x.executable),0) AS cards_with_current_program,
          coalesce(sum(x.isolated),0) AS cards_with_isolated_compiled_context,
          coalesce(sum(x.unmatched),0) AS cards_with_unmatched_context,
          coalesce(sum(x.prior),0) AS cards_with_earlier_profile_scope,
          min(CASE WHEN x.verified THEN c.oracle_id END) AS representative_game_oracle_id,
          min(CASE WHEN x.executable THEN c.oracle_id END) AS representative_program_oracle_id,
          min(CASE WHEN x.isolated THEN c.oracle_id END) AS representative_isolated_oracle_id,
          min(CASE WHEN x.unmatched THEN c.oracle_id END) AS representative_unmatched_oracle_id,
          min(CASE WHEN x.prior THEN c.oracle_id END) AS representative_prior_oracle_id
        FROM requirement r LEFT JOIN context_by_card x ON x.req_id=r.id LEFT JOIN card c ON c.id=x.card_id GROUP BY r.id;
        CREATE UNIQUE INDEX status_by_requirement ON requirement_status(req_id);
    """)
    keyword_scope(db,source_folder)
    export_query(db,folder/"requirement-status.csv","""
        SELECT s.*,g.name AS representative_game_card,g.evidence AS representative_game_receipt,
            h.prior_scopes AS representative_earlier_profile_scope,
            (SELECT group_concat(e.rule_id || ' ' || e.keyword || ': ' || e.reported_scope || ' (' || e.covered_form || ')',' | ')
               FROM requirement_family f JOIN keyword_family_evidence e USING(rule_id) WHERE f.req_id=s.req_id) AS keyword_family_evidence,
            (SELECT group_concat(e.focused_game,' | ') FROM requirement_family f JOIN keyword_family_evidence e USING(rule_id)
               WHERE f.req_id=s.req_id) AS focused_keyword_games
        FROM requirement_status s LEFT JOIN card g ON g.oracle_id=s.representative_game_oracle_id
        LEFT JOIN card h ON h.oracle_id=s.representative_prior_oracle_id
        ORDER BY s.kind,s.cards_with_source_or_diagnostic DESC,s.label""")
    export_query(db,folder/"rule-evidence.csv","""
        SELECT c.rule_id,c.parent_rule,c.heading,c.source_line,
            CASE WHEN h.evidence_json IS NULL THEN 'Unassessed at CR level'
                ELSE json_extract(h.evidence_json,'$.scope') END AS rule_evidence_status,
            (SELECT COUNT(*) FROM source_rule_reference s WHERE s.rule_id=c.rule_id AND role='native source citation') AS native_source_citations,
            (SELECT COUNT(*) FROM source_rule_reference s WHERE s.rule_id=c.rule_id AND role='test source citation') AS test_source_citations,
            h.evidence_json AS historically_recorded_keyword_scope,e.focused_game AS focused_game_receipt
        FROM cr c LEFT JOIN historical_evidence h ON h.rule_id=c.rule_id LEFT JOIN keyword_family_evidence e ON e.rule_id=c.rule_id ORDER BY c.source_line""")
    db.execute("DROP TABLE temp.context_by_card")
    return dict(db.execute("SELECT status,count(*) FROM requirement_status GROUP BY status"))


def refresh_evidence(args,out):
    """Refresh card-level receipts without reparsing an unchanged CR, source corpus or engine registry."""
    summary_path=out/"summary.json"
    summary=json.loads(summary_path.read_text())
    paths=[args.input_dir/s for s in ("cards.csv","summary.json","ordinary-cards.tsv","ordinary-spells.tsv","verified-games.log","card-games.log")]
    stable=[args.input_dir/s for s in ("ai-review-input.jsonl","programs.jsonl","clauses.tsv")]+[args.rules,args.rulebook]
    for path in stable:
        if summary["input_sha256"].get(str(path))!=digest(path):
            raise ValueError(f"Requirement source changed. Rebuild without --refresh-evidence: {path}")
    hashes={str(path):digest(path) for path in paths}
    expected=json.loads((args.input_dir/"summary.json").read_text())
    db=sqlite3.connect(out/"ledger.sqlite3")
    db.execute("PRAGMA foreign_keys=ON")
    try:
        db.execute("BEGIN IMMEDIATE")
        previous={row[0]:row[1:] for row in db.execute("SELECT oracle_id,id,name,support,processor,semantic_sha256 FROM card")}
        for row in table(args.input_dir/"cards.csv"):
            oid=row["oracle_id"];cid,name,old,processor,semantic=previous.pop(oid)
            if (name,processor,semantic)!=(row["name"],row["current_processor"],row["semantic_sha256"]):
                raise ValueError(f"Registry identity changed. Rebuild: {oid}")
            db.execute("UPDATE card SET support=?,reason=?,evidence=?,prior_scopes=?,remaining_work=? WHERE id=?",
                       (row["support"],row["reason"],row["behavior_evidence"],row["earlier_profile_scopes"],row["remaining_work_families"],cid))
            if old!=row["support"] and processor=="Executable":
                scope="in whole-card game receipt; individual branch not isolated" if row["support"]=="Fully supported" else "in executable card definition; individual branch not verified"
                db.execute("""UPDATE occurrence SET evidence_scope=? WHERE face_id IN (SELECT id FROM face WHERE card_id=?)
                    AND paragraph_id IN (SELECT p.id FROM face f JOIN paragraph p ON p.face_id=f.id WHERE f.card_id=? AND p.probe_status='not isolated')""",(scope,cid,cid))
        if previous:raise ValueError("receipt file omits previously indexed identities")
        counts=dict(db.execute("SELECT support,count(*) FROM card GROUP BY support"))
        if expected["status"]!=counts:raise ValueError("receipt counts disagree with snapshot")
        db.execute("DELETE FROM game_receipt")
        detailed_receipts(db,args.input_dir)
        if hashes!={str(path):digest(path) for path in paths}:raise ValueError("receipt inputs moved; retry refresh")
        receipt_count=db.execute("SELECT COUNT(*) FROM game_receipt").fetchone()[0]
        if db.execute("PRAGMA foreign_key_check").fetchone():raise ValueError("broken receipt references")
        db.commit()
        statuses=requirement_status(db,out,args.input_dir)
        db.commit()
    finally:
        db.close()
    summary["requirement_evidence_status"]=statuses
    summary["verification"].pop("formal_proof_records",None)
    summary["totals"]["game_receipt"]=receipt_count
    (out/"README.md").write_text(README,encoding="utf8")
    summary["input_sha256"].update(hashes)
    summary["receipt_refresh_script_sha256"]=digest(Path(__file__))
    summary["card_support"]=counts
    with tempfile.NamedTemporaryFile(dir=out,mode="w",delete=False,encoding="utf8") as file:
        json.dump(summary,file,indent=2,ensure_ascii=False);file.write("\n")
    Path(file.name).replace(summary_path)
    return summary


def lookup(out,args):
    """Read a complete card or a CR topic with scoped reverse counts, using an immutable data connection."""
    path=(out/"ledger.sqlite3").resolve()
    if not path.exists():raise SystemExit(f"No generated ledger at {path}")
    db=sqlite3.connect(path.as_uri()+"?mode=ro",uri=True)
    db.row_factory=sqlite3.Row
    try:
        def rows(sql,params=()):
            return [dict(r) for r in db.execute(sql,params)]
        if args.card:
            cards=rows("SELECT * FROM card WHERE oracle_id=? OR name=? COLLATE NOCASE ORDER BY name",(args.card,args.card))
            if not cards:raise SystemExit(f"No exact card name or Oracle ID: {args.card}")
            for card in cards:
                cid=card["id"]
                source=rows("SELECT * FROM face WHERE card_id=? ORDER BY face_index",(cid,))
                for face in source:
                    face["attributes"]=json.loads(face["attributes"])
                    face["paragraphs"]=rows("SELECT ordinal,start_char,end_char,text,probe_status,probe_context FROM paragraph WHERE face_id=? ORDER BY ordinal",(face["id"],))
                card["source_attributes"]=json.loads(card["source_attributes"])
                card["faces"]=source
                occurrences=rows("""SELECT f.face_index,p.ordinal AS paragraph,r.kind,r.label,o.field,o.start_char,o.end_char,o.evidence_scope,
                    o.req_id FROM face f JOIN occurrence o ON f.id=o.face_id JOIN requirement r ON r.id=o.req_id
                    LEFT JOIN paragraph p ON p.id=o.paragraph_id WHERE f.card_id=? ORDER BY f.face_index,o.id""",(cid,))
                for item in occurrences:
                    req=item["req_id"]
                    item["cross_card_status"]=rows("SELECT status,evidence_boundary,cards_with_complete_game_receipt,cards_with_current_program,cards_with_isolated_compiled_context,cards_with_unmatched_context FROM requirement_status WHERE req_id=?",(req,))[0]
                    item["keyword_family_evidence"]=rows("SELECT f.relationship,e.* FROM requirement_family f JOIN keyword_family_evidence e USING(rule_id) WHERE req_id=?",(req,))
                    item["cr"]=rows("SELECT rule_id,basis FROM requirement_rule WHERE req_id=? ORDER BY rule_id,basis",(item.pop("req_id"),))
                card["requirements"]=occurrences
                card["shared_game_context"]=rows("""SELECT r.label,g.applicability,m.rule_id FROM global_requirement g JOIN requirement r ON r.id=g.req_id
                    JOIN requirement_rule m ON m.req_id=r.id WHERE g.applicability='all' OR ? LIKE '%commander%' ORDER BY r.label,m.rule_id""",(card["formats"],))
                card["observed_gaps"]=rows("SELECT kind,label,basis,only_identified_blocker FROM remaining_by_card WHERE oracle_id=?",(card["oracle_id"],))
                card["game_receipts"]=rows("SELECT source_file,observed_status,source_row FROM game_receipt WHERE card_id=? ORDER BY source_file",(cid,))
                program=rows("SELECT definition,details FROM executable WHERE card_id=?",(cid,))
                if program:program[0]["details"]=json.loads(program[0]["details"])
                card["current_rust_program"]=program[0] if program else None
            return {"cards":cards,"proof_scope":"Recorded source, processor and game evidence; no formal proof."}
        rid=args.rule.removeprefix("CR ").strip().rstrip(".")
        root=rows("SELECT * FROM cr WHERE rule_id=?",(rid,))
        if not root:raise SystemExit(f"No numbered rule in the pinned CR: {rid}")
        # A reference to a parent section never proves applicability of every subrule.
        db.execute("CREATE TEMP TABLE desc_rule(rule_id TEXT PRIMARY KEY) WITHOUT ROWID")
        db.execute("""INSERT INTO desc_rule WITH RECURSIVE d(rid) AS (SELECT ? UNION ALL SELECT rule_id FROM cr JOIN d ON parent_rule=rid)
            SELECT rid FROM d""",(rid,))
        db.executescript("""
            CREATE TEMP TABLE rel(req_id INTEGER PRIMARY KEY);
            INSERT OR IGNORE INTO rel SELECT req_id FROM desc_rule d CROSS JOIN requirement_rule m ON m.rule_id=d.rule_id;
            CREATE TEMP TABLE source_cards(id INTEGER PRIMARY KEY);
            INSERT OR IGNORE INTO source_cards SELECT f.card_id FROM rel r CROSS JOIN occurrence o ON o.req_id=r.req_id CROSS JOIN face f ON f.id=o.face_id;
            CREATE TEMP TABLE global_cards(id INTEGER PRIMARY KEY);
            INSERT OR IGNORE INTO global_cards SELECT c.id FROM rel r CROSS JOIN global_requirement g ON g.req_id=r.req_id CROSS JOIN card c
              WHERE g.applicability='all' OR c.formats LIKE '%commander%';
            CREATE TEMP TABLE gaps(id INTEGER PRIMARY KEY);
            INSERT OR IGNORE INTO gaps SELECT b.card_id FROM rel r CROSS JOIN blocker b ON b.req_id=r.req_id;
            CREATE TEMP TABLE sole(id INTEGER PRIMARY KEY);
            INSERT INTO sole SELECT g.id FROM gaps g WHERE (SELECT count(*) FROM blocker WHERE card_id=g.id)=1;
            CREATE TEMP TABLE affected(id INTEGER PRIMARY KEY);
            INSERT INTO affected SELECT id FROM source_cards UNION SELECT id FROM global_cards UNION SELECT id FROM gaps;
        """)
        counts=rows("""SELECT (SELECT count(*) FROM affected) AS indexed_cards,
            (SELECT count(*) FROM source_cards) AS with_face_level_requirements,
            (SELECT count(*) FROM global_cards) AS shared_context_cards,(SELECT count(*) FROM gaps) AS with_linked_unresolved_requirement,
            (SELECT count(*) FROM sole) AS with_one_observed_requirement_linked_here""")[0]
        status=rows("""SELECT c.processor,c.support,count(*) AS cards FROM affected a JOIN card c ON c.id=a.id
            GROUP BY c.processor,c.support ORDER BY c.processor,c.support""")
        examples=rows("""SELECT c.oracle_id,c.name,c.processor,c.support,c.prior_scopes,
            s.id IS NOT NULL AS source_reference,g.id IS NOT NULL AS linked_gap,u.id IS NOT NULL AS sole_observed_requirement
            FROM affected a JOIN card c ON c.id=a.id LEFT JOIN source_cards s ON s.id=c.id
            LEFT JOIN gaps g ON g.id=c.id LEFT JOIN sole u ON u.id=c.id
            ORDER BY sole_observed_requirement DESC,linked_gap DESC,source_reference DESC,c.name COLLATE NOCASE,c.oracle_id LIMIT ?""",(args.limit,))
        hierarchy=[rid]
        while True:
            row=db.execute("SELECT parent_rule FROM cr WHERE rule_id=?",(hierarchy[-1],)).fetchone()
            if not row or not row[0]:break
            hierarchy.append(row[0])
        relatives=rows("SELECT c.rule_id,c.heading,c.source_line FROM cr c JOIN desc_rule d USING(rule_id) ORDER BY c.source_line")
        cited=rows("SELECT s.* FROM source_rule_reference s JOIN desc_rule d USING(rule_id) ORDER BY s.role,s.source_path,s.source_line LIMIT ?",(args.limit,))
        historical=rows("SELECT h.*,e.focused_game FROM historical_evidence h JOIN desc_rule d USING(rule_id) JOIN keyword_family_evidence e USING(rule_id) ORDER BY h.rule_id")
        for item in historical:item["evidence_json"]=json.loads(item["evidence_json"])
        return {"rule":root[0],"ancestors_for_broader_topics":hierarchy[1:],"included_rules":relatives,
                "count_scope":"Direct index references to this rule or its descendants. References only to broader sections are excluded.",
                "counts":counts,"support_counts":status,"example_limit":args.limit,"cards":examples,
                "source_citation_examples":cited,"historical_keyword_evidence":historical,
                "proof_scope":"A reference on a blocked paragraph identifies a topic. Individual CR rule implementation or sufficiency is unproven."}
    finally:
        db.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-dir",type=Path,default=DEFAULT)
    parser.add_argument("--rules",type=Path,default=RULES)
    parser.add_argument("--rulebook",type=Path,default=WAVE)
    parser.add_argument("--output-dir",type=Path)
    parser.add_argument("--refresh-evidence",action="store_true",help="update card-specific receipts after re-running the same catalogue and registry")
    query=parser.add_mutually_exclusive_group()
    query.add_argument("--card",help="read a card by exact English name or Oracle ID")
    query.add_argument("--rule",help="read a CR number and its reverse index, including descendant rules")
    parser.add_argument("--limit",type=int,default=15,help="maximum example cards and source citations for --rule; counts are always complete")
    args=parser.parse_args()
    out=args.output_dir or args.input_dir/"rule-ledger"
    if args.card or args.rule:
        if args.limit < 0:parser.error("--limit must be nonnegative")
        print(json.dumps(lookup(out,args),indent=2,ensure_ascii=False))
        return
    out.mkdir(exist_ok=True,parents=True)
    if args.refresh_evidence:
        # The collector issues DDL while materializing aggregate tables. Refresh a private
        # copy so a failed or interrupted run cannot expose partially regenerated evidence.
        with tempfile.TemporaryDirectory(prefix=".receipt-refresh-",dir=out) as staging:
            temp=Path(staging)
            for name in ("ledger.sqlite3","summary.json"):
                shutil.copyfile(out/name,temp/name)
            result=refresh_evidence(args,temp)
            for file in sorted(temp.iterdir(),key=lambda x:x.name=="summary.json"):
                file.replace(out/file.name)
        print(json.dumps(result["card_support"],indent=2))
        return
    with tempfile.TemporaryDirectory(prefix=".ledger-build-",dir=out) as staging:
        temp=Path(staging)
        result=build(args,temp)
        for file in sorted(temp.iterdir(),key=lambda x:x.name=="summary.json"):
            file.replace(out/file.name)
    print(json.dumps({k:result[k] for k in ("totals","verification","cards_with_one_identified_blocker")},indent=2))

if __name__=="__main__":
    main()
