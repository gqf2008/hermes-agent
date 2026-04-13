//! Hermes Agent CLI — main entry point.
//!
//! Replaces the Python `hermes` command (hermes_cli.main:main).
//! Supports subcommands: chat, setup, tools, skills, gateway, doctor, etc.

use clap::{Parser, Subcommand};
use hermes_cli::app::HermesApp;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "hermes", about = "Hermes Agent CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable verbose (debug) logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Hermes home directory override (profiles)
    #[arg(long, global = true)]
    hermes_home: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive chat session with the agent
    Chat {
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
        /// Quiet mode (suppress debug output)
        #[arg(short, long)]
        quiet: bool,
        /// Skip loading context files
        #[arg(long)]
        skip_context_files: bool,
        /// Skip memory loading
        #[arg(long)]
        skip_memory: bool,
        /// Enable voice mode
        #[arg(long)]
        voice: bool,
    },
    /// Interactive setup wizard
    Setup,
    /// Manage tool configurations
    Tools {
        #[command(subcommand)]
        action: Option<ToolAction>,
    },
    /// Manage skill configurations
    Skills {
        #[command(subcommand)]
        action: Option<SkillAction>,
    },
    /// Run the messaging gateway
    Gateway {
        #[command(subcommand)]
        action: Option<GatewayAction>,
    },
    /// Diagnose common configuration issues
    Doctor,
    /// List available models
    Models,
    /// Manage profiles
    Profiles {
        #[command(subcommand)]
        action: Option<ProfileAction>,
    },
    /// Manage conversation sessions
    Sessions {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Parallel batch processing on JSONL datasets
    Batch {
        #[command(subcommand)]
        action: Option<BatchAction>,
    },
    /// Manage scheduled cron jobs
    Cron {
        #[command(subcommand)]
        action: Option<CronAction>,
    },
    /// Manage authentication
    Auth {
        #[command(subcommand)]
        action: Option<AuthAction>,
    },
    /// Log out and clear stored credentials
    Logout,
}

#[derive(Subcommand)]
enum ToolAction {
    /// List all available tools
    List,
    /// Show tool details
    Info { name: String },
}

#[derive(Subcommand)]
enum SkillAction {
    /// List all available skills
    List,
    /// Show skill details
    Info { name: String },
    /// Enable a disabled skill
    Enable {
        /// Skill name
        name: String,
        /// Platform (e.g., cli, telegram, discord)
        #[arg(short, long)]
        platform: Option<String>,
    },
    /// Disable a skill
    Disable {
        /// Skill name
        name: String,
        /// Platform (e.g., cli, telegram, discord)
        #[arg(short, long)]
        platform: Option<String>,
    },
    /// List discovered skill slash commands
    Commands,
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List all profiles
    List,
    /// Create a new profile
    Create { name: String },
    /// Switch to a profile
    Use { name: String },
}

