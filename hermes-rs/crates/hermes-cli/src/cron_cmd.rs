//! Cron management subcommands.
//!
//! Mirrors Python: hermes cron list/create/pause/resume/delete

use console::Style;

use hermes_cron::jobs::JobStore;

fn get_store() -> Option<JobStore> {
    JobStore::new().ok()
}

/// List all cron jobs.
pub fn cmd_cron_list() -> anyhow::Result<()> {
    let cyan = Style::new().cyan();
    let green = Style::new().green();
    let dim = Style::new().dim();
    let yellow = Style::new().yellow();

    let store = match get_store() {
        Some(s) => s,
        None => {
            println!();
            println!("{}", cyan.apply_to("◆ Scheduled Jobs"));
            println!();
            println!("  {}", dim.apply_to("No cron jobs scheduled."));
            println!("  Create one with: hermes cron create");
            println!();
            return Ok(());
        }
    };

    let jobs = store.list(false);

    println!();
    println!("{}", cyan.apply_to("◆ Scheduled Jobs"));
    println!();

    if jobs.is_empty() {
        println!("  {}", dim.apply_to("No cron jobs scheduled."));
        println!("  Create one with: hermes cron create");
        println!();
        return Ok(());
    }

    println!("{:<14} {:<20} {:<20} {:<10} {:<20}", "ID", "Name", "Schedule", "Status", "Next Run");
    println!("{}", "-".repeat(90));

    for job in &jobs {
        let status = if job.enabled {
            green.apply_to("active").to_string()
        } else {
            yellow.apply_to("paused").to_string()
        };
        let next = job.next_run_at.as_deref().unwrap_or("unknown").to_string();

        println!("{:<14} {:<20} {:<20} {:<10} {}", job.id, job.name, job.schedule_display, status, next);
    }
    println!();
    println!("  Total: {} job(s)", jobs.len());
    println!();

    Ok(())
}

/// Create a new cron job.
pub fn cmd_cron_create(
    name: &str,
    schedule: &str,
    command: &str,
    delivery: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    let green = Style::new().green();
    let cyan = Style::new().cyan();

    let mut store = get_store().ok_or_else(|| anyhow::anyhow!("Failed to initialize cron store"))?;

    let job = store.create(command, schedule, Some(name))?;
    if !enabled {
        store.pause(&job.id, Some("created paused"))?;
    }

    println!();
    println!("{}", cyan.apply_to("◆ Cron Job Created"));
    println!("  {} Job '{name}' created with ID: {}", green.apply_to("✓"), job.id);
    println!("  Schedule:   {}", job.schedule_display);
    println!("  Command:    {command}");
    println!("  Delivery:   {delivery}");
    println!("  Enabled:    {enabled}");
    println!();
    println!("  Start the scheduler with: hermes cron start");
    println!();

    Ok(())
}

/// Delete a cron job.
pub fn cmd_cron_delete(job_id: &str, _force: bool) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let mut store = get_store().ok_or_else(|| anyhow::anyhow!("Failed to initialize cron store"))?;

    if store.get(job_id).is_some() {
        store.remove(job_id)?;
        println!("  {} Job '{job_id}' deleted.", green.apply_to("✓"));
    } else {
        println!("  {} Job '{job_id}' not found.", yellow.apply_to("✗"));
    }
    println!();

    Ok(())
}

/// Pause a cron job.
pub fn cmd_cron_pause(job_id: &str) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let mut store = get_store().ok_or_else(|| anyhow::anyhow!("Failed to initialize cron store"))?;

    if store.get(job_id).is_some() {
        store.pause(job_id, None)?;
        println!("  {} Job '{job_id}' paused.", green.apply_to("✓"));
    } else {
        println!("  {} Job '{job_id}' not found.", yellow.apply_to("✗"));
    }
    println!();

    Ok(())
}

/// Resume a cron job.
pub fn cmd_cron_resume(job_id: &str) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let mut store = get_store().ok_or_else(|| anyhow::anyhow!("Failed to initialize cron store"))?;

    if let Some(_job) = store.get(job_id) {
        store.resume(job_id)?;
        println!("  {} Job '{job_id}' resumed.", green.apply_to("✓"));
    } else {
        println!("  {} Job '{job_id}' not found.", yellow.apply_to("✗"));
    }
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_store_returns_some() {
        // JobStore::new() creates a directory, so it should succeed
        let result = get_store();
        assert!(result.is_some());
    }
}
