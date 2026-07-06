# Markdown knowledge graphs as agent memory

Measures [IWE](https://github.com/iwe-org/iwe) as agent memory for `claude -p` (Claude
Code headless) on the [LOCOMO](https://github.com/snap-research/locomo) dataset, judged
Mem0-style. An agent curates each conversation into an IWE knowledge graph — markdown
pages, links, dated event pages — and answers are produced with that graph as the only
memory: either by an agent holding search tools, or (the headline) by a single
one-shot call over a retrieval dossier the harness assembles with `iwe retrieve`.

Every model interaction — curation, answering, and judging — runs through `claude -p`.
There are no direct API calls; sessions authenticate through the bench's own isolated
profile. The judge prompt and grading criteria are ported verbatim from the legacy
[Mem0 LOCOMO evaluation](https://github.com/mem0ai/mem0/tree/aae5989e78/evaluation)
(binary CORRECT/WRONG "J" metric, categories 1–4, adversarial category 5 excluded).

The research narrative is published separately. Inside this repo: `results/RUNS.md` is
the ledger of every measured run and the source of final numbers;
`results/curated-pilot-report.md` holds the dev-set analysis; `docs/test-plan.md` is
the protocol of record for the test set; `docs/store-form.md` is the normative store
shape.

## Repository map

- `xtask/` — the harness. Prompts live in `xtask/prompts/` (`curate.md`,
  `consolidate.md`, `answer.md` for agentic arms, `answer_context.md` for the
  one-shot arm, `full_context.md`, `judge.md`) and are compiled in — see Setup.
- `docs/store-form.md` — normative store shape (when a curation prompt and this
  document disagree, the document wins); `docs/store-schemas/` — hub and session
  page schemas; `docs/test-plan.md` — test-set protocol.
- `results/` — one directory per run (JSONL records, `meta.json`, `summary.json`),
  plus `RUNS.md` and `curated-pilot-report.md`.
- `bin-pinned/` — frozen `iwe`/`iwec` builds for the test set.
- `workspaces*/` — generated stores (gitignored): `workspaces/` holds the raw
  transcripts from `prepare` and is the default; curated stores live in one
  directory per curation prompt version (`workspaces-v7`, …), selected with
  `--workspaces`.
- `archive/` — retired stores kept for the record; `data/` — the dataset
  (gitignored).

## Arms

| Arm | Memory | Answering | Measures |
| --- | --- | --- | --- |
| `curated-ctx` | curated notes only | one-shot: harness-assembled dossier inlined into the prompt, zero tools | the headline: automated curation + one-shot product retrieval |
| `fs` | raw session transcripts | agentic: `Grep`, `Glob`, `Read` | filesystem baseline (Letta-style) the headline must beat |
| `curated-fs` | curated notes only | agentic: `Grep`, `Glob`, `Read` | representation without graph tools (dev ablation) |
| `curated-q` | curated notes only | agentic: `iwe_find`, `iwe_retrieve`, `iwe_tree`, `iwe_squash`, `iwe_stats`, `iwe_query` via MCP | multi-turn agentic pipeline (former headline, superseded by `curated-ctx`) |

The `curated-ctx` dossier is one product retrieval per question — no agentic search,
no second chance:

```bash
iwe retrieve --lexical "$QUESTION" --limit 5 \
  --expand-references --expand-included-by --max-tokens 12000 -f json
```

The store shape this contract demands (search bait, expansion targets, links binding
them) is specified in `docs/store-form.md`.

The curated arms answer over a store built by `xtask curate`: one `claude -p` session
per conversation session, chronological, question-blind. `curate --consolidate` adds
an editor's pass after ingestion — one more session with the whole store visible that
reorganizes pages without adding facts (`xtask/prompts/consolidate.md`). The raw
transcript is absent from curated workspaces — what the curator failed to write down
is lost.

Additional arms from earlier rounds (`iwe`, `fs-iwe`, `curated`, `full-context`)
remain runnable; see `results/RUNS.md` for their history.

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

## Dataset

The LOCOMO dataset is not distributed with this repo; `cargo xtask download` fetches
`locomo10.json` from [snap-research/locomo](https://github.com/snap-research/locomo)
(CC BY-NC 4.0; Maharana et al., *Evaluating Very Long-Term Conversational Memory of LLM
Agents*, ACL 2024) into the gitignored `data/` directory.

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

Build once up front — the parallel loops below invoke `target/debug/xtask` directly to
avoid cargo lock contention. `doctor` verifies isolation and model access before every
sweep.

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

Everything is resumable: `answer` and `judge` append to JSONL and skip completed items
on re-invocation, so throttled or interrupted runs are simply re-run. `answer` aborts
after 5 consecutive failures (usage limits); rerun to resume. Concurrency knobs:
`--workers` inside each command, plus running the arms as separate processes. Tune
down if runs get throttled.

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
| tool surface | explicit per-arm `--allowedTools` / `--disallowedTools`; grep arms cannot reach MCP, iwe arms cannot reach file tools, no arm gets `Bash`; the one-shot `curated-ctx` arm holds no tools at all |
| cross-run state | `--no-session-persistence`, per-question budget caps (`--max-budget-usd`) |

Verified against claude 2.1.201. `cargo xtask doctor` re-verifies before every sweep:
it plants sentinel `CLAUDE.md` files in the ancestor and global locations and runs a
probe that must answer `NONE`, proving no sentinel reached the agent.

## Metrics

`summary.json` per run, overall and per category: `j` (judge accuracy — headline),
`f1`, `exact_match`, `bleu1` (clipped unigram precision × brevity penalty; BLEU-2..4
from the legacy harness are dropped), cost, turns, duration percentiles, token totals.

## Deviations from the legacy Mem0 harness

- The answering "system" is Claude Code, not a search+prompt pipeline: agentic arms hold tools; the headline one-shot arm receives a harness-assembled retrieval dossier and holds none. The judge runs as a one-shot `claude -p` call with tools disabled.
- Memory construction is agentic and question-blind (see Validity rules), not an extraction pipeline.
- BLEU reduced to BLEU-1 with a simple tokenizer.
- No temperature control exists for `claude -p`; headline runs use 3 repetitions, reported mean ± std.
