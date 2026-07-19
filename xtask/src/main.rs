mod answer;
mod claude;
mod clean;
mod curate;
mod doctor;
mod enrich;
mod hotpot;
mod judge;
mod locomo;
mod metrics;
mod prepare;
mod records;
mod report;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use anyhow::Result;
use clap::{Parser, Subcommand};

use answer::{AnswerConfig, Arm, Dataset, Split};
use curate::CurateConfig;
use enrich::EnrichConfig;
use hotpot::IngestConfig;
use judge::JudgeConfig;

const LOCOMO_URL: &str =
    "https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json";
const HOTPOT_URL: &str =
    "http://web.archive.org/web/20260310132809id_/http://curtis.ml.cmu.edu/datasets/hotpot/hotpot_dev_distractor_v1.json";

#[derive(Parser)]
#[command(name = "xtask", about = "IWE agent-memory benchmark harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Download {
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
        #[arg(long, value_enum, default_value_t = Dataset::Locomo)]
        dataset: Dataset,
        #[arg(long)]
        force: bool,
    },
    Ingest {
        #[arg(long, default_value = "data/hotpot_dev_distractor_v1.json")]
        data: PathBuf,
        #[arg(long, default_value = "workspaces")]
        workspaces: PathBuf,
        #[arg(long, default_value_t = 50)]
        dev_questions: usize,
        #[arg(long, default_value_t = 300)]
        test_questions: usize,
        #[arg(long)]
        linked: bool,
        #[arg(long)]
        force: bool,
    },
    Enrich {
        #[arg(long, default_value = "workspaces")]
        workspaces: PathBuf,
        #[arg(long, default_value = "corpus-linked")]
        source: String,
        #[arg(long, default_value = "corpus-agentic")]
        target: String,
        #[arg(long, default_value = "claude-haiku-4-5-20251001")]
        model: String,
        #[arg(long, default_value_t = 15)]
        candidates: usize,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        replay: Option<PathBuf>,
        #[arg(long, default_value_t = 4)]
        workers: usize,
        #[arg(long, default_value_t = 0.05)]
        max_budget_usd: f64,
        #[arg(long, default_value_t = 120)]
        timeout_secs: u64,
    },
    Prepare {
        #[arg(long, default_value = "data/locomo10.json")]
        data: PathBuf,
        #[arg(long, default_value = "workspaces")]
        workspaces: PathBuf,
        #[arg(long)]
        conversations: Option<String>,
    },
    Curate {
        #[arg(long, default_value = "data/locomo10.json")]
        data: PathBuf,
        #[arg(long, default_value = "workspaces")]
        workspaces: PathBuf,
        #[arg(long)]
        conversations: Option<String>,
        #[arg(long, default_value = "claude-haiku-4-5-20251001")]
        model: String,
        #[arg(long, default_value_t = 2)]
        workers: usize,
        #[arg(long, default_value_t = 3.0)]
        max_budget_usd: f64,
        #[arg(long, default_value_t = 900)]
        timeout_secs: u64,
        #[arg(long, default_value_t = false)]
        consolidate: bool,
    },
    Answer {
        #[arg(long)]
        run: PathBuf,
        #[arg(long, value_enum, default_value_t = Dataset::Locomo)]
        dataset: Dataset,
        #[arg(long, value_enum)]
        arm: Arm,
        #[arg(long, default_value = "claude-sonnet-4-6")]
        model: String,
        #[arg(long, default_value = "data/locomo10.json")]
        data: PathBuf,
        #[arg(long, default_value = "workspaces")]
        workspaces: PathBuf,
        #[arg(long, default_value = "1,2,3,4")]
        categories: String,
        #[arg(long)]
        conversations: Option<String>,
        #[arg(long, value_enum, conflicts_with = "conversations")]
        split: Option<Split>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, default_value_t = answer::DOSSIER_LIMIT)]
        dossier_limit: usize,
        #[arg(long, default_value = "corpus")]
        corpus: String,
        #[arg(long)]
        anchors: bool,
        #[arg(long, default_value_t = 2)]
        workers: usize,
        #[arg(long, default_value_t = 0.5)]
        max_budget_usd: f64,
        #[arg(long, default_value_t = 600)]
        timeout_secs: u64,
    },
    Judge {
        #[arg(long)]
        run: PathBuf,
        #[arg(long, default_value = "claude-sonnet-4-6")]
        judge_model: String,
        #[arg(long, default_value_t = 2)]
        workers: usize,
        #[arg(long, default_value_t = 0.25)]
        max_budget_usd: f64,
        #[arg(long, default_value_t = 180)]
        timeout_secs: u64,
    },
    Report {
        #[arg(long)]
        run: PathBuf,
    },
    Doctor {
        #[arg(long, default_value = "claude-haiku-4-5-20251001")]
        model: String,
    },
    Clean {
        #[arg(long, default_value = "workspaces")]
        workspaces: PathBuf,
        #[arg(long, default_value = "curated")]
        kind: String,
        #[arg(long)]
        conversations: Option<String>,
    },
}

