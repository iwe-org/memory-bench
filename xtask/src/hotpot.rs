use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prepare;

pub const SAMPLE_SEED: u64 = 20260718;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub question: String,
    pub answer: String,
    pub qtype: String,
    pub context: Vec<(String, Vec<String>)>,
    pub supporting: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub question: String,
    pub answer: String,
    #[serde(rename = "type")]
    pub qtype: String,
    pub supporting: Vec<String>,
}

pub fn load(path: &Path) -> Result<Vec<Item>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let raw: Value = serde_json::from_str(&text)?;
    let mut items = Vec::new();
    for entry in raw.as_array().context("dataset root must be an array")? {
        let context = entry["context"]
            .as_array()
            .context("context must be an array")?
            .iter()
            .map(|pair| {
                let title = pair[0].as_str().unwrap_or_default().to_string();
                let sentences = pair[1]
                    .as_array()
                    .map(|s| {
                        s.iter()
                            .map(|v| v.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                (title, sentences)
            })
            .collect();
        let mut supporting: Vec<String> = entry["supporting_facts"]
            .as_array()
            .map(|facts| {
                facts
                    .iter()
                    .filter_map(|f| f[0].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let mut seen = BTreeSet::new();
        supporting.retain(|title| seen.insert(title.clone()));
        items.push(Item {
            id: entry["_id"].as_str().unwrap_or_default().to_string(),
            question: entry["question"].as_str().unwrap_or_default().to_string(),
            answer: entry["answer"].as_str().unwrap_or_default().to_string(),
            qtype: entry["type"].as_str().unwrap_or_default().to_string(),
            context,
            supporting,
        });
    }
    Ok(items)
}

fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed.max(1);
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        items.swap(i, (state % (i as u64 + 1)) as usize);
    }
}

pub fn sample(mut items: Vec<Item>, seed: u64) -> Vec<Item> {
    items.sort_by(|a, b| a.id.cmp(&b.id));
    shuffle(&mut items, seed);
    items
}

pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "article".to_string()
    } else {
        out
    }
}

fn render_article(title: &str, sentences: &[String]) -> String {
    let joined = sentences.join(" ");
    let text: Vec<&str> = joined.split_whitespace().collect();
    format!("# {}\n\n{}\n", title, text.join(" "))
}

pub fn corpus_pages(items: &[Item]) -> BTreeMap<String, String> {
    let mut articles: BTreeMap<&str, &[String]> = BTreeMap::new();
    for item in items {
        for (title, sentences) in &item.context {
            articles.entry(title.as_str()).or_insert(sentences);
        }
    }
    let mut pages = BTreeMap::new();
    for (title, sentences) in articles {
        let base = slug(title);
        let mut key = base.clone();
        let mut n = 2;
        while pages.contains_key(&key) {
            key = format!("{base}-{n}");
            n += 1;
        }
        pages.insert(key, render_article(title, sentences));
    }
    pages
}

pub struct IngestConfig {
    pub data: PathBuf,
    pub workspaces: PathBuf,
    pub dev_questions: usize,
    pub test_questions: usize,
    pub force: bool,
}

fn write_questions(path: &Path, items: &[Item]) -> Result<()> {
    let questions: Vec<Question> = items
        .iter()
        .map(|item| Question {
            id: item.id.clone(),
            question: item.question.clone(),
            answer: item.answer.clone(),
            qtype: item.qtype.clone(),
            supporting: item.supporting.clone(),
        })
        .collect();
    std::fs::write(path, serde_json::to_string_pretty(&questions)? + "\n")?;
    Ok(())
}

pub fn read_questions(path: &Path) -> Result<Vec<Question>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}; run `cargo xtask ingest` first", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn ingest(config: &IngestConfig) -> Result<()> {
    let items = sample(load(&config.data)?, SAMPLE_SEED);
    let total = config.dev_questions + config.test_questions;
    anyhow::ensure!(
        items.len() >= total,
        "dataset has {} questions, need {total}",
        items.len()
    );
    let root = config.workspaces.join("hotpot");
    let dev_path = root.join("questions-dev.json");
    let test_path = root.join("questions-test.json");
    if config.force {
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
    } else {
        anyhow::ensure!(
            !dev_path.exists() && !test_path.exists(),
            "frozen question files exist in {}; pass --force to re-sample",
            root.display()
        );
    }
    let corpus = root.join("corpus");
    std::fs::create_dir_all(&corpus)?;
    let pages = corpus_pages(&items[..total]);
    for (key, content) in &pages {
        std::fs::write(corpus.join(format!("{key}.md")), content)?;
    }
    prepare::init_iwe_bare(&corpus)?;
    write_questions(&dev_path, &items[..config.dev_questions])?;
    write_questions(&test_path, &items[config.dev_questions..total])?;
    println!(
        "hotpot: {} dev + {} test questions, {} articles in {}",
        config.dev_questions,
        config.test_questions,
        pages.len(),
        corpus.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        serde_json::json!([
            {
                "_id": "q1",
                "question": "Were Alpha Corp and Beta Corp founded in the same country?",
                "answer": "yes",
                "type": "comparison",
                "level": "hard",
                "supporting_facts": [["Alpha Corp", 0], ["Beta Corp", 0], ["Alpha Corp", 1]],
                "context": [
                    ["Alpha Corp", ["Alpha Corp is a company.", " It was founded in 1990."]],
                    ["Beta Corp", ["Beta Corp is a company."]]
                ]
            },
            {
                "_id": "q2",
                "question": "Who founded Alpha Corp?",
                "answer": "Jane Roe",
                "type": "bridge",
                "level": "hard",
                "supporting_facts": [["Alpha Corp", 0]],
                "context": [
                    ["Alpha Corp", ["Alpha Corp is a company.", " It was founded in 1990."]],
                    ["Gamma Inc", ["Gamma Inc makes widgets."]]
                ]
            }
        ])
        .to_string()
    }

    fn write_fixture(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("hotpot-{}-{name}.json", std::process::id()));
        std::fs::write(&path, fixture()).unwrap();
        path
    }

    #[test]
    fn loads_items() {
        let path = write_fixture("load");
        let loaded = load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            loaded,
            vec![
                Item {
                    id: "q1".to_string(),
                    question: "Were Alpha Corp and Beta Corp founded in the same country?"
                        .to_string(),
                    answer: "yes".to_string(),
                    qtype: "comparison".to_string(),
                    context: vec![
                        (
                            "Alpha Corp".to_string(),
                            vec![
                                "Alpha Corp is a company.".to_string(),
                                " It was founded in 1990.".to_string(),
                            ],
                        ),
                        (
                            "Beta Corp".to_string(),
                            vec!["Beta Corp is a company.".to_string()],
                        ),
                    ],
                    supporting: vec!["Alpha Corp".to_string(), "Beta Corp".to_string()],
                },
                Item {
                    id: "q2".to_string(),
                    question: "Who founded Alpha Corp?".to_string(),
                    answer: "Jane Roe".to_string(),
                    qtype: "bridge".to_string(),
                    context: vec![
                        (
                            "Alpha Corp".to_string(),
                            vec![
                                "Alpha Corp is a company.".to_string(),
                                " It was founded in 1990.".to_string(),
                            ],
                        ),
                        (
                            "Gamma Inc".to_string(),
                            vec!["Gamma Inc makes widgets.".to_string()],
                        ),
                    ],
                    supporting: vec!["Alpha Corp".to_string()],
                },
            ]
        );
    }

    #[test]
    fn sample_is_deterministic() {
        let path = write_fixture("sample");
        let first = sample(load(&path).unwrap(), 7);
        let second = sample(load(&path).unwrap(), 7);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn slugs_titles() {
        assert_eq!(slug("Alpha Corp"), "alpha-corp");
        assert_eq!(slug("Beta (film)"), "beta-film");
        assert_eq!(slug("A.B. Corp"), "a-b-corp");
        assert_eq!(slug("!!!"), "article");
    }

    #[test]
    fn corpus_dedups_and_disambiguates() {
        let items = vec![
            Item {
                id: "q1".to_string(),
                question: String::new(),
                answer: String::new(),
                qtype: String::new(),
                context: vec![
                    (
                        "Alpha Corp".to_string(),
                        vec!["Alpha Corp is a company.".to_string()],
                    ),
                    (
                        "Alpha. Corp".to_string(),
                        vec!["Another article.".to_string()],
                    ),
                ],
                supporting: Vec::new(),
            },
            Item {
                id: "q2".to_string(),
                question: String::new(),
                answer: String::new(),
                qtype: String::new(),
                context: vec![(
                    "Alpha Corp".to_string(),
                    vec!["Duplicate entry.".to_string()],
                )],
                supporting: Vec::new(),
            },
        ];
        assert_eq!(
            corpus_pages(&items),
            BTreeMap::from([
                (
                    "alpha-corp".to_string(),
                    "# Alpha Corp\n\nAlpha Corp is a company.\n".to_string(),
                ),
                (
                    "alpha-corp-2".to_string(),
                    "# Alpha. Corp\n\nAnother article.\n".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn renders_article_with_normalized_spacing() {
        assert_eq!(
            render_article(
                "Alpha Corp",
                &[
                    "Alpha Corp is a company.".to_string(),
                    " It was founded in 1990.".to_string(),
                ],
            ),
            "# Alpha Corp\n\nAlpha Corp is a company. It was founded in 1990.\n"
        );
    }
}
