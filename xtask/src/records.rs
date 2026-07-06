use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub id: String,
    pub conversation: String,
    pub category: u8,
    pub question: String,
    pub gold_answer: String,
    pub answer: String,
    pub total_cost_usd: f64,
    pub num_turns: u32,
    pub duration_ms: u64,
    pub session_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JudgmentRecord {
    pub id: String,
    pub label: String,
    pub explanation: String,
    pub judge_cost_usd: f64,
}

pub fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    std::fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

pub fn existing_ids(path: &Path) -> Result<BTreeSet<String>> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    std::fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line)?;
            Ok(value["id"].as_str().unwrap_or_default().to_string())
        })
        .collect()
}
