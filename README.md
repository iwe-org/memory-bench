# Markdown knowledge graphs as agent memory

Measures [IWE](https://github.com/iwe-org/iwe) knowledge graphs — markdown
pages joined by links — as memory for `claude -p` (Claude Code headless),
across two tracks:

- **LOCOMO** (conversational memory): an agent curates long personal
  conversations into a store, and answers are produced with that store as the
  only memory.
- **HotPotQA** (multi-hop retrieval): a document corpus is ingested
  mechanically, and the measured variable is the graph itself — what link
  structure is worth, and who has to build it.

Both tracks share the answering architecture: either an agent holding search
tools, or (the headline in both) a **single one-shot call over a retrieval
dossier** the harness assembles with `iwe retrieve`. Every model interaction —
curation, answering, judging — runs through `claude -p`; there are no direct
API calls, and sessions authenticate through the bench's own isolated profile.
Judge prompts and grading are ported from the legacy
[Mem0 LOCOMO evaluation](https://github.com/mem0ai/mem0/tree/aae5989e78/evaluation)
(binary CORRECT/WRONG "J" metric).

Headline results (sealed test sets, sonnet-4-6 answering and judging):

| track | one-shot over the store | agentic `fs` baseline | verdict |
| --- | --- | --- | --- |
| LOCOMO (652 q) | 0.7515 | 0.8113 | grep leads by 0.06; one-shot wins efficiency only |
| HotPotQA (300 q) | 0.90–0.91 | 0.9033 | parity at 2.4× lower cost, 5× lower latency |

The research narrative for the HotPotQA track is drafted in `article.md`.
Inside this repo: `results/RUNS.md` is the ledger of every measured run and
the source of final numbers; `results/curated-pilot-report.md` holds the
LOCOMO dev-set analysis; `docs/test-plan.md` (LOCOMO) and `docs/hotpot.md`
(HotPotQA) are the protocols of record; `docs/store-form.md` is the normative
LOCOMO store shape.

## Repository map

- `xtask/` — the harness. Prompts live in `xtask/prompts/` (`curate.md`,
  `consolidate.md`, `enrich.md`, `answer*.md` per arm and track,
  `full_context.md`, `judge.md`, `judge_hotpot.md`) and are compiled in — see
  Setup.
- `docs/store-form.md` — normative LOCOMO store shape (when a curation prompt
  and this document disagree, the document wins); `docs/store-schemas/` — hub
  and session page schemas; `docs/test-plan.md` — LOCOMO test-set protocol;
  `docs/hotpot.md` — HotPotQA track protocol.
- `results/` — one directory per run (JSONL records, `meta.json`,
  `summary.json`), plus `RUNS.md` and `curated-pilot-report.md`.
- `article.md` — draft research narrative for the HotPotQA track.
- `bin-pinned/` — frozen `iwe`/`iwec` builds for measured runs (`iwe-hotpot`
  is the HotPotQA-era pin).
- `workspaces*/` — generated stores (gitignored): `workspaces/` holds LOCOMO
  raw transcripts and the HotPotQA stores (`hotpot/corpus*`); curated LOCOMO
  stores live in one directory per curation prompt version
  (`workspaces-v7`, …), selected with `--workspaces`.
- `archive/` — retired stores kept for the record; `data/` — datasets
  (gitignored).

# LOCOMO track: conversational memory

An agent curates each conversation into an IWE knowledge graph — markdown
pages, links, dated event pages — question-blind and chronologically; answers
are produced with that graph as the only memory. Judged with the Mem0 prompt
verbatim (categories 1–4, adversarial category 5 excluded).

## Arms

| Arm | Memory | Answering | Measures |
| --- | --- | --- | --- |
| `curated-ctx` | curated notes only | one-shot: harness-assembled dossier inlined into the prompt, zero tools | the headline: automated curation + one-shot product retrieval |
| `fs` | raw session transcripts | agentic: `Grep`, `Glob`, `Read` | filesystem baseline (Letta-style) the headline must beat |
| `curated-fs` | curated notes only | agentic: `Grep`, `Glob`, `Read` | representation without graph tools (dev ablation) |
| `curated-q` | curated notes only | agentic: `iwe_find`, `iwe_retrieve`, `iwe_tree`, `iwe_squash`, `iwe_stats`, `iwe_query` via MCP | multi-turn agentic pipeline (former headline, superseded by `curated-ctx`) |

The `curated-ctx` dossier is one product retrieval per question — no agentic
search, no second chance:

```bash
iwe retrieve --lexical "$QUESTION" --limit 5 \
  --expand-references --expand-included-by --max-tokens 12000 -f json
```

The store shape this contract demands (search bait, expansion targets, links
binding them) is specified in `docs/store-form.md`.

The curated arms answer over a store built by `xtask curate`: one `claude -p`
session per conversation session, chronological, question-blind.
`curate --consolidate` adds an editor's pass after ingestion — one more
session with the whole store visible that reorganizes pages without adding
facts (`xtask/prompts/consolidate.md`). The raw transcript is absent from
curated workspaces — what the curator failed to write down is lost.

Additional arms from earlier rounds (`iwe`, `fs-iwe`, `curated`,
`full-context`) remain runnable; see `results/RUNS.md` for their history.

## Configuration of record

Frozen for the test set (v7.7, one-shot era; `docs/test-plan.md` has the full
protocol):

- **Curation**: `claude-haiku-4-5-20251001`, prompt `xtask/prompts/curate.md` (v7.7:
  page schemas, lint budgets, occurrence-date link texts), $3/session budget.
- **Answering**: `claude-sonnet-4-6`, one-shot over the dossier above
  (`xtask/prompts/answer_context.md`), $2/question budget.
- **Judge**: `claude-sonnet-4-6`, Mem0 prompt verbatim. Calibration: the 4-6 judge
  grades the full-context anchor at J = 0.70 (sonnet-5 judge: 0.75; published: 0.73).
  Judge strictness is a judge × answer-style interaction, not a scalar: the sonnet-5
  judge that grades verbose full-context answers more generously grades the bench's
  terse answers ~0.04 lower. Scores are comparable only within one judge. All dev-era
  judgments before 2026-07-09 were sonnet-5-judged.
- **Binaries pinned**: `iwe`/`iwec` frozen in `bin-pinned/`; pass
  `IWE_BIN`/`IWEC_BIN` explicitly so a product rebuild cannot change the harness
  mid-run, and record the versions in the run's `meta.json` notes.
- Model IDs pinned by probe on 2026-07-09: `claude-sonnet-4-6` serves under its own
  ID; the `haiku` alias resolves to `claude-haiku-4-5-20251001`. Pass the exact IDs
  (they are the compiled-in defaults of `xtask`). Record the resolved IDs
  (`modelUsage` in run records) with the first run and keep them pinned across
  repetitions.
- **One store per conversation.** LOCOMO questions never span conversations, so a
  unified store cannot improve any answer and only adds distractors;
  per-conversation stores also keep numbers comparable to published results. Tested
  empirically: merging four conversations into one namespaced store cost both arms
  ~1 point (see the combined-store experiment in `results/RUNS.md`).

## Validity rules

- **Question-blind curation**: the curator never sees QA pairs, ingests sessions in
  chronological order with no lookahead, and its prompt must make sense for any
  personal-conversation corpus.
- **Decontaminated prompt examples**: examples use synthetic entities and out-of-era
  dates (2024; the dataset lives in 2022–2023), so no example can pair a dataset
  entity with a fact or align with a gold answer.
- **Dev/test split**: dev = `conv-26`, `conv-30` (all iteration happened there);
  test = `conv-41,conv-42,conv-43,conv-44,conv-47,conv-48,conv-49,conv-50`. Spend
  status: conv-41/42 went in the 2026-07-09 multi-turn installment, and their failure
  reviews shaped prompt v7 (method iteration only, no dataset facts in the prompt);
  conv-43/44 went sealed in the 2026-07-12 one-shot installment (first-ever contact);
  conv-47–50 remain unspent. Results report all 8 conversations plus a
  **sealed-only cut** (conversations never read, curated, or answered before their
  measured run) as the sensitivity check.
- **Mutation guard** — applies to agentic arms holding a mutation-capable tool
  (`curated-q` with `iwe_query`, always strict over MCP): snapshot the curated
  workspaces before answering, diff after. Every dev run to date showed zero writes.
  `curated-ctx` holds no tools, so the ceremony does not apply to the headline.

## Runbook

`docs/test-plan.md` is the protocol of record for spending the test set; in summary:

```bash
X=target/debug/xtask
TEST=conv-41,conv-42,conv-43,conv-44,conv-47,conv-48,conv-49,conv-50

# 1. Raw-transcript workspaces for the fs arm
$X prepare --conversations $TEST

# 2. Curation — versioned workspace dir, one sequential worker per conversation
$X curate --conversations $TEST --workspaces workspaces-v7 --workers 7

# 3. Spot-audit each store before spending answers (shape per docs/store-form.md)

# 4. Answering — headline and baseline as concurrent processes
$X answer --run results/test2-ctx-r1 --arm curated-ctx --workspaces workspaces-v7 \
   --split test --workers 6 --max-budget-usd 2 &
$X answer --run results/test2-fs --arm fs --split test --workers 6 --max-budget-usd 2 &
wait

# 5. Judging (prints the per-category report; judge model is the compiled-in
#    default, claude-sonnet-4-6)
$X judge --run results/test2-ctx-r1 --workers 6
$X judge --run results/test2-fs --workers 6

# 6. Reports any time
$X report --run results/test2-ctx-r1
```

The headline arm uses 3 repetitions reported mean ± std (no temperature control
exists for `claude -p`): repeat steps 4–5 into fresh run directories
(`results/test2-ctx-r2`, …). Curation runs once — the store is the artifact;
repetitions vary only the answering. `xtask clean --kind curated` removes stale
curated stores when a workspace dir is being rebuilt in place.

**Partial runs and budget slices.** The test set can be spent in installments. Curate
a subset (`--conversations conv-47,conv-48`), answer into the *final* run directories
with the same subset filter, judge, and stop. To continue later: curate the next
conversations, then re-invoke the same `answer` commands with `--split test` — both
`answer` and `judge` skip completed items, so each installment picks up exactly where
the last ended and the run directories accumulate into the full test result. Do not
change models or prompts between installments; `meta.json` guards the arm and model
per run directory.

## Cost and time

Measured on the second test installment (652 questions across 4 conversations),
API-equivalent figures as reported by Claude Code; tokens and turns are the primary
efficiency metrics.

| stage | measured |
| --- | --- |
| curation (haiku, per conversation) | ~$4–5, one pass |
| `curated-ctx` answering | ~$0.051/q, 1 turn, ~2 s, ~13k tokens read per question |
| `fs` answering | ~$0.056/q, ~4.6 turns, ~12 s, ~70k+ tokens read per question |
| judge | ~$0.013/q |

Full test set (8 conversations, ~1,400 questions, 3 `curated-ctx` repetitions +
1 `fs` baseline + judges): ~$350, an afternoon of wall clock — the breakdown is in
`docs/test-plan.md`.

# HotPotQA track: multi-hop retrieval

Measures the knowledge graph itself. From the HotPotQA dev set (distractor
setting) a deterministic sample is frozen — 50 dev + 300 sealed test questions
— and the union of their context paragraphs becomes **one shared store: 3,484
markdown pages, one per Wikipedia article**. There is no curation stage; the
measured variable is link structure, added one tier at a time
(`docs/hotpot.md` is the protocol of record):

| tier | store | links built by | build cost | J (dev, limit 10) |
| --- | --- | --- | --- | --- |
| 1 | `corpus/` | none | $0 | 0.76 |
| 2 | `corpus-linked/` | string-matching article titles (corpus-rare stripped variants) | $0, 16 s | **0.900 ± 0.000** |
| 3 | `corpus-agentic/` | tier 2 + per-page agent link proposals (`enrich`, haiku) | ~$48 | 0.90 (no measurable gain) |

The tier-3 zero is a designed finding, not a failure: with mechanical links in
place, dossier recall of the supporting articles was already 44/50, and
exactly one dev question was a true retrieval miss — the LLM-built graph was
competing for one question of headroom. `results/RUNS.md` carries the full
forensics, including a query-decomposition variant (`--anchors`) that gained
+0.04 on dev and failed to replicate on the sealed test.

## Arms and sealed-test result

| Arm | Answering | J (300 sealed q) | turns | p50 | $/q |
| --- | --- | --- | --- | --- | --- |
| `ctx` | one-shot over an `iwe retrieve` dossier (BM25 seeds + all reference edges, inbound included) | 0.9133 (plain); 0.900 ± 0.005 (anchored, 3 reps) | 1 | ~2 s | 0.017 |
| `fs` | agentic `Grep`, `Glob`, `Read` over the raw corpus | 0.9033 | 5.2 | ~10.7 s | 0.039 |

Parity within noise — the one-shot over a $0 graph matches the 5-turn agent.
Configuration of record: `corpus-linked`, `--dossier-limit 10`, plain
whole-question query, `claude-sonnet-4-6` answering and judging
(`judge_hotpot.md`, selected automatically from the run's `meta.json`),
binary pinned as `bin-pinned/iwe-hotpot`.

## Runbook

```bash
X=target/debug/xtask
export IWE_BIN=$PWD/bin-pinned/iwe-hotpot

$X download --dataset hotpot     # pinned Wayback snapshot of the CMU original
$X ingest                        # freeze question samples + tier-1 corpus
$X ingest --linked               # tier-2 mechanical links
$X enrich --workers 6            # optional tier 3 (~$48); resumable JSONL log
$X enrich --source corpus-linked --target corpus-combined \
   --replay workspaces/hotpot/enrich-corpus-agentic.jsonl   # replay a logged enrichment, $0

$X answer --run results/hp-ctx-r1 --dataset hotpot --arm ctx --split test \
   --corpus corpus-linked --dossier-limit 10 --workers 6 --max-budget-usd 2
$X answer --run results/hp-fs --dataset hotpot --arm fs --split test \
   --workers 6 --max-budget-usd 2
$X judge --run results/hp-ctx-r1 --workers 6
```

`ingest` refuses to re-sample frozen question files without `--force`;
`meta.json` guards dataset, arm, model, corpus, dossier limit, and anchors per
run directory. Stores are question-blind by construction: mechanical
ingestion and linking never see a question, and the tier-3 agent sees only a
page plus its BM25 neighbors and can only propose links — the harness
validates and applies them, so curation cannot inject facts.

## Dataset

`hotpot_dev_distractor_v1.json` (7,405 questions; CC BY-SA 4.0; Yang et al.,
*HotpotQA: A Dataset for Diverse, Explainable Multi-hop Question Answering*,
EMNLP 2018). The canonical CMU server stopped responding in July 2026, so the
download is pinned to an immutable Wayback Machine snapshot — which doubles as
a reproducibility guarantee. Metric note: `exact_match`/`f1` are the bench's
Mem0-style implementations, not the official HotPotQA script — treat them as
within-bench comparators; `j` is the headline.

# Shared machinery

## Setup

One-time: authenticate the isolated bench profile.

```bash
CLAUDE_CONFIG_DIR=$PWD/.claude-profile claude /login
```

Point `IWE_BIN`/`IWEC_BIN` at the pinned binaries for measured runs; plain `iwe` and
`iwec` on `PATH` suffice for exploration.

```bash
export IWE_BIN=$PWD/bin-pinned/iwe IWEC_BIN=$PWD/bin-pinned/iwec
cargo build -p xtask
cargo xtask download
cargo xtask doctor
```

**Prompts are compiled into `xtask` via `include_str!`** — a prompt edit changes
nothing until `cargo build -p xtask` runs again. Verify the binary carries the edit
before spending money (one run already shipped with a stale prompt):
`strings target/debug/xtask | grep -q "<a phrase from the edit>"`.

Build once up front — the parallel loops invoke `target/debug/xtask` directly to
avoid cargo lock contention. `doctor` verifies isolation and model access before every
sweep.

Everything is resumable: `answer`, `judge`, and `enrich` append to JSONL and
skip completed items on re-invocation, so throttled or interrupted runs are
simply re-run. `answer` aborts after 5 consecutive failures (usage limits);
rerun to resume. Concurrency knobs: `--workers` inside each command, plus
running arms as separate processes. Tune down if runs get throttled.

## Isolation: how it works

A default `claude -p` invocation is *not* a clean environment. It loads the user's
global configuration from `~/.claude` (a personal `CLAUDE.md`, settings, hooks,
skills, plugins, MCP servers, auto-memory) and walks **up** from the working directory
collecting every ancestor `CLAUDE.md` into the system prompt — so an instruction file
two directories above a workspace silently reaches the agent. For a benchmark, each of
those is a leak: operator instructions contaminating the agents, or personal MCP tools
appearing in the tool list. The harness closes each path explicitly, and each measure
covers a leak the others do not:

| leak path | countermeasure |
| --- | --- |
| user-global config (`~/.claude`: CLAUDE.md, settings, hooks, skills, plugins, memory) | dedicated `CLAUDE_CONFIG_DIR` (`.claude-profile/`, gitignored) containing only the OAuth credentials from a one-time login — there is nothing else in it to load |
| project and ancestor `CLAUDE.md` discovery from the workspace's parent directories | `--setting-sources ""` — note this does **not** block the user-global config, which is why the dedicated profile is also required (verified empirically, not assumed) |
| MCP servers from user or project config | `--strict-mcp-config`: only the explicitly passed workspace `.mcp.json` (the `iwec` server) is visible |
| tool surface | explicit per-arm `--allowedTools` / `--disallowedTools`; grep arms cannot reach MCP, iwe arms cannot reach file tools, no arm gets `Bash`; the one-shot arms hold no tools at all |
| cross-run state | `--no-session-persistence`, per-question budget caps (`--max-budget-usd`) |

Verified against claude 2.1.201. `cargo xtask doctor` re-verifies before every sweep:
it plants sentinel `CLAUDE.md` files in the ancestor and global locations and runs a
probe that must answer `NONE`, proving no sentinel reached the agent.

## Metrics

`summary.json` per run, overall and per category: `j` (judge accuracy — headline),
`f1`, `exact_match`, `bleu1` (clipped unigram precision × brevity penalty; BLEU-2..4
from the legacy harness are dropped), cost, turns, duration percentiles, token totals.
Categories are LOCOMO's numeric 1–4 or HotPotQA's `bridge`/`comparison`.

## Deviations from the legacy Mem0 harness

- The answering "system" is Claude Code, not a search+prompt pipeline: agentic arms hold tools; the headline one-shot arms receive a harness-assembled retrieval dossier and hold none. The judge runs as a one-shot `claude -p` call with tools disabled.
- Memory construction is agentic and question-blind (see Validity rules), not an extraction pipeline.
- BLEU reduced to BLEU-1 with a simple tokenizer.
- No temperature control exists for `claude -p`; headline runs use 3 repetitions, reported mean ± std.
