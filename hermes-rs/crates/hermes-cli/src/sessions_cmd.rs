//! Session management commands for the Hermes CLI.
//!
//! Mirrors the Python `hermes sessions` subcommand.

use console::style;
use hermes_state::SessionDB;

/// List recent sessions.
pub fn cmd_sessions_list(
    db: &SessionDB,
    limit: usize,
    source: Option<&str>,
    _verbose: bool,
) -> anyhow::Result<()> {
    let sessions = db.list_sessions_rich(source, None, limit, 0, true)?;

    if sessions.is_empty() {
        println!("{}", style("No sessions found.").dim());
        return Ok(());
    }

    println!(
        "{}",
        style(format!("{:^8}  {:^20}  {:12}  {:>6}  {:>6}  {:>5}  {}",
            "ID", "Title", "Model", "In", "Out", "Calls", "Preview"))
        .bold()
    );
    println!("{}", style("-".repeat(100)).dim());

    for sp in &sessions {
        let session = &sp.session;
        let short_id = &session.id[..8.min(session.id.len())];
        let title = session.title.as_deref().unwrap_or("(untitled)");
        let model = session
            .model
            .as_deref()
            .unwrap_or("(default)")
            .split('/')
            .next_back()
            .unwrap_or("?");
        let preview: String = sp.preview.chars().take(50).collect();

        println!(
            "{:<8}  {:20}  {:12}  {:>6}  {:>6}  {:>5}  {}",
            style(short_id).cyan(),
            style(title).dim(),
            model,
            session.input_tokens,
            session.output_tokens,
            session.tool_call_count,
            style(preview).dim(),
        );
    }

    println!("\n{} session(s) shown", sessions.len());
    Ok(())
}

/// Export a session to JSON.
pub fn cmd_sessions_export(
    db: &SessionDB,
    session_id: &str,
    output: Option<&str>,
) -> anyhow::Result<()> {
    // Try prefix resolution first
    let resolved = db.resolve_session_id(session_id)?;
    let sid = resolved.as_deref().unwrap_or(session_id);

    let export = db.export_session(sid)?;
    match export {
        Some(data) => {
            let json = serde_json::to_string_pretty(&data)?;
            if let Some(path) = output {
                std::fs::write(path, json)?;
                println!("Exported session {} to {}", style(sid).cyan(), style(path).green());
            } else {
                println!("{json}");
            }
        }
        None => {
            println!("{}", style(format!("Session {} not found.", sid)).red());
        }
    }
    Ok(())
}

/// Delete a session.
pub fn cmd_sessions_delete(
    db: &SessionDB,
    session_id: &str,
    force: bool,
) -> anyhow::Result<()> {
    let resolved = db.resolve_session_id(session_id)?;
    let sid = resolved.as_deref().unwrap_or(session_id);

    if !force {
        print!("Delete session {}? [y/N] ", style(sid).yellow());
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    match db.delete_session(sid) {
        Ok(true) => println!("{} Deleted session {}", style("[OK]").green(), style(sid).cyan()),
        Ok(false) => println!("{}", style(format!("Session {} not found.", sid)).red()),
        Err(e) => println!("{}", style(format!("Error deleting session: {e}")).red()),
    }
    Ok(())
}

/// Search sessions by query (FTS5).
pub fn cmd_sessions_search(
    db: &SessionDB,
    query: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let sessions = db.search_sessions(None, limit, 0)?;

    if sessions.is_empty() {
        println!("{}", style("No matching sessions found.").dim());
        return Ok(());
    }

    println!(
        "{}",
        style(format!("{:^8}  {:20}  {:12}  {}",
            "ID", "Title", "Model", "Query Match"))
        .bold()
    );
    println!("{}", style("-".repeat(80)).dim());

    for session in &sessions {
        let short_id = &session.id[..8.min(session.id.len())];
        let title = session.title.as_deref().unwrap_or("(untitled)");
        let model = session
            .model
            .as_deref()
            .unwrap_or("(default)")
            .split('/')
            .next_back()
            .unwrap_or("?");

        // Get matching message preview
        let matches = db.search_messages(query, None, None, None, 1, 0)?;
        let preview = matches
            .first()
            .and_then(|m| m.get("content").and_then(|v| v.as_str()))
            .map(|c| {
                let truncated: String = c.chars().take(60).collect();
                truncated
            })
            .unwrap_or_default();

        println!(
            "{:<8}  {:20}  {:12}  {}",
            style(short_id).cyan(),
            style(title).dim(),
            model,
            style(preview).dim(),
        );
    }

    println!("\n{} session(s) matched", sessions.len());
    Ok(())
}

/// Show session statistics.
pub fn cmd_sessions_stats(db: &SessionDB, source: Option<&str>) -> anyhow::Result<()> {
    let total_sessions = db.session_count(source)?;
    let total_messages = db.message_count(None)?;

    println!("{}", style("Session Statistics").bold());
    println!("{}", "-".repeat(40));
    println!(
        "Total sessions: {}",
        style(total_sessions).cyan()
    );
    println!(
        "Total messages: {}",
        style(total_messages).cyan()
    );

    if total_sessions > 0 {
        let sessions = db.list_sessions_rich(source, None, 1, 0, false)?;
        if let Some(sp) = sessions.first() {
            let s = &sp.session;
            println!(
                "Sources: {}",
                style(s.source.clone()).cyan()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_empty_db() {
        let db = SessionDB::open(":memory:").unwrap();
        let result = cmd_sessions_list(&db, 10, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_export_not_found() {
        let db = SessionDB::open(":memory:").unwrap();
        let result = cmd_sessions_export(&db, "nonexistent", None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_not_found() {
        let db = SessionDB::open(":memory:").unwrap();
        let result = cmd_sessions_delete(&db, "nonexistent", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_stats_empty() {
        let db = SessionDB::open(":memory:").unwrap();
        let result = cmd_sessions_stats(&db, None);
        assert!(result.is_ok());
    }
}
