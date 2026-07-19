use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::locomo::{self, Conversation, Session};

pub fn render_session(session: &Session) -> String {
    let mut out = format!("# Session {} — {}\n", session.number, session.timestamp);
    for turn in &session.turns {
        out.push('\n');
        out.push_str(&format!("{}: {}", turn.speaker, turn.text));
        if let Some(caption) = &turn.blip_caption {
            out.push_str(&format!(" [shared photo: {caption}]"));
        }
        out.push('\n');
    }
    out
}

pub fn render_index(conversation: &Conversation) -> String {
    let mut out = format!(
        "# Conversation between {} and {}\n",
        conversation.speaker_a, conversation.speaker_b
    );
    for session in &conversation.sessions {
        out.push_str(&format!(
            "\n[Session {} — {}](sessions/session-{:02})\n",
            session.number, session.timestamp, session.number
        ));
    }
    out
}

pub fn render_transcript(conversation: &Conversation) -> String {
    let mut lines = Vec::new();
    for session in &conversation.sessions {
        for turn in &session.turns {
            let mut line = format!("{} | {}: {}", session.timestamp, turn.speaker, turn.text);
            if let Some(caption) = &turn.blip_caption {
                line.push_str(&format!(" [shared photo: {caption}]"));
            }
            lines.push(line);
        }
    }
    lines.join("\n")
}

pub fn resolve_iwe() -> Result<String> {
    resolve_bin("IWE_BIN", "iwe")
}

fn resolve_bin(env_key: &str, name: &str) -> Result<String> {
    if let Ok(path) = std::env::var(env_key) {
        return Ok(path);
    }
    let output = Command::new("which").arg(name).output()?;
    anyhow::ensure!(
        output.status.success(),
        "{name} not found on PATH; set {env_key}"
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn write_content(dir: &Path, conversation: &Conversation) -> Result<()> {
    let sessions_dir = dir.join("sessions");
    std::fs::create_dir_all(&sessions_dir)?;
    for session in &conversation.sessions {
        std::fs::write(
            sessions_dir.join(format!("session-{:02}.md", session.number)),
            render_session(session),
        )?;
    }
    std::fs::write(dir.join("index.md"), render_index(conversation))?;
    Ok(())
}

const SESSION_SCHEMA: &str = include_str!("../../docs/store-schemas/session.yaml");
const HUB_SCHEMA: &str = include_str!("../../docs/store-schemas/hub.yaml");
const SCHEMA_BINDINGS: &str = r#"
[schemas.session]
match = "[0-9]*"

[schemas.hub]
match = "[a-z]*"
"#;

pub fn init_iwe_bare(dir: &Path) -> Result<()> {
    if dir.join(".iwe").exists() {
        return Ok(());
    }
    let iwe_bin = resolve_bin("IWE_BIN", "iwe")?;
    let status = Command::new(&iwe_bin)
        .arg("init")
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to run {iwe_bin} init"))?;
    anyhow::ensure!(status.success(), "iwe init failed in {}", dir.display());
    Ok(())
}

pub fn init_iwe(dir: &Path) -> Result<()> {
    if dir.join(".iwe").exists() {
        return Ok(());
    }
    init_iwe_bare(dir)?;
    let schemas_dir = dir.join(".iwe").join("schemas");
    std::fs::create_dir_all(&schemas_dir)?;
    std::fs::write(schemas_dir.join("session.yaml"), SESSION_SCHEMA)?;
    std::fs::write(schemas_dir.join("hub.yaml"), HUB_SCHEMA)?;
    let config_path = dir.join(".iwe").join("config.toml");
    let mut config = std::fs::read_to_string(&config_path)?;
    config.push_str(SCHEMA_BINDINGS);
    std::fs::write(&config_path, config)?;
    Ok(())
}

pub fn write_mcp_config(dir: &Path) -> Result<()> {
    let iwec_bin = resolve_bin("IWEC_BIN", "iwec")?;
    let config = serde_json::json!({
        "mcpServers": {
            "iwe": {
                "command": iwec_bin,
                "args": []
            }
        }
    });
    std::fs::write(
        dir.join(".mcp.json"),
        serde_json::to_string_pretty(&config)? + "\n",
    )?;
    Ok(())
}

pub fn prepare(
    data: &Path,
    workspaces: &Path,
    conversation_filter: Option<&BTreeSet<String>>,
) -> Result<()> {
    let conversations = locomo::load(data, None)?;
    for conversation in &conversations {
        if conversation_filter.map(|f| f.contains(&conversation.sample_id)) == Some(false) {
            continue;
        }
        let fs_dir = workspaces.join("fs").join(&conversation.sample_id);
        write_content(&fs_dir, conversation)?;
        let iwe_dir = workspaces.join("iwe").join(&conversation.sample_id);
        write_content(&iwe_dir, conversation)?;
        init_iwe(&iwe_dir)?;
        write_mcp_config(&iwe_dir)?;
        println!(
            "{}: {} sessions",
            conversation.sample_id,
            conversation.sessions.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locomo::{Qa, Turn};

    fn conversation() -> Conversation {
        Conversation {
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
            qa: Vec::<Qa>::new(),
        }
    }

    #[test]
    fn renders_session() {
        assert_eq!(
            render_session(&conversation().sessions[0]),
            "# Session 1 — 1:00 pm on 1 May, 2023\n\
             \n\
             Alice: Hi Bob!\n\
             \n\
             Bob: Look at this! [shared photo: a red bicycle]\n"
        );
    }

    #[test]
    fn renders_index() {
        assert_eq!(
            render_index(&conversation()),
            "# Conversation between Alice and Bob\n\
             \n\
             [Session 1 — 1:00 pm on 1 May, 2023](sessions/session-01)\n\
             \n\
             [Session 2 — 2:00 pm on 2 May, 2023](sessions/session-02)\n"
        );
    }

    #[test]
    fn renders_transcript() {
        assert_eq!(
            render_transcript(&conversation()),
            "1:00 pm on 1 May, 2023 | Alice: Hi Bob!\n\
             1:00 pm on 1 May, 2023 | Bob: Look at this! [shared photo: a red bicycle]\n\
             2:00 pm on 2 May, 2023 | Alice: I got a new bike."
        );
    }
}
