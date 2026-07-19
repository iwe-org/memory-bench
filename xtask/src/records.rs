use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};

fn category_label<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(label) => Ok(label),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "unexpected category value: {other}"
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub id: String,
    pub conversation: String,
    #[serde(deserialize_with = "category_label")]
    pub category: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_numeric_category() {
        let record: AnswerRecord = serde_json::from_str(
            r#"{"id":"a:1","conversation":"a","category":4,"question":"q","gold_answer":"g","answer":"x","total_cost_usd":0.0,"num_turns":1,"duration_ms":10,"session_id":null,"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#,
        )
        .unwrap();
        assert_eq!(record.category, "4");
    }

    #[test]
    fn reads_string_category() {
        let record: AnswerRecord = serde_json::from_str(
            r#"{"id":"q1","conversation":"hotpot","category":"bridge","question":"q","gold_answer":"g","answer":"x","total_cost_usd":0.0,"num_turns":1,"duration_ms":10,"session_id":null,"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#,
        )
        .unwrap();
        assert_eq!(record.category, "bridge");
    }
}
