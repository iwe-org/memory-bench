use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub speaker: String,
    pub text: String,
    pub blip_caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub number: u32,
    pub timestamp: String,
    pub turns: Vec<Turn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Qa {
    pub question: String,
    pub answer: String,
    pub category: u8,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    pub sample_id: String,
    pub speaker_a: String,
    pub speaker_b: String,
    pub sessions: Vec<Session>,
    pub qa: Vec<Qa>,
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn value_to_u8(value: &Value) -> Result<u8> {
    match value {
        Value::Number(n) => Ok(n.as_u64().context("category out of range")? as u8),
        Value::String(s) => Ok(s.parse()?),
        other => anyhow::bail!("unexpected category value: {other}"),
    }
}

pub fn load(path: &Path, categories: Option<&BTreeSet<u8>>) -> Result<Vec<Conversation>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let raw: Value = serde_json::from_str(&text)?;
    let mut conversations = Vec::new();
    for item in raw.as_array().context("dataset root must be an array")? {
        let conv = item["conversation"]
            .as_object()
            .context("conversation must be an object")?;
        let mut numbers: Vec<u32> = conv
            .keys()
            .filter_map(|k| k.strip_prefix("session_"))
            .filter_map(|rest| rest.parse().ok())
            .collect();
        numbers.sort_unstable();
        let sessions = numbers
            .iter()
            .map(|n| {
                let turns = conv[&format!("session_{n}")]
                    .as_array()
                    .context("session must be an array")?
                    .iter()
                    .map(|t| Turn {
                        speaker: value_to_string(&t["speaker"]),
                        text: value_to_string(&t["text"]),
                        blip_caption: t
                            .get("blip_caption")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                    .collect();
                Ok(Session {
                    number: *n,
                    timestamp: value_to_string(&conv[&format!("session_{n}_date_time")]),
                    turns,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut qa = Vec::new();
        for q in item["qa"].as_array().context("qa must be an array")? {
            let category = value_to_u8(&q["category"])?;
            if categories.map(|set| set.contains(&category)) == Some(false) {
                continue;
            }
            qa.push(Qa {
                question: value_to_string(&q["question"]),
                answer: q.get("answer").map(value_to_string).unwrap_or_default(),
                category,
                evidence: q
                    .get("evidence")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().map(value_to_string).collect())
                    .unwrap_or_default(),
            });
        }
        conversations.push(Conversation {
            sample_id: value_to_string(&item["sample_id"]),
            speaker_a: value_to_string(&conv["speaker_a"]),
            speaker_b: value_to_string(&conv["speaker_b"]),
            sessions,
            qa,
        });
    }
    Ok(conversations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        serde_json::json!([
            {
                "sample_id": "conv-1",
                "conversation": {
                    "speaker_a": "Alice",
                    "speaker_b": "Bob",
                    "session_1_date_time": "1:00 pm on 1 May, 2023",
                    "session_1": [
                        {"speaker": "Alice", "dia_id": "D1:1", "text": "Hi Bob!"},
                        {"speaker": "Bob", "dia_id": "D1:2", "text": "Look at this!", "blip_caption": "a red bicycle"}
                    ],
                    "session_2_date_time": "2:00 pm on 2 May, 2023",
                    "session_2": [
                        {"speaker": "Alice", "dia_id": "D2:1", "text": "I got a new bike."}
                    ]
                },
                "qa": [
                    {"question": "What did Alice get?", "answer": "A new bike", "evidence": ["D2:1"], "category": 4},
                    {"question": "What did Bob lose?", "adversarial_answer": "nothing", "evidence": ["D1:2"], "category": 5},
                    {"question": "How many wheels?", "answer": 2, "evidence": ["D2:1"], "category": 3}
                ]
            }
        ])
        .to_string()
    }

    fn expected(qa: Vec<Qa>) -> Vec<Conversation> {
        vec![Conversation {
            sample_id: "conv-1".to_string(),
            speaker_a: "Alice".to_string(),
            speaker_b: "Bob".to_string(),
            sessions: vec![
                Session {
                    number: 1,
                    timestamp: "1:00 pm on 1 May, 2023".to_string(),
                    turns: vec![
                        Turn {
                            speaker: "Alice".to_string(),
                            text: "Hi Bob!".to_string(),
                            blip_caption: None,
                        },
                        Turn {
                            speaker: "Bob".to_string(),
                            text: "Look at this!".to_string(),
                            blip_caption: Some("a red bicycle".to_string()),
                        },
                    ],
                },
                Session {
                    number: 2,
                    timestamp: "2:00 pm on 2 May, 2023".to_string(),
                    turns: vec![Turn {
                        speaker: "Alice".to_string(),
                        text: "I got a new bike.".to_string(),
                        blip_caption: None,
                    }],
                },
            ],
            qa,
        }]
    }

    fn write_fixture(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("membench-{}-{name}.json", std::process::id()));
        std::fs::write(&path, fixture()).unwrap();
        path
    }

    #[test]
    fn loads_all_categories() {
        let path = write_fixture("all");
        let loaded = load(&path, None).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            loaded,
            expected(vec![
                Qa {
                    question: "What did Alice get?".to_string(),
                    answer: "A new bike".to_string(),
                    category: 4,
                    evidence: vec!["D2:1".to_string()],
                },
                Qa {
                    question: "What did Bob lose?".to_string(),
                    answer: String::new(),
                    category: 5,
                    evidence: vec!["D1:2".to_string()],
                },
                Qa {
                    question: "How many wheels?".to_string(),
                    answer: "2".to_string(),
                    category: 3,
                    evidence: vec!["D2:1".to_string()],
                },
            ])
        );
    }

    #[test]
    fn filters_categories() {
        let path = write_fixture("filtered");
        let categories = BTreeSet::from([3, 4]);
        let loaded = load(&path, Some(&categories)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            loaded,
            expected(vec![
                Qa {
                    question: "What did Alice get?".to_string(),
                    answer: "A new bike".to_string(),
                    category: 4,
                    evidence: vec!["D2:1".to_string()],
                },
                Qa {
                    question: "How many wheels?".to_string(),
                    answer: "2".to_string(),
                    category: 3,
                    evidence: vec!["D2:1".to_string()],
                },
            ])
        );
    }
}
