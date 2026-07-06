# Store form for one-shot retrieval

Normative reference for curation prompts and store audits. Derived from the
hand-curation study and the question-level sufficiency assessment on the dev/test
conversations, co-designed with the retrieval mechanism below. When a curation
prompt and this document disagree, this document wins.

## The retrieval contract

The store is read by a single invocation — no agentic search, no second chance:

``` bash
iwe retrieve --lexical "$QUESTION" --limit 5 \
  --expand-references --expand-included-by -f json
```

Three stages, each imposing one requirement on the store:

| stage | mechanism | requirement on the store |
| --- | --- | --- |
| search | BM25 over `title + body`, top-N seeds | the distinctive vocabulary of every fact must sit concentrated on a findable page |
| expand | one hop along edge lists | everything search cannot find must be one link away from something it can |
| budget | token caps, periphery trimmed first | pages small and self-contained; nothing essential arrives late |

## The principle

**Every page is either search bait or an expansion target, and links bind them.**
Search finds pages by the question's distinctive terms; expansion climbs from
those pages to the standing knowledge; the budget holds because both kinds of
page are small.

## Page kinds

**Episode pages** — search bait. One page per session or event.

- Hold the verbatim specifics: names, titles of works, quantities, durations,
  prices, quoted phrases, photo captions. Questions quote these terms and BM25
  matches stems, not synonyms — a paraphrased fact is unfindable and wrong at
  once ("9 days" written as "about a week" fails twice).
- One episode per page. Merging sessions dilutes term concentration and blurs
  dates.
- A bullet list, one fact per bullet, each bullet carrying its occurrence date.

**Hub pages** — expansion targets. One per entity: person, relationship (one
page for the pair), project, or a topic with its own shared history.

- Standing state: identity facts, preferences as stated, health — current truth,
  with one line of history when it changes ("previously X; changed as of
  <date>").
- A **timeline of dated instance lines**, each linking its episode page:
  "Won her first rowing race the week before 21 May 2024
  ([21 May 2024](k240521))."
- Hubs exist because search can never find them for entity questions: the
  entity's name appears on every page, so it has no discriminating power. Hubs
  arrive by expansion only.
- Hubs answer what no episode holds: aggregations ("how many times…") and
  current state ("what is her favorite…").

## The link contract

The structure is the pin — no caller-supplied must-include keys exist, so
reachability is the store's own responsibility:

1. Every episode bullet that touches an entity links its hub: `[Ada](ada)`.
   → `--expand-references 1` from any seed reaches every relevant hub.
2. Every hub timeline line links its episode.
   → a hub, once present, brings its evidence; the hub is also reachable from
   its episodes via `referencedBy`.
3. An index page, if kept, stays **out of the expansion paths**: episodes and
   hubs never link to it, and retrieval excludes it. A page that links
   everything turns any inbound expansion into the whole store.

Invariant to audit: *every fact is on a page findable by the words a question
about it would use, or one hop from one that is.*

## Writing rules

1. **Verbatim specifics, never vaguer words.** Numbers, durations, counts,
   names, nicknames, titles of movies/books/games/dishes, place names. If the
   speaker gave a quantity, the note carries the quantity.
2. **Exact words for exact-words facts.** Advice, encouragement, stated
   feelings ("thrilled and terrified"), reasons and motivations — quote or
   near-quote; keep every part of a compound reason.
3. **Dates resolved at write time.** Relative wording plus its resolution:
   "'last Friday' (17 May 2024)". Match the source's precision — a week stays a
   week; never invent day precision. Occurrence date, not session date, on
   every event bullet.
4. **Instances, never tallies.** Recurring things accumulate as separate dated
   timeline lines; a written count ("has won 3 races") goes stale and
   conflicts. Counting happens at answer time over the accumulated lines.
5. **Photos are facts.** Record what was shared and what it showed, verbatim
   from the caption. The thing shown is often the answer.
6. **Distilled means shorter, never less specific.**

## Keys, titles, frontmatter

Division of labor, fixed by how the engine works:

| element | in the BM25 index | delivered by retrieve | role |
| --- | --- | --- | --- |
| key | no | as identifier | links and identity only — **short and stable**; any scheme works |
| title | yes | yes | search bait — informative, carries topical nouns |
| body | yes | yes | the content — everything that answers questions |
| frontmatter | no | no | selectors — `kind`, `date`, participants, for filter/sort composition |

- Keys repeat in every link and every output line: long slugified keys are pure
  token cost with zero retrieval value. Key *values* carry no meaning to the
  mechanism; the format below optimizes the surfaces where keys appear alone
  (bare key lists, edge lists, file names), ranked unique → stable → short →
  legible in a list → sortable.
- **Recommended scheme.** Hubs: the entity's name, lowercase kebab (`ada`,
  `ada-and-kai`). Episodes: the ISO date, with the shortest disambiguator only
  when needed (`2024-05-21`, `2024-05-21-b`; a one-word prefix if the store has
  several dated page kinds). ISO keys make lexicographic order chronological,
  so every key-ordered listing reads as a timeline. Avoid title slugs (long,
  unstable under retitling, redundant with the indexed title) and opaque ids
  (illegible exactly where keys appear without titles).
- **The key is a primary key — use it as one.** Creation at an existing key is
  a conflict error, so a metadata-derived scheme makes identity deterministic
  and duplicates structurally impossible: the same entity or the same session
  cannot be recorded twice. This replaces the behavioral rule "search before
  you write" with an engine guarantee (create errors → update instead), makes
  re-ingestion idempotent, and makes link targets *computable* — a hub timeline
  line can link `2024-05-21` without any lookup, before the page even exists.
  Title-derived keys forfeit all three: rewording mints a fresh key and the
  store silently accumulates near-duplicates.