#[derive(Subcommand)]
enum SessionAction {
    /// List recent sessions
    List {
        /// Maximum number of sessions to show
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// Filter by source (e.g., cli, telegram)
        #[arg(short, long)]
        source: Option<String>,
    },
    /// Export a session to JSON
    Export {
        /// Session ID or prefix
        session_id: String,
        /// Output file path
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Delete a session
    Delete {
        /// Session ID or prefix
        session_id: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Search sessions by query
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Show session statistics
    Stats {
        /// Filter by source
        #[arg(short, long)]
        source: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show {
        /// Show full YAML config
        #[arg(long)]
        verbose: bool,
    },
    /// Edit configuration file
    Edit,
    /// Set a configuration value
    Set {
        /// Config key (supports dotted paths, e.g., agent.model)
        key: String,
        /// Value to set
        value: String,
    },
}

#[derive(Subcommand)]
enum BatchAction {
    /// Run batch processing on a JSONL dataset
    Run {
        /// Path to the JSONL dataset file
        dataset: String,
        /// Run name (used for output directory and checkpoint)
        #[arg(short, long)]
        name: Option<String>,
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
        /// Number of prompts per batch
        #[arg(long, default_value_t = 10)]
        batch_size: usize,
        /// Number of parallel workers
        #[arg(long, default_value_t = 4)]
        workers: usize,
        /// Max tool-calling iterations per prompt
        #[arg(long, default_value_t = 90)]
        max_iterations: usize,
        /// Truncate dataset to N samples (0 = all)
        #[arg(long, default_value_t = 0)]
        max_samples: usize,
        /// Resume from checkpoint
        #[arg(long)]
        resume: bool,
        /// Toolset distribution for sampling
        #[arg(long)]
        distribution: Option<String>,
    },
    /// List available toolset distributions
    Distributions,
    /// Show batch run status
    Status {
        /// Run name
        name: String,
    },
}

#[derive(Subcommand)]
enum GatewayAction {
    /// Run gateway in foreground
    Run,
    /// Start gateway as background service
    Start,
    /// Stop gateway service
    Stop,
    /// Show gateway status
    Status,
    /// Install gateway as systemd/launchd service
    Install,
    /// Uninstall gateway service
    Uninstall,
}

#[derive(Subcommand)]
enum CronAction {
    /// List scheduled jobs
    List,
    /// Create a new scheduled job
    Create {
        /// Job name
        name: String,
        /// Cron expression or interval (e.g., "0 9 * * *" or "1h")
        #[arg(short, long)]
        schedule: String,
        /// Command or URL to execute
        #[arg(short, long)]
        command: String,
        /// Delivery platform (e.g., telegram, discord, webhook)
        #[arg(long, default_value = "local")]
        delivery: Option<String>,
        /// Start disabled
        #[arg(long)]
        paused: bool,
    },
    /// Delete a scheduled job
    Delete {
        /// Job ID
        job_id: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Pause a scheduled job
    Pause {
        /// Job ID
        job_id: String,
    },
    /// Resume a paused job
    Resume {
        /// Job ID
        job_id: String,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Add a pooled credential
    Add {
        /// Provider name (e.g., openai, anthropic)
        provider: String,
        /// API key
        #[arg(long)]
        key: String,
        /// Label for this credential
        #[arg(long)]
        label: Option<String>,
    },
    /// List pooled credentials
    List,
    /// Remove a credential by index
    Remove {
        /// Credential index
        index: usize,
    },
    /// Reset exhaustion for a provider
    Reset {
        /// Provider name
        provider: String,
    },
    /// Show auth status
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("debug"))
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .init();
    }

    // Set Hermes home if provided
    if let Some(home) = cli.hermes_home {
        hermes_core::hermes_home::set_hermes_home(&home)
            .ok();
    }

    let app = HermesApp::new()?;

    match cli.command {
        Some(Commands::Chat { model, quiet, skip_context_files, skip_memory, voice }) => {
            app.run_chat(model, quiet, skip_context_files, skip_memory, voice)?;
        }
        Some(Commands::Setup) => {
            app.run_setup()?;
        }
        Some(Commands::Tools { action }) => {
            match action {
                Some(ToolAction::List) => app.list_tools()?,
                Some(ToolAction::Info { name }) => app.show_tool_info(&name)?,
                None => app.list_tools()?,
            }
        }
        Some(Commands::Skills { action }) => {
            match action {
                Some(SkillAction::List) => app.list_skills()?,
                Some(SkillAction::Info { name }) => app.show_skill_info(&name)?,
                Some(SkillAction::Enable { name, platform }) => app.enable_skill(&name, platform.as_deref())?,
                Some(SkillAction::Disable { name, platform }) => app.disable_skill(&name, platform.as_deref())?,
                Some(SkillAction::Commands) => app.list_skill_commands()?,
                None => app.list_skills()?,
            }
        }
        Some(Commands::Gateway { action }) => {
            match action {
                Some(GatewayAction::Run) | None => {
                    app.run_gateway()?;
                }
                Some(GatewayAction::Start) => {
                    println!("Gateway start — stub (not yet migrated)");
                }
                Some(GatewayAction::Stop) => {
                    println!("Gateway stop — stub (not yet migrated)");
                }
                Some(GatewayAction::Status) => {
                    println!("Gateway status — stub (not yet migrated)");
                }
                Some(GatewayAction::Install) => {
                    println!("Gateway install — stub (not yet migrated)");
                }
                Some(GatewayAction::Uninstall) => {
                    println!("Gateway uninstall — stub (not yet migrated)");
                }
            }
        }
        Some(Commands::Doctor) => {
            app.run_doctor()?;
        }
        Some(Commands::Models) => {
            app.list_models()?;
        }
        Some(Commands::Profiles { action }) => {
            match action {
                Some(ProfileAction::List) => app.list_profiles()?,
                Some(ProfileAction::Create { name }) => app.create_profile(&name)?,
                Some(ProfileAction::Use { name }) => app.use_profile(&name)?,
                None => app.list_profiles()?,
            }
        }
        Some(Commands::Sessions { action }) => {
            let db = hermes_state::SessionDB::open_default()?;
            match action {
                Some(SessionAction::List { limit, source }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_list(&db, limit, source.as_deref(), false)?;
                }
                Some(SessionAction::Export { session_id, output }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_export(&db, &session_id, output.as_deref())?;
                }
                Some(SessionAction::Delete { session_id, force }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_delete(&db, &session_id, force)?;
                }
                Some(SessionAction::Search { query, limit }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_search(&db, &query, limit)?;
                }
                Some(SessionAction::Stats { source }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_stats(&db, source.as_deref())?;
                }
                None => {
                    hermes_cli::sessions_cmd::cmd_sessions_list(&db, 20, None, false)?;
                }
            }
        }
        Some(Commands::Config { action }) => {
            match action {
                Some(ConfigAction::Show { verbose }) => {
                    hermes_cli::config_cmd::cmd_config_show(verbose)?;
                }
                Some(ConfigAction::Edit) => {
                    hermes_cli::config_cmd::cmd_config_edit()?;
                }
                Some(ConfigAction::Set { key, value }) => {
                    hermes_cli::config_cmd::cmd_config_set(&key, &value)?;
                }
                None => {
                    hermes_cli::config_cmd::cmd_config_show(false)?;
                }
            }
        }
        Some(Commands::Batch { action }) => {
            match action {
                Some(BatchAction::Run { dataset, name, model, batch_size, workers, max_iterations, max_samples, resume, distribution }) => {
                    let opts = hermes_cli::batch_cmd::BatchRunOptions {
                        dataset,
                        run_name: name,
                        model,
                        batch_size: Some(batch_size),
                        workers: Some(workers),
                        max_iterations: Some(max_iterations),
                        max_samples: Some(max_samples),
                        resume,
                        distribution,
                    };
                    hermes_cli::batch_cmd::cmd_batch_run(&opts)?;
                }
                Some(BatchAction::Distributions) => {
                    hermes_cli::batch_cmd::cmd_batch_distributions()?;
                }
                Some(BatchAction::Status { name }) => {
                    hermes_cli::batch_cmd::cmd_batch_status(&name)?;
                }
                None => {
                    hermes_cli::batch_cmd::cmd_batch_distributions()?;
                }
            }
        }
        Some(Commands::Cron { action }) => {
            match action {
                Some(CronAction::List) => {
                    hermes_cli::cron_cmd::cmd_cron_list()?;
                }
                Some(CronAction::Create { name, schedule, command, delivery, paused }) => {
                    hermes_cli::cron_cmd::cmd_cron_create(&name, &schedule, &command, &delivery.unwrap_or_else(|| "local".to_string()), !paused)?;
                }
                Some(CronAction::Delete { job_id, force }) => {
                    hermes_cli::cron_cmd::cmd_cron_delete(&job_id, force)?;
                }
                Some(CronAction::Pause { job_id }) => {
                    hermes_cli::cron_cmd::cmd_cron_pause(&job_id)?;
                }
                Some(CronAction::Resume { job_id }) => {
                    hermes_cli::cron_cmd::cmd_cron_resume(&job_id)?;
                }
                None => {
                    hermes_cli::cron_cmd::cmd_cron_list()?;
                }
            }
        }
        Some(Commands::Auth { action }) => {
            match action {
                Some(AuthAction::Add { provider, key, label }) => {
                    hermes_cli::auth_cmd::cmd_auth_add(&provider, &key, label.as_deref())?;
                }
                Some(AuthAction::List) => {
                    hermes_cli::auth_cmd::cmd_auth_list()?;
                }
                Some(AuthAction::Remove { index }) => {
                    hermes_cli::auth_cmd::cmd_auth_remove(index)?;
                }
                Some(AuthAction::Reset { provider }) => {
                    hermes_cli::auth_cmd::cmd_auth_reset(&provider)?;
                }
                Some(AuthAction::Status) => {
                    hermes_cli::auth_cmd::cmd_auth_status()?;
                }
                None => {
                    hermes_cli::auth_cmd::cmd_auth_status()?;
                }
            }
        }
        Some(Commands::Logout) => {
            hermes_cli::auth_cmd::cmd_logout()?;
        }
        None => {
            // Default: interactive chat
            app.run_chat(None, false, false, false, false)?;
        }
    }

    Ok(())
}