fn parse_categories(text: &str) -> Result<BTreeSet<u8>> {
    text.split(',')
        .map(|c| Ok(c.trim().parse()?))
        .collect()
}

fn parse_filter(text: &Option<String>) -> Option<BTreeSet<String>> {
    text.as_ref().map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

fn download(data_dir: &PathBuf, dataset: Dataset, force: bool) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let (url, filename) = match dataset {
        Dataset::Locomo => (LOCOMO_URL, "locomo10.json"),
        Dataset::Hotpot => (HOTPOT_URL, "hotpot_dev_distractor_v1.json"),
    };
    let target = data_dir.join(filename);
    if target.exists() && !force {
        println!("{} already exists", target.display());
        return Ok(());
    }
    let status = ProcessCommand::new("curl")
        .args(["-sfL", url, "-o"])
        .arg(&target)
        .status()?;
    anyhow::ensure!(status.success(), "download failed");
    println!(
        "downloaded {} ({} bytes)",
        target.display(),
        target.metadata()?.len()
    );
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Download {
            data_dir,
            dataset,
            force,
        } => download(&data_dir, dataset, force),
        Command::Ingest {
            data,
            workspaces,
            dev_questions,
            test_questions,
            linked,
            force,
        } => hotpot::ingest(&IngestConfig {
            data,
            workspaces,
            dev_questions,
            test_questions,
            linked,
            force,
        }),
        Command::Enrich {
            workspaces,
            source,
            target,
            model,
            candidates,
            limit,
            replay,
            workers,
            max_budget_usd,
            timeout_secs,
        } => enrich::run(&EnrichConfig {
            workspaces,
            source,
            target,
            model,
            candidates,
            limit,
            replay,
            workers,
            max_budget_usd,
            timeout_secs,
        }),
        Command::Prepare {
            data,
            workspaces,
            conversations,
        } => prepare::prepare(&data, &workspaces, parse_filter(&conversations).as_ref()),
        Command::Curate {
            data,
            workspaces,
            conversations,
            model,
            workers,
            max_budget_usd,
            timeout_secs,
            consolidate,
        } => curate::run(&CurateConfig {
            data,
            workspaces,
            conversation_filter: parse_filter(&conversations),
            model,
            workers,
            max_budget_usd,
            timeout_secs,
            consolidate,
        }),
        Command::Answer {
            run,
            dataset,
            arm,
            model,
            data,
            workspaces,
            categories,
            conversations,
            split,
            limit,
            dossier_limit,
            corpus,
            anchors,
            workers,
            max_budget_usd,
            timeout_secs,
        } => {
            answer::run(&AnswerConfig {
                run: run.clone(),
                dataset,
                arm,
                model,
                data,
                workspaces,
                categories: parse_categories(&categories)?,
                conversation_filter: split
                    .map(Split::conversations)
                    .or_else(|| parse_filter(&conversations)),
                split,
                limit,
                dossier_limit,
                corpus,
                anchors,
                workers,
                max_budget_usd,
                timeout_secs,
            })?;
            report::print(&report::build(&run)?);
            Ok(())
        }
        Command::Judge {
            run,
            judge_model,
            workers,
            max_budget_usd,
            timeout_secs,
        } => {
            judge::run(&JudgeConfig {
                run: run.clone(),
                judge_model,
                workers,
                max_budget_usd,
                timeout_secs,
            })?;
            report::print(&report::build(&run)?);
            Ok(())
        }
        Command::Report { run } => {
            report::print(&report::build(&run)?);
            Ok(())
        }
        Command::Doctor { model } => doctor::run(&model),
        Command::Clean {
            workspaces,
            kind,
            conversations,
        } => clean::run(&clean::CleanConfig {
            workspaces,
            kinds: clean::resolve_kinds(&kind),
            conversations: parse_filter(&conversations),
        }),
    }
}