- **Derivation must be canonical.** The uniqueness guarantee holds only when
  the scheme leaves the writer no choices: an identity with several spellings
  defeats the collision check (a pair of names has two orderings — measured:
  `ada-and-kai` and `kai-and-ada` both got created). Pin every degree of
  freedom in the scheme itself: names in alphabetical order, dates in ISO
  form, lowercase throughout.
- **Nothing that answers a question may live only in frontmatter**: it is
  excluded from the search index and stripped from retrieve output. Dates earn
  both forms — prose in the bullet (searchable, readable) and a frontmatter
  field (filterable, sortable).

## Machine-checked form: document schemas

The page-shape half of this document is expressed as document schemas
(`docs/store-schemas/session.yaml`, `docs/store-schemas/hub.yaml`), bound in
`.iwe/config.toml`:

``` toml
[schemas.session]
match = "[0-9]*"

[schemas.hub]
match = "[a-z]*"
```

The key scheme makes the bindings disjoint by construction (every schema whose
glob matches applies — there is no first-match-wins). Calibration is the
acceptance test: the reference store must validate clean before a schema is
enforced. Calibrated 2026-07-11: the hand-built store passes with zero
violations; the best automated store shows exactly its known residual defects
(timeline sections over the 900-token positional budget on all three hubs, one
session title missing its date suffix). The hub timeline additionally carries
block rules — `blocks: [{type: bullet-list, items: {maxTokens: 100}}]` with
`additionalBlocks: false` — bullets-only, each entry under a loose
paragraph-catching cap. Two calibration lessons encoded there: a tight item
budget (40 tokens) flags the *good* store's merged-arc lines, so item budgets
are paragraph-catchers, not style enforcers (the ≤20-word style stays
editorial); and the structural bullets-only rule is what catches re-narration
— prose timelines report `required block bullet-list missing`. Violation
counts track store quality: hand-built 0, lint-era automated 22, pre-lint
automated 272 — the same ordering as their QA scores. What schemas add over flat page
budgets: **positional** token budgets (the timeline can be tight while
standing-facts sections stay free — a page-level budget cannot express this),
required/canonical section names (which also protect `iwe_query` `$append`
targeting), title-shape enforcement (the date suffix that makes titles search
bait), and heading-depth caps. Graph rules (dangling links, orphans,
near-duplicates) are not expressible as page schemas and remain lint's job.

## Sizing

- The whole store should be a fraction of its source material (the reference
  store: roughly one third of the raw transcripts).
- Episode pages ~100–300 words; hub pages bounded by pushing detail down into
  episodes — a hub is standing facts plus an index of dated one-liners
  (one bullet, ≤20 words, date-only link text). This is load-bearing for
  ranking, not just cost: a hub that re-narrates sessions becomes lexically
  similar to *every* question and steals seed slots from the episode pages
  that hold the evidence (measured: a 3k-word hub cost ~0.12 J against a lean
  store on identical questions).
- Target: top-5 seeds + one hop + hubs fit in a few thousand tokens without the
  budget trimming anything.

## Growth: when a page must split

Budgets force distillation first — an over-budget hub usually holds duplicated
episode detail, and the fix is deletion, not a new page. But a store that runs
for years eventually has hubs where everything is legitimate and the page is
still too big: a decade of timeline lines outgrows any honest budget. The
split rules, when that day comes:

- **The trigger is the budget itself.** Over budget after distillation — when
  every line left is an instance line that must be kept — split; never before.
- **Split by closed time period, never by theme.** The timeline's oldest
  closed period moves to a period page (`ada-2023`), linked from the hub.
  Thematic splits fragment the aggregation surface; period splits archive it.
- **Closed periods may be tallied.** The instances-never-tallies rule exists
  because running totals go stale — but nothing accumulates into a closed
  year. The hub keeps one immutable rollup line per archived period ("2023:
  won 4 races, moved twice ([details](ada-2023))"), so counting questions
  stay answerable at the hub, with the evidence one link away.
- **Archive pages are search bait, not expansion targets.** One-hop expansion
  from an episode seed reaches the hub but not the hub's sub-pages (expansion
  is per-seed, not transitive). That is acceptable by design: a period page
  carries distinctive dated content, so questions about that period rank it
  *directly* — search replaces the missing hop. What must stay on the hub is
  the current period and the rollup lines; what moves out must be findable by
  its own words.
- Schema/lint accommodation: period keys (`ada-2023`) get their own binding
  with an episode-sized budget; the hub budget does not grow.

## Known limits of the form

Measured residuals that the form mitigates but does not eliminate:

- **Identity-of-things-shown questions** (what a photo depicts standing in for
  the answer) — mitigated by mandatory verbatim captions; a small irreducible
  tax remains.
- **Enumeration spans** wider than the seed count — mitigated by hub
  accumulation (the span lives on one page).
- **Exact-quote answers** — mitigated by the exact-words rule; paraphrase decay
  is the curator's most common failure.

## Evidence

Same questions, same reader, same judge (conv-42, 199 questions):

| store | J | note |
| --- | --- | --- |
| this form, hand-built | 0.824 | beats agentic grep over raw transcripts (0.809) at ~1/10 the answering cost |
| automated curation that drifted from the form | 0.724 | bloated pages, paraphrase, weak link contract |

The gap between the two rows is curation adherence — the form itself is the
ceiling-setter. Prerequisite for automated adherence: the curator must be able
to keep keys short independently of titles (`iwe_create` key parameter or
equivalent).
