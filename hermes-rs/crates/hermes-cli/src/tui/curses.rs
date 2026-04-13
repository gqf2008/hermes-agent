//! Curses-based TUI components for Hermes CLI.
//!
//! Interactive menus for model selection, file picking, and session browsing.

use console::{style, Style};
use std::io::{self, Write};

/// Display an interactive menu and return the selected index.
///
/// Uses keyboard navigation: numbered selection, Enter to confirm,
/// q to cancel.
pub fn show_menu(
    title: &str,
    items: &[String],
    allow_cancel: bool,
) -> io::Result<Option<usize>> {
    if items.is_empty() {
        return Ok(None);
    }

    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let dim = Style::new().dim();

    println!("\n{} {}", green.apply_to(">"), title);
    println!("{}", dim.apply_to(&"-".repeat(title.len() + 2)));

    for (i, item) in items.iter().enumerate() {
        println!("  {} {}", yellow.apply_to(format!("{:>2}.", i + 1)), item);
    }
    println!();

    if allow_cancel {
        print!("Select (1-{}/q): ", items.len());
    } else {
        print!("Select (1-{}): ", items.len());
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.eq_ignore_ascii_case("q") {
        return Ok(None);
    }

    match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= items.len() => Ok(Some(n - 1)),
        _ => Ok(None),
    }
}

/// Display available models grouped by provider.
pub fn show_model_selector(models: &[String], current: Option<&str>) {
    let green = Style::new().green();
    let dim = Style::new().dim();

    println!("\n{} Available Models", green.apply_to(">"));

    if let Some(cur) = current {
        println!("  {} Current: {}", dim.apply_to("->"), cur);
    }
    println!();

    let mut providers: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for model in models {
        if let Some((provider, rest)) = model.split_once('/') {
            providers.entry(provider.to_string()).or_default().push(rest.to_string());
        } else {
            providers.entry("other".to_string()).or_default().push(model.clone());
        }
    }

    for (provider, names) in &providers {
        println!("  {}", style(provider).bold());
        for name in names {
            let full = format!("{provider}/{name}");
            let marker = if current == Some(full.as_str()) {
                green.apply_to(" (current)").to_string()
            } else {
                String::new()
            };
            println!("    - {name}{marker}");
        }
    }
}

/// Display a session browser showing recent sessions.
pub fn show_session_list(sessions: &[(String, String, String)]) {
    // (id, title, last_message_preview)
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let dim = Style::new().dim();

    println!("\n{} Recent Sessions", green.apply_to(">"));
    println!("{}", dim.apply_to(&"-".repeat(50)));

    if sessions.is_empty() {
        println!("  {}", dim.apply_to("No sessions found."));
        return;
    }

    for (id, title, preview) in sessions {
        let short_id = if id.len() > 8 { &id[..8] } else { id };
        println!("  {} {}  {}",
            yellow.apply_to(short_id),
            style(title).bold(),
            dim.apply_to(preview),
        );
    }
}

/// Display a skill browser with categories.
pub fn show_skills_list(categories: &[(String, Vec<(String, String)>)]) {
    // (category_name, [(skill_name, description)])
    let green = Style::new().green();
    let dim = Style::new().dim();

    println!("\n{} Skills", green.apply_to(">"));

    for (cat, skills) in categories {
        println!("\n  {}", style(cat).bold().underlined());
        if skills.is_empty() {
            println!("    {}", dim.apply_to("(none)"));
        }
        for (name, desc) in skills {
            println!("    {}  {}", style(name).cyan(), dim.apply_to(desc));
        }
    }
}

/// Display a confirmation prompt with colored default.
pub fn confirm(prompt_text: &str, default: bool) -> io::Result<bool> {
    let dim = Style::new().dim();

    let default_label = if default { "Y/n" } else { "y/N" };
    print!("{} [{}]: ", prompt_text, dim.apply_to(default_label));
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    if input.is_empty() {
        return Ok(default);
    }

    Ok(input == "y" || input == "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_show_menu_empty() {
        let result = show_menu("Empty", &[], true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_show_session_list_empty() {
        show_session_list(&[]);
    }

    #[test]
    fn test_show_skills_list_empty() {
        show_skills_list(&[]);
    }

    #[test]
    fn test_show_model_selector_empty() {
        show_model_selector(&[], None);
    }
}
