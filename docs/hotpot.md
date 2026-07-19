# HotPotQA track

Measures IWE as a retrieval substrate for multi-hop factoid QA over a document
corpus — the arena where graph-memory tools (cognee, LightRAG, Graphiti) publish
their comparisons. Unlike the LOCOMO track there is no curation stage: the corpus
is ingested mechanically, so the measurement isolates the retrieval mechanism
(`iwe retrieve` BM25 + expansion) from store authoring.

## Dataset

HotPotQA dev set, distractor setting (CC BY-SA 4.0; Yang et al., *HotpotQA: A
Dataset for Diverse, Explainable Multi-hop Question Answering*, EMNLP 2018).
`cargo xtask download --dataset hotpot` fetches `hotpot_dev_distractor_v1.json`
(7,405 questions, 46.3 MB) into `data/`. The download is pinned to an immutable
Wayback Machine snapshot (2026-03-10) of the canonical CMU URL — the origin
server stopped responding as of 2026-07-18, and the pin doubles as a
reproducibility guarantee. Each question carries 10 context paragraphs —
2 gold articles plus 8 distractors — a gold answer span, and a type
(`bridge` or `comparison`; the dev split is hard-level throughout).

## Sampling

`cargo xtask ingest` draws a deterministic sample (seeded shuffle, seed compiled
in) and freezes it into the workspace:

- `workspaces/hotpot/questions-dev.json` — 50 questions, iteration set
- `workspaces/hotpot/questions-test.json` — 300 questions, disjoint, spent once

The frozen files are the protocol artifact: `answer --split dev|test` reads them,
never the raw dataset. Re-ingesting refuses to overwrite them without `--force`
so the sample cannot drift between runs.

## Corpus store

One shared store, `workspaces/hotpot/corpus/`: the union of context paragraphs
across both frozen samples, deduplicated by article title — one markdown page per
article (`# Title` + paragraph text), key slugified from the title. Both arms read
this same directory; ingestion is question-blind by construction (the corpus is
the dataset's own distractor design, assembled without model involvement, $0).

Three corpus tiers isolate what each layer of graph structure buys, all
question-blind:

1. **Tier 1, `corpus/`** — no links; the `ctx` arm measures the BM25 floor of
   the one-shot mechanism (reference expansion has nothing to walk).
2. **Tier 2, `corpus-linked/`** (`ingest --linked`) — mechanical title-mention
   linking: each page's first bounded occurrence of another article's title
   becomes a link, reconstructing Wikipedia's own hyperlink structure by
   string matching. Parenthetical-stripped variants ("Eluvium" for "Eluvium
   (musician)") are added only when corpus-rare (bounded occurrences in ≤ 8
   other pages) — common words like "Office" would otherwise false-link
   dozens of pages and pollute inbound expansion. Deterministic, $0, no model
   involvement. Select with `answer --corpus corpus-linked`.
3. **Tier 3, `corpus-agentic/`** (`enrich`) — an agentic pass on top of
   tier 2: one one-shot model call per page (haiku by default) sees the page
   plus BM25-nearest candidate articles and proposes links string matching
   cannot see (descriptive mentions — "defending champions", "the previous
   year's final"). The harness validates and applies proposals mechanically:
   spans must exist verbatim outside existing links, keys must exist, the
   agent never rewrites content — its only reachable surface is link
   topology, so curation cannot inject facts. Resumable via
   `enrich-<target>.jsonl`. Select with `answer --corpus corpus-agentic`.

The hotpot dossier expands all reference edges — outbound
(`--expand-references`) and inbound (`--expand-referenced-by`) plus
`--expand-included-by` — so bridge questions whose second-hop article *links
to* the BM25-found seed are pulled in. Both expansion directions are no-ops on
the unlinked tier-1 corpus, keeping the corpus the only variable between tiers.
`--dossier-limit` (default 5) sets the BM25 seed count. `--anchors` adds
entity-anchored seeding: distinctive spans mechanically extracted from the
question text (quoted strings, stopword-trimmed capitalized runs,
slash/camel-case singles) each get their own BM25 sub-query (limit 2, 4k-token
slice, expansions on) ahead of the whole-question query — long questions can
no longer drown a rare entity term, and comparison questions get both entity
articles as seeds by construction. All three knobs are recorded and guarded in
the run's `meta.json`.

## Arms

| Arm | Memory | Answering | Measures |
| --- | --- | --- | --- |
| `ctx` | corpus store | one-shot: harness-assembled dossier (`iwe retrieve --lexical`, limit 5, 12k tokens), zero tools | one product retrieval per question |
| `fs` | same corpus files | agentic: `Grep`, `Glob`, `Read` | filesystem baseline the one-shot must beat |

Dossier parameters are identical to the LOCOMO track so the mechanism is
comparable across tracks. The `fs` arm is the honest baseline the published
tool comparisons omit.

## Metrics

`j` (binary CORRECT/WRONG, `judge_hotpot.md`, same judge machinery and model as
the LOCOMO track) is the headline; `exact_match`, `f1`, `bleu1` from the existing
Mem0-style implementation are reported alongside. Deviation note: these are *not*
the official HotPotQA evaluation script figures (no article stripping, set-based
F1, simple tokenizer) — treat them as within-bench comparators, not
literature-comparable numbers. Categories in `summary.json` are `bridge` and
`comparison`.

## Comparability

Published cognee/Mem0/LightRAG/Graphiti HotPotQA numbers come from different
answering pipelines, different judges (DeepEval), and a 24-question sample —
none of it transfers. The claims this track can make honestly:

1. `ctx` vs `fs` under one answerer and one judge — does one-shot product
   retrieval beat agentic grep on multi-hop factoid QA, and at what cost/latency.
2. Tier 2 vs tier 1 — what linking adds over bare BM25 on the same store.
3. Sample size 300 vs their 24, with the same mean ± std repetition discipline
   as the LOCOMO test set.

## Validity rules

- Sample frozen at ingest, recorded in the workspace, never re-drawn; dev for
  iteration, test spent once per config.
- No question-aware edits to the corpus store; tier 2 linking prompts must make
  sense for any encyclopedia corpus and use decontaminated examples (synthetic
  entities) per the LOCOMO rules.
- Isolation machinery unchanged (bench profile, `--setting-sources ""`,
  `--strict-mcp-config`, per-arm tool lists, `doctor` before sweeps).
- Pinned binaries for measured runs (`IWE_BIN`), versions recorded in run notes.

## Cost

At LOCOMO-measured rates (~$0.05/q one-shot answering, ~$0.056/q fs, ~$0.013/q
judge): dev iteration ≈ $6 per full pass; test run (300 q × 2 arms + judges)
≈ $45; 3 `ctx` repetitions + 1 `fs` ≈ $75. Ingestion is free.

## Runbook

```bash
X=target/debug/xtask
$X download --dataset hotpot
$X ingest
$X answer --run results/hotpot-dev-ctx --dataset hotpot --arm ctx --split dev --workers 6 --max-budget-usd 2
$X answer --run results/hotpot-dev-fs --dataset hotpot --arm fs --split dev --workers 6 --max-budget-usd 2
$X judge --run results/hotpot-dev-ctx --workers 6
$X judge --run results/hotpot-dev-fs --workers 6
$X report --run results/hotpot-dev-ctx
```

`answer` and `judge` resume exactly as in the LOCOMO track; `meta.json` guards
arm, model, and dataset per run directory.
