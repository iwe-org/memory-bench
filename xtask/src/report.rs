use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use crate::metrics;
use crate::records::{read_jsonl, AnswerRecord, JudgmentRecord};

struct Row {
    answer: AnswerRecord,
    label: Option<String>,
    metrics: metrics::Metrics,
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let collected: Vec<f64> = values.collect();
    if collected.is_empty() {
        return 0.0;
    }
    collected.iter().sum::<f64>() / collected.len() as f64
}

fn percentile(mut values: Vec<f64>, p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).expect("comparable"));
    let index = (p * (values.len() - 1) as f64).round() as usize;
    values[index]
}

fn aggregate(rows: &[&Row]) -> Value {
    let judged: Vec<&&Row> = rows.iter().filter(|r| r.label.is_some()).collect();
    let correct = judged
        .iter()
        .filter(|r| r.label.as_deref() == Some("CORRECT"))
        .count();
    let durations: Vec<f64> = rows
        .iter()
        .map(|r| r.answer.duration_ms as f64 / 1000.0)
        .collect();
    let turns: Vec<f64> = rows.iter().map(|r| r.answer.num_turns as f64).collect();
    json!({
        "questions": rows.len(),
        "judged": judged.len(),
        "j": if judged.is_empty() { Value::Null } else { json!(round4(correct as f64 / judged.len() as f64)) },
        "f1": round4(mean(rows.iter().map(|r| r.metrics.f1))),
        "exact_match": round4(mean(rows.iter().map(|r| r.metrics.exact_match as f64))),
        "bleu1": round4(mean(rows.iter().map(|r| r.metrics.bleu1))),
        "cost_usd": round4(rows.iter().map(|r| r.answer.total_cost_usd).sum()),
        "turns_mean": round4(mean(turns.iter().copied())),
        "turns_p95": percentile(turns, 0.95),
        "duration_p50_s": round4(percentile(durations.clone(), 0.5)),
        "duration_p95_s": round4(percentile(durations, 0.95)),
        "input_tokens": rows.iter().map(|r| r.answer.input_tokens).sum::<u64>(),
        "output_tokens": rows.iter().map(|r| r.answer.output_tokens).sum::<u64>(),
        "cache_creation_input_tokens": rows.iter().map(|r| r.answer.cache_creation_input_tokens).sum::<u64>(),
        "cache_read_input_tokens": rows.iter().map(|r| r.answer.cache_read_input_tokens).sum::<u64>(),
    })
}

pub fn build(run: &Path) -> Result<Value> {
    let answers: Vec<AnswerRecord> = read_jsonl(&run.join("answers.jsonl"))?;
    let judgments: Vec<JudgmentRecord> = read_jsonl(&run.join("judgments.jsonl"))?;
    let labels: BTreeMap<String, String> = judgments
        .into_iter()
        .map(|j| (j.id, j.label))
        .collect();
    let rows: Vec<Row> = answers
        .into_iter()
        .map(|answer| Row {
            label: labels.get(&answer.id).cloned(),
            metrics: metrics::calculate(&answer.answer, &answer.gold_answer),
            answer,
        })
        .collect();
    let all: Vec<&Row> = rows.iter().collect();
    let mut categories = serde_json::Map::new();
    let mut category_ids: Vec<String> = rows.iter().map(|r| r.answer.category.clone()).collect();
    category_ids.sort_unstable();
    category_ids.dedup();
    for category in category_ids {
        let subset: Vec<&Row> = rows
            .iter()
            .filter(|r| r.answer.category == category)
            .collect();
        categories.insert(category, aggregate(&subset));
    }
    let meta_path = run.join("meta.json");
    let meta: Value = if meta_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&meta_path)?)?
    } else {
        Value::Null
    };
    let summary = json!({
        "meta": meta,
        "overall": aggregate(&all),
        "categories": Value::Object(categories),
    });
    std::fs::write(
        run.join("summary.json"),
        serde_json::to_string_pretty(&summary)? + "\n",
    )?;
    Ok(summary)
}

fn cell(value: &Value) -> String {
    match value {
        Value::Null => "-".to_string(),
        Value::Number(n) => format!("{n}"),
        other => other.to_string(),
    }
}

pub fn print(summary: &Value) {
    println!(
        "{:<21}{:>6}{:>8}{:>8}{:>8}{:>9}{:>8}{:>9}{:>9}",
        "scope", "n", "J", "F1", "BLEU-1", "cost$", "turns", "p50 s", "p95 s"
    );
    let mut scopes = vec![("overall".to_string(), &summary["overall"])];
    if let Some(categories) = summary["categories"].as_object() {
        for (category, value) in categories {
            scopes.push((format!("category {category}"), value));
        }
    }
    for (name, s) in scopes {
        println!(
            "{:<21}{:>6}{:>8}{:>8}{:>8}{:>9}{:>8}{:>9}{:>9}",
            name,
            cell(&s["questions"]),
            cell(&s["j"]),
            cell(&s["f1"]),
            cell(&s["bleu1"]),
            cell(&s["cost_usd"]),
            cell(&s["turns_mean"]),
            cell(&s["duration_p50_s"]),
            cell(&s["duration_p95_s"]),
        );
    }
    let overall = &summary["overall"];
    println!(
        "tokens: input={} output={} cache_read={} cache_write={}",
        cell(&overall["input_tokens"]),
        cell(&overall["output_tokens"]),
        cell(&overall["cache_read_input_tokens"]),
        cell(&overall["cache_creation_input_tokens"]),
    );
}
