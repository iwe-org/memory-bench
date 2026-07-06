# Test-set plan — one-shot era

The protocol for spending the test set under the one-shot architecture.
Supersedes the multi-turn runbook in README.md for the headline arms; the
2026-07-09 installment (fs / curated-fs / curated-q over conv-41+42) stays in
`results/RUNS.md` as the multi-turn era record and is not extended.

## Gate (precondition)

`ctx42-haiku-v7` — haiku curation of conv-42 under prompt v7, one-shot
answering, judge. Proceed at **J ≥ 0.78**. Below that: iterate the curation
prompt on dev conversations (conv-26 / conv-30) and, if needed, conv-42 —
which is burned as a clean test point anyway — but NEVER on the sealed six.

## Arms of record

| arm | measures | reps |
| --- | --- | --- |
| `curated-ctx` over v7 haiku stores | the headline: automated curation + one-shot product retrieval | 3 (answering only) |
| `fs` over raw transcripts | the baseline the headline must beat | 1 |

Dropped from the test protocol: `curated-q` (multi-turn agentic — the
architecture the one-shot replaced), `curated-fs` (representation ablation —
a dev-set question, answered there). The mutation snapshot/diff ceremony is
obsolete: `curated-ctx` holds zero tools.

## Frozen configuration

- Curation: `claude-haiku-4-5-20251001`, prompt v7 (`xtask/prompts/curate.md`
  as of the gate run), $3/session budget, question-blind, chronological.
- Answering: `claude-sonnet-4-6`, one-shot, dossier =
  `iwe retrieve --lexical "<question>" --limit 5 --expand-references
  --expand-included-by -f json` (assembled by the harness, agent gets zero
  tools).
- Judge: `claude-sonnet-4-6`, Mem0 prompt verbatim.
- Binaries: `iwe`/`iwec` built from the iwe repo at the gate-run commit; pass
  `IWE_BIN`/`IWEC_BIN` explicitly. Record versions in `meta.json` notes.
- **Prompts are compiled into xtask (`include_str!`) — rebuild xtask after any
  prompt edit and verify before launching** (we shipped one run with a stale
  prompt already).
- No prompt, model, or flag changes between conversations or repetitions.

## Validity notes (disclose with results)

- conv-41 and conv-42 were spent in the multi-turn installment and analyzed in
  failure reviews; prompt v7 was shaped by those reviews (method iteration, no
  dataset facts in the prompt). The six sealed conversations
  (conv-43, 44, 47, 48, 49, 50) have never been read, curated, or answered.
- Report the headline over all 8 test conversations (comparable to published
  LOCOMO numbers), plus a **sealed-only cut** (6 conversations) as the
  sensitivity check that 41/42 exposure did not inflate the result.
- The conv-42 v7 store from the gate run is reused as-is (it was curated
  question-blind; re-curating would only add variance).

## Runbook

```bash
cd memory-bench
export IWE_BIN=.../iwe/target/debug/iwe IWEC_BIN=.../iwe/target/debug/iwec
X=target/debug/xtask
REST=conv-41,conv-43,conv-44,conv-47,conv-48,conv-49,conv-50

# 0. doctor + confirm xtask binary carries prompt v7
$X doctor
strings target/debug/xtask | grep -q "Keys are identities" || echo REBUILD

# 1. curate the remaining seven conversations (conv-42 store exists from the gate)
$X curate --conversations $REST --workspaces workspaces-v7 --workers 7

# 2. spot-audit each store before spending answers:
#    bare ISO keys, topical titles, hub Timeline links, no page links to index

# 3. answering — three ctx repetitions + fs baseline, concurrent
TEST=conv-41,conv-42,conv-43,conv-44,conv-47,conv-48,conv-49,conv-50
$X answer --run results/test2-ctx-r1 --arm curated-ctx --workspaces workspaces-v7 \
   --split test --workers 6 --max-budget-usd 2 &
$X answer --run results/test2-ctx-r2 --arm curated-ctx --workspaces workspaces-v7 \
   --split test --workers 6 --max-budget-usd 2 &
$X answer --run results/test2-fs --arm fs --split test --workers 6 --max-budget-usd 2 &
wait
$X answer --run results/test2-ctx-r3 --arm curated-ctx --workspaces workspaces-v7 \
   --split test --workers 6 --max-budget-usd 2

# 4. judge everything
for r in test2-ctx-r1 test2-ctx-r2 test2-ctx-r3 test2-fs; do
  $X judge --run results/$r --workers 6
done

# 5. reports
$X report --run results/test2-ctx-r1
```

Small xtask addition needed before step 5: `report --conversations <LIST>`
(filter the summary to a conversation subset) for the sealed-only cut — today
`report` takes only `--run`.

Everything resumes: `answer`/`judge` skip completed items, so rate-limit
interruptions are re-run, not restarted.

## Cost and time (estimate)

| stage | est. cost | wall clock |
| --- | --- | --- |
| curation, 7 conversations (~190 sessions, haiku) | ~$35 | ~45 min (parallel) |
| `curated-ctx` answering, ~1,400 q × 3 reps | ~$150–200 | ~2–3 h total |
| `fs` answering, ~1,400 q × 1 | ~$85 | concurrent with ctx |
| judges, ~5,600 answers | ~$75 | ~2 h |
| **total** | **~$350** | **an afternoon** |

Single-repetition fallback if budget-constrained: run r1 only (~$170 total),
add r2/r3 later — repetitions vary answering only, so they append cleanly.

## Reporting

- Headline: `curated-ctx` mean ± std across 3 reps, overall and per category,
  vs `fs`, with cost/question, turns (=1), and latency percentiles.
- Sealed-only cut alongside.
- Anchors for context (different judges — approximate): Mem0 ~0.67,
  Mem0g 0.684, full-context 0.729, Letta-fs 0.740, Zep 0.751; our 4-6 judge
  grades the full-context calibration at 0.70.
- Everything lands in `results/RUNS.md`; the article takes its final numbers
  from here.
