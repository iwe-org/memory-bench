use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub exact_match: u8,
    pub f1: f64,
    pub bleu1: f64,
}

pub fn simple_tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['.', ',', '!', '?'], " ")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn counts(tokens: &[String]) -> BTreeMap<&str, u64> {
    let mut map = BTreeMap::new();
    for token in tokens {
        *map.entry(token.as_str()).or_insert(0) += 1;
    }
    map
}

fn bleu1(prediction_tokens: &[String], reference_tokens: &[String]) -> f64 {
    if prediction_tokens.is_empty() || reference_tokens.is_empty() {
        return 0.0;
    }
    let reference_counts = counts(reference_tokens);
    let clipped: u64 = counts(prediction_tokens)
        .iter()
        .map(|(token, count)| (*count).min(*reference_counts.get(token).unwrap_or(&0)))
        .sum();
    let precision = clipped as f64 / prediction_tokens.len() as f64;
    let brevity_penalty = if prediction_tokens.len() >= reference_tokens.len() {
        1.0
    } else {
        (1.0 - reference_tokens.len() as f64 / prediction_tokens.len() as f64).exp()
    };
    brevity_penalty * precision
}

pub fn calculate(prediction: &str, reference: &str) -> Metrics {
    let prediction = prediction.trim();
    let reference = reference.trim();
    if prediction.is_empty() || reference.is_empty() {
        return Metrics {
            exact_match: 0,
            f1: 0.0,
            bleu1: 0.0,
        };
    }
    let exact_match = u8::from(prediction.to_lowercase() == reference.to_lowercase());
    let prediction_tokens = simple_tokenize(prediction);
    let reference_tokens = simple_tokenize(reference);
    let prediction_set: BTreeSet<&String> = prediction_tokens.iter().collect();
    let reference_set: BTreeSet<&String> = reference_tokens.iter().collect();
    let common = prediction_set.intersection(&reference_set).count() as f64;
    let f1 = if prediction_set.is_empty() || reference_set.is_empty() || common == 0.0 {
        0.0
    } else {
        let precision = common / prediction_set.len() as f64;
        let recall = common / reference_set.len() as f64;
        2.0 * precision * recall / (precision + recall)
    };
    Metrics {
        exact_match,
        f1,
        bleu1: bleu1(&prediction_tokens, &reference_tokens),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_answer() {
        assert_eq!(
            calculate("the cat sat on the mat", "the cat sat on the mat"),
            Metrics {
                exact_match: 1,
                f1: 1.0,
                bleu1: 1.0,
            }
        );
    }

    #[test]
    fn partial_answer() {
        assert_eq!(
            calculate("A shell necklace", "The shell necklace"),
            Metrics {
                exact_match: 0,
                f1: 2.0 / 3.0,
                bleu1: 2.0 / 3.0,
            }
        );
    }

    #[test]
    fn empty_prediction() {
        assert_eq!(
            calculate("", "a red bicycle"),
            Metrics {
                exact_match: 0,
                f1: 0.0,
                bleu1: 0.0,
            }
        );
    }

    #[test]
    fn punctuation_and_case() {
        assert_eq!(
            calculate("7 May, 2023", "7 May 2023"),
            Metrics {
                exact_match: 0,
                f1: 1.0,
                bleu1: 1.0,
            }
        );
    }
}
