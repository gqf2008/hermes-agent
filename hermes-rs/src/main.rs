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

    /// Profile name (resolves HERMES_HOME before subcommands)
    #[arg(short = 'p', long, global = true)]
    profile: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Interactive chat session with the agent
    Chat {
        /// Model to use
        #[arg(short, long)]
        model: Option<String>,
        /// Single query (non-interactive mode)
        #[arg(short = 'q', long)]
        query: Option<String>,
        /// Optional local image path to attach to a single query
        #[arg(long)]
        image: Option<String>,
        /// Comma-separated toolsets to enable
        #[arg(short = 't', long)]
        toolsets: Option<String>,
        /// Preload one or more skills (comma-separated)
        #[arg(short = 's', long)]
        skills: Option<String>,
        /// Inference provider selection (default: auto)
        #[arg(long)]
        provider: Option<String>,
        /// Resume a previous session by ID
        #[arg(short = 'r', long)]
        resume: Option<String>,
        /// Resume last session (or by name if provided)
        #[arg(short = 'c', long)]
        continue_last: Option<Option<String>>,
        /// Run in an isolated git worktree
        #[arg(short = 'w', long)]
        worktree: bool,
        /// Enable filesystem checkpoints before destructive file operations
        #[arg(long)]
        checkpoints: bool,
        /// Maximum tool-calling iterations per turn
        #[arg(long)]
        max_turns: Option<u32>,
        /// Bypass all dangerous command approval prompts
        #[arg(long)]
        yolo: bool,
        /// Include session ID in system prompt
        #[arg(long)]
        pass_session_id: bool,
        /// Session source tag (default: cli)
        #[arg(long)]
        source: Option<String>,
        /// Quiet mode (suppress debug output)
        #[arg(long)]
        quiet: bool,
        /// Verbose output (show tool previews, debug info)
        #[arg(short = 'v', long)]
        verbose: bool,
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
    Setup {
        /// Section to configure (model, terminal, agent, gateway, tools, tts)
        section: Option<String>,
        /// Non-interactive mode (use defaults/env vars)
        #[arg(long)]
        non_interactive: bool,
        /// Reset configuration to defaults
        #[arg(long)]
        reset: bool,
    },
    /// Backup Hermes state
    Backup {
        /// Output directory (default: current dir)
        #[arg(short, long)]
        output: Option<String>,
        /// Include session database
        #[arg(long)]
        include_sessions: bool,
        /// Quick snapshot (critical state only)
        #[arg(short, long)]
        quick: bool,
        /// Snapshot label
        #[arg(short, long)]
        label: Option<String>,
    },
    /// Restore from a backup
    Restore {
        /// Backup directory path
        path: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// List available backups
    BackupList,
    /// Print debug info
    Debug,
    /// Generate and share debug report
    DebugShare {
        /// Number of log lines to include
        #[arg(short = 'n', long, default_value_t = 200)]
        lines: usize,
        /// Expiration in days
        #[arg(long, default_value_t = 7)]
        expire_days: usize,
        /// Print locally only (don't upload)
        #[arg(long)]
        local_only: bool,
    },
    /// Dump session data for debugging
    Dump {
        /// Session ID or prefix
        session_id: Option<String>,
        /// Show redacted API key prefixes
        #[arg(long)]
        show_keys: bool,
    },
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
    Doctor {
        /// Auto-fix detected issues
        #[arg(long)]
        fix: bool,
    },
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
    /// Manage CLI skins/themes
    Skin {
        #[command(subcommand)]
        action: Option<SkinAction>,
    },
    /// Show status of all components
    Status {
        /// Show all redacted details
        #[arg(long)]
        all: bool,
        /// Run deep checks (slower)
        #[arg(long)]
        deep: bool,
    },
    /// Show session analytics and insights
    Insights {
        /// Number of days to analyze (default: 30)
        #[arg(long, default_value_t = 30)]
        days: usize,
        /// Filter by platform/source
        #[arg(long)]
        source: Option<String>,
    },
    /// Generate shell completion script
    Completion {
        /// Shell type: bash, zsh, fish, elvish, powershell
        #[arg(short, long, default_value = "bash")]
        shell: String,
    },
    /// Show version information
    Version,
    /// View and filter log files
    Logs {
        /// Log to view: agent (default), errors, gateway, or 'list'
        log_name: Option<String>,
        /// Number of lines to show
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Follow log in real time
        #[arg(short, long)]
        follow: bool,
        /// Minimum log level
        #[arg(long)]
        level: Option<String>,
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Filter by component
        #[arg(long)]
        component: Option<String>,
        /// Show lines since time ago (e.g. 1h, 30m)
        #[arg(long)]
        since: Option<String>,
    },
    /// Manage webhook subscriptions
    Webhook {
        #[command(subcommand)]
        action: WebhookAction,
    },
    /// Manage plugins
    Plugins {
        #[command(subcommand)]
        action: Option<PluginAction>,
    },
    /// Configure external memory provider
    Memory {
        #[command(subcommand)]
        action: Option<MemoryAction>,
    },
    /// Log out and clear stored credentials
    Logout {
        /// Provider to log out from (default: all)
        #[arg(long)]
        provider: Option<String>,
    },
    /// Restore a backup from a zip archive
    Import {
        /// Backup archive path (.zip)
        path: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    /// Manage MCP server connections
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },
    /// Interactive model selection and management
    Model {
        #[command(subcommand)]
        action: Option<ModelAction>,
    },
    /// OAuth login for supported providers
    Login {
        /// Provider name (google, anthropic, openai)
        provider: String,
        /// OAuth client ID
        #[arg(long)]
        client_id: Option<String>,
        /// Skip browser auto-open
        #[arg(long)]
        no_browser: bool,
        /// OAuth scopes
        #[arg(long)]
        scopes: Option<String>,
        /// Nous portal base URL
        #[arg(long)]
        portal_url: Option<String>,
        /// Nous inference base URL
        #[arg(long)]
        inference_url: Option<String>,
        /// Request timeout in seconds
        #[arg(long)]
        timeout: Option<f64>,
        /// Custom CA bundle path
        #[arg(long)]
        ca_bundle: Option<String>,
        /// Disable TLS verification
        #[arg(long)]
        insecure: bool,
    },
    /// Manage device pairings
    Pairing {
        #[command(subcommand)]
        action: PairingAction,
    },
    /// Self-update Hermes Agent
    Update {
        /// Use preview (pre-release) channel
        #[arg(long)]
        preview: bool,
        /// Force upgrade even when up to date
        #[arg(long)]
        force: bool,
        /// Gateway mode: use file-based IPC (internal use)
        #[arg(long)]
        gateway: bool,
    },
    /// Uninstall Hermes Agent
    Uninstall {
        /// Preserve data directory
        #[arg(long)]
        keep_data: bool,
        /// Preserve config
        #[arg(long)]
        keep_config: bool,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Interactive analytics dashboard
    Dashboard {
        /// Port to listen on
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Don't auto-open browser
        #[arg(long)]
        no_open: bool,
        /// Disable HTTPS redirect (testing)
        #[arg(long)]
        insecure: bool,
    },
    /// Configure WhatsApp Cloud API
    WhatsApp {
        /// Action: setup, connect, status
        action: String,
        /// Access token
        #[arg(long)]
        token: Option<String>,
        /// Phone Number ID
        #[arg(long)]
        phone_id: Option<String>,
    },
    /// Agent Client Protocol (IDE integration)
    Acp {
        /// Action: status, install, run
        action: Option<String>,
        /// Editor name (vscode, zed, jetbrains)
        #[arg(long)]
        editor: Option<String>,
    },
    /// Migrate from another agent system
    Claw {
        /// Action: migrate, cleanup
        action: String,
        /// Source system path or name (claude-code, chatgpt, or ~/.openclaw)
        #[arg(long)]
        source: Option<String>,
        /// Force migration
        #[arg(long)]
        force: bool,
        /// Preview only — stop after showing what would be migrated
        #[arg(long)]
        dry_run: bool,
        /// Migration preset: user-data (excludes secrets) or full
        #[arg(long, default_value = "full")]
        preset: String,
        /// Overwrite existing files (default: skip conflicts)
        #[arg(long)]
        overwrite: bool,
        /// Include allowlisted secrets
        #[arg(long)]
        migrate_secrets: bool,
        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,
        /// Path to copy workspace instructions into
        #[arg(long)]
        workspace_target: Option<String>,
        /// How to handle skill conflicts (choices: skip, overwrite, rename)
        #[arg(long, default_value = "skip")]
        skill_conflict: String,
    },
}

/// Subcommands for model management.
#[derive(Subcommand)]
enum ModelAction {
    /// Interactive model selection
    Browse,
    /// List available models
    #[command(alias = "ls")]
    List,
    /// Switch to a different model
    Switch {
        /// Model identifier (e.g., anthropic/claude-sonnet-4-6)
        model: String,
    },
    /// Show model details
    Info {
        /// Model identifier
        model: String,
    },
}

/// Subcommands for skin management.
#[derive(Subcommand)]
enum SkinAction {
    /// List available skins
    #[command(alias = "ls")]
    List,
    /// Apply a skin
    Apply {
        /// Skin name
        name: String,
    },
    /// Preview a skin without applying
    Preview {
        /// Skin name
        name: String,
    },
}

/// Subcommands for device pairing.
#[derive(Subcommand)]
enum PairingAction {
    /// Show pending + approved pairings
    #[command(alias = "ls")]
    List,
    /// Approve a pairing code
    Approve {
        /// Platform name (telegram, discord, slack, whatsapp)
        platform: String,
        /// Pairing code
        code: String,
    },
    /// Revoke user access
    Revoke {
        /// Platform name
        platform: String,
        /// Pairing code or user ID to revoke
        code: String,
    },
    /// Clear all pending codes
    ClearPending,
}

#[derive(Subcommand)]
enum WebhookAction {
    /// Create a webhook subscription
    #[command(alias = "add")]
    Subscribe {
        /// Route name
        name: String,
        /// Prompt template with {dot.notation} payload refs
        #[arg(long, default_value = "")]
        prompt: String,
        /// Comma-separated event types
        #[arg(long, default_value = "")]
        events: String,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
        /// Delivery target
        #[arg(long, default_value = "log")]
        deliver: String,
        /// Target chat ID for cross-platform delivery
        #[arg(long)]
        deliver_chat_id: Option<String>,
        /// Comma-separated skill names
        #[arg(long, default_value = "")]
        skills: String,
        /// HMAC secret for payload verification
        #[arg(long)]
        secret: Option<String>,
    },
    /// List webhook subscriptions
    #[command(alias = "ls")]
    List,
    /// Remove a subscription
    #[command(alias = "rm")]
    Remove {
        /// Subscription name
        name: String,
    },
    /// Send a test POST to a webhook route
    Test {
        /// Subscription name
        name: String,
        /// JSON payload to send
        #[arg(long, default_value = "")]
        payload: String,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// Install a plugin from Git
    Install {
        /// Git URL or owner/repo shorthand
        identifier: String,
        /// Remove existing and reinstall
        #[arg(short, long)]
        force: bool,
    },
    /// Update a plugin
    Update {
        /// Plugin name
        name: String,
    },
    /// Remove a plugin
    #[command(alias = "rm", alias = "uninstall")]
    Remove {
        /// Plugin name
        name: String,
    },
    /// List installed plugins
    #[command(alias = "ls")]
    List,
    /// Enable a disabled plugin
    Enable {
        /// Plugin name
        name: String,
    },
    /// Disable a plugin
    Disable {
        /// Plugin name
        name: String,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Interactive provider selection and configuration
    Setup,
    /// Show current memory provider config
    Status,
    /// Disable external provider (built-in only)
    Off,
}

#[derive(Subcommand)]
enum McpAction {
    /// List configured MCP servers
    #[command(alias = "ls")]
    List,
    /// Add an MCP server
    Add {
        /// Server name
        name: String,
        /// HTTP/SSE endpoint URL
        #[arg(long)]
        url: Option<String>,
        /// Command to run
        #[arg(long)]
        command: Option<String>,
        /// Command arguments
        #[arg(long, default_values_t = Vec::<String>::new())]
        args: Vec<String>,
        /// Auth method (oauth, header)
        #[arg(long)]
        auth: Option<String>,
        /// Known MCP preset name
        #[arg(long)]
        preset: Option<String>,
        /// Environment variables (KEY=VALUE)
        #[arg(long, default_values_t = Vec::<String>::new())]
        env: Vec<String>,
    },
    /// Remove an MCP server
    #[command(alias = "rm", alias = "delete")]
    Remove {
        /// Server name
        name: String,
    },
    /// Test connection to an MCP server
    Test {
        /// Server name
        name: String,
    },
    /// Interactive MCP configuration
    #[command(alias = "config")]
    Configure {
        /// Server name
        name: String,
    },
    /// Run as MCP stdio server
    Serve {
        /// Enable verbose logging on stderr
        #[arg(short = 'v', long)]
        verbose: bool,
    },
}

#[derive(Subcommand)]
enum ToolAction {
    /// List all available toolsets with enabled/disabled status
    #[command(alias = "ls")]
    List {
        /// Platform to show (default: cli)
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Show tool/toolset details
    Info { name: String },
    /// Disable one or more toolsets
    Disable {
        /// Tool names to disable
        names: Vec<String>,
        /// Platform to apply to
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Enable one or more toolsets
    Enable {
        /// Tool names to enable
        names: Vec<String>,
        /// Platform to apply to
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Disable all toolsets
    #[command(alias = "disable-all")]
    DisableAll {
        /// Platform to apply to
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Enable all toolsets
    #[command(alias = "enable-all")]
    EnableAll {
        /// Platform to apply to
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Batch disable multiple toolsets
    #[command(alias = "disable-batch")]
    DisableBatch {
        /// Tool names to disable
        names: Vec<String>,
        /// Platform to apply to
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Batch enable multiple toolsets
    #[command(alias = "enable-batch")]
    EnableBatch {
        /// Tool names to enable
        names: Vec<String>,
        /// Platform to apply to
        #[arg(long, default_value = "cli")]
        platform: String,
    },
    /// Show summary of enabled tools per platform
    Summary,
}

#[derive(Subcommand)]
enum SkillAction {
    /// List installed skills
    #[command(alias = "ls")]
    List {
        /// Filter by source: all, hub, builtin, local
        #[arg(long, default_value = "all")]
        source: String,
    },
    /// Search skill registries
    Search {
        /// Search query
        query: String,
        /// Filter by source
        #[arg(long, default_value = "all")]
        source: String,
        /// Max results
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Browse all available skills (paginated)
    Browse {
        /// Page number
        #[arg(long, default_value_t = 1)]
        page: usize,
        /// Results per page
        #[arg(long, default_value_t = 20)]
        size: usize,
        /// Filter by source
        #[arg(long, default_value = "all")]
        source: String,
    },
    /// Install a skill
    Install {
        /// Skill identifier
        identifier: String,
        /// Category folder to install into
        #[arg(long, default_value = "")]
        category: String,
        /// Force install despite existing
        #[arg(long)]
        force: bool,
        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Preview a skill without installing
    Inspect {
        /// Skill identifier
        identifier: String,
    },
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
    /// Uninstall a skill
    Uninstall {
        /// Skill name to remove
        name: String,
    },
    /// Check installed skills for updates
    Check {
        /// Specific skill to check (default: all)
        name: Option<String>,
    },
    /// Update installed hub skills
    Update {
        /// Specific skill to update (default: all)
        name: Option<String>,
    },
    /// Re-scan installed hub skills
    Audit {
        /// Specific skill to audit (default: all)
        name: Option<String>,
    },
    /// List discovered skill slash commands
    Commands,
    /// Publish a skill to a registry
    Publish {
        /// Skill name
        name: String,
        /// Registry URL
        #[arg(long)]
        registry: Option<String>,
        /// Target GitHub repo (owner/repo)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Export/import skill configurations
    Snapshot {
        #[command(subcommand)]
        snapshot_action: Option<SnapshotAction>,
    },
    /// Manage skill sources (taps)
    Tap {
        #[command(subcommand)]
        tap_action: Option<TapAction>,
    },
    /// Interactive skill configuration
    Config,
}

/// Subcommands for skill snapshots.
#[derive(Subcommand)]
enum SnapshotAction {
    /// Export installed skills to a file
    Export {
        /// Output file path
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Import and install skills from a file
    Import {
        /// Input file path
        path: String,
        /// Force import despite existing
        #[arg(long)]
        force: bool,
    },
}

/// Subcommands for skill taps.
#[derive(Subcommand)]
enum TapAction {
    /// List configured taps
    #[command(alias = "ls")]
    List,
    /// Add a GitHub repo as skill source
    Add {
        /// GitHub repo URL or owner/repo
        repo: String,
    },
    /// Remove a tap
    #[command(alias = "rm")]
    Remove {
        /// Tap name
        name: String,
    },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// List all profiles
    #[command(alias = "ls")]
    List,
    /// Create a new profile
    #[command(alias = "add")]
    Create {
        name: String,
        /// Copy config.yaml, .env, SOUL.md from active profile
        #[arg(long)]
        clone: bool,
        /// Full copy of active profile
        #[arg(long)]
        clone_all: bool,
        /// Source profile to clone from
        #[arg(long)]
        clone_from: Option<String>,
        /// Skip wrapper script creation
        #[arg(long)]
        no_alias: bool,
    },
    /// Switch to a profile
    Use { name: String },
    /// Delete a profile
    #[command(alias = "rm")]
    Delete {
        /// Profile name
        name: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
        /// Skip confirmation (alias)
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Show profile details
    Show {
        /// Profile name
        name: String,
    },
    /// Manage wrapper scripts
    Alias {
        /// Profile name
        name: String,
        /// Remove the wrapper script
        #[arg(long)]
        remove: bool,
        /// Custom alias name
        #[arg(long)]
        alias_name: Option<String>,
    },
    /// Rename a profile
    Rename {
        /// Current name
        old_name: String,
        /// New name
        #[arg(long)]
        new_name: String,
    },
    /// Export a profile to archive
    Export {
        /// Profile name
        name: String,
        /// Output file path
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Import a profile from archive
    Import {
        /// Archive file path
        path: String,
        /// Profile name (default: inferred from archive)
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// List recent sessions
    #[command(alias = "ls")]
    List {
        /// Maximum number of sessions to show
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// Filter by source (e.g., cli, telegram)
        #[arg(short, long)]
        source: Option<String>,
    },
    /// Delete a session
    #[command(alias = "rm")]
    Delete {
        /// Session ID or prefix
        session_id: String,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
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
    /// Rename a session's title
    Rename {
        /// Session ID
        session_id: String,
        /// New title
        #[arg(short, long)]
        title: String,
    },
    /// Prune old sessions
    Prune {
        /// Delete sessions older than this many days (default: 90)
        #[arg(long, default_value_t = 90)]
        older_than: i64,
        /// Filter by source
        #[arg(short, long)]
        source: Option<String>,
        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Interactive session browser
    Browse {
        /// Filter by source
        #[arg(short, long)]
        source: Option<String>,
        /// Maximum number of sessions to show
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
    },
    /// Export sessions to JSONL
    Export {
        /// Output file path (use - for stdout)
        path: String,
        /// Filter by source
        #[arg(short, long)]
        source: Option<String>,
        /// Export a specific session by ID
        #[arg(long)]
        session_id: Option<String>,
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
    /// Print config file path
    Path,
    /// Print .env file path
    EnvPath,
    /// Check for missing/outdated config
    Check,
    /// Update config with new options
    Migrate,
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
    Run {
        /// Enable verbose logging
        #[arg(short = 'v', long)]
        verbose: bool,
        /// Suppress non-error output
        #[arg(short = 'q', long)]
        quiet: bool,
        /// Replace running gateway instance
        #[arg(long)]
        replace: bool,
    },
    /// Start gateway as background service
    Start {
        /// Start on all platforms
        #[arg(long)]
        all: bool,
        /// Target Linux system-level gateway service (systemd)
        #[arg(long)]
        system: bool,
    },
    /// Stop gateway service
    Stop {
        /// Stop on all platforms
        #[arg(long)]
        all: bool,
        /// Target Linux system-level gateway service (systemd)
        #[arg(long)]
        system: bool,
    },
    /// Restart gateway service
    Restart {
        /// Restart system service (systemd/launchd)
        #[arg(long)]
        system: bool,
        /// Restart on all platforms
        #[arg(long)]
        all: bool,
    },
    /// Show gateway status
    Status {
        /// Run deep checks (slower)
        #[arg(long)]
        deep: bool,
        /// Target Linux system-level gateway service (systemd)
        #[arg(long)]
        system: bool,
    },
    /// Install gateway as systemd/launchd service
    Install {
        /// Force reinstall
        #[arg(long)]
        force: bool,
        /// Install as Linux system-level service (systemd)
        #[arg(long)]
        system: bool,
        /// User account the Linux system service should run as
        #[arg(long)]
        run_as_user: Option<String>,
    },
    /// Uninstall gateway service
    Uninstall {
        /// Target Linux system-level gateway service (systemd)
        #[arg(long)]
        system: bool,
    },
    /// Configure messaging platforms
    Setup,
}

#[derive(Subcommand)]
enum CronAction {
    /// List scheduled jobs
    #[command(alias = "ls")]
    List {
        /// Include disabled jobs
        #[arg(long)]
        all: bool,
    },
    /// Create a new scheduled job
    #[command(alias = "add")]
    Create {
        /// Job name
        name: String,
        /// Cron expression or interval (e.g., "0 9 * * *" or "1h")
        #[arg(short, long)]
        schedule: String,
        /// Command or URL to execute
        #[arg(short, long)]
        command: String,
        /// Task instruction / prompt for the agent
        #[arg(long)]
        prompt: Option<String>,
        /// Delivery platform (e.g., telegram, discord, webhook)
        #[arg(long, default_value = "local")]
        delivery: Option<String>,
        /// Start disabled
        #[arg(long)]
        paused: bool,
        /// Repeat count (0 = infinite)
        #[arg(long, default_value_t = 0)]
        repeat: usize,
        /// Skill name to invoke
        #[arg(long)]
        skill: Option<String>,
        /// Script content to execute
        #[arg(long)]
        script: Option<String>,
    },
    /// Delete a scheduled job
    #[command(alias = "rm", alias = "delete")]
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
    /// Edit a scheduled job
    Edit {
        /// Job ID
        job_id: String,
        /// New schedule
        #[arg(short, long)]
        schedule: Option<String>,
        /// New name
        #[arg(short, long)]
        name: Option<String>,
        /// New prompt
        #[arg(short, long)]
        prompt: Option<String>,
        /// New delivery target
        #[arg(long)]
        deliver: Option<String>,
        /// Repeat count
        #[arg(long)]
        repeat: Option<usize>,
        /// Script content
        #[arg(long)]
        script: Option<String>,
        /// Skill name to set
        #[arg(long)]
        skill: Option<String>,
        /// Add a skill to the job
        #[arg(long)]
        add_skill: Option<String>,
        /// Remove a skill from the job
        #[arg(long)]
        remove_skill: Option<String>,
        /// Clear all skills
        #[arg(long)]
        clear_skills: bool,
    },
    /// Trigger a job to run on next tick
    Run {
        /// Job ID
        job_id: String,
    },
    /// Show scheduler status
    Status,
    /// Run all due jobs once (debug)
    Tick,
}

#[derive(Subcommand)]
enum AuthAction {
    /// Add a pooled credential
    Add {
        /// Provider name (e.g., openai, anthropic)
        provider: String,
        /// Credential type
        #[arg(long, default_value = "api-key")]
        auth_type: String,
        /// API key value
        #[arg(long, alias = "key")]
        api_key: Option<String>,
        /// Label for this credential
        #[arg(long)]
        label: Option<String>,
        /// OAuth client id
        #[arg(long)]
        client_id: Option<String>,
        /// Skip browser auto-open for OAuth
        #[arg(long)]
        no_browser: bool,
        /// Nous portal base URL
        #[arg(long)]
        portal_url: Option<String>,
        /// Nous inference base URL
        #[arg(long)]
        inference_url: Option<String>,
        /// OAuth scope override
        #[arg(long)]
        scope: Option<String>,
        /// OAuth/network timeout in seconds
        #[arg(long)]
        timeout: Option<f64>,
        /// Disable TLS verification for OAuth login
        #[arg(long)]
        insecure: bool,
        /// Custom CA bundle for OAuth login
        #[arg(long)]
        ca_bundle: Option<String>,
    },
    /// List pooled credentials
    List {
        /// Filter by provider
        provider: Option<String>,
    },
    /// Remove a pooled credential
    Remove {
        /// Provider name
        provider: String,
        /// Credential index, entry id, or label
        target: String,
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
    } else if let Some(profile) = cli.profile {
        // Resolve profile name to path and set as HERMES_HOME
        let profile_path = hermes_core::hermes_home::resolve_profile_path(&profile);
        hermes_core::hermes_home::set_hermes_home(&profile_path)
            .ok();
    }

    let app = HermesApp::new()?;

    match cli.command {
        Some(Commands::Chat { model, query, image, toolsets, skills, provider, resume, continue_last, worktree, checkpoints, max_turns, yolo, pass_session_id, source, quiet, verbose, skip_context_files, skip_memory, voice }) => {
            app.run_chat(model, query, image, toolsets, skills, provider, resume, continue_last, worktree, checkpoints, max_turns, yolo, pass_session_id, source, quiet, verbose, skip_context_files, skip_memory, voice)?;
        }
        Some(Commands::Setup { section, non_interactive, reset }) => {
            if reset {
                hermes_cli::setup_cmd::cmd_setup_reset()
                    .map_err(|e| anyhow::anyhow!(e))?;
            } else if let Some(sec) = section {
                hermes_cli::setup_cmd::cmd_setup_section(&sec, non_interactive)
                    .map_err(|e| anyhow::anyhow!(e))?;
            } else {
                hermes_cli::setup_cmd::cmd_setup()
                    .map_err(|e| anyhow::anyhow!(e))?;
            }
        }
        Some(Commands::Backup { output, include_sessions, quick, label }) => {
            hermes_cli::backup_cmd::cmd_backup_extended(output.as_deref(), include_sessions, quick, label.as_deref())?;
        }
        Some(Commands::Restore { path, force }) => {
            hermes_cli::backup_cmd::cmd_restore(&path, force)?;
        }
        Some(Commands::BackupList) => {
            hermes_cli::backup_cmd::cmd_backup_list()?;
        }
        Some(Commands::Debug) => {
            hermes_cli::debug_cmd::cmd_debug()?;
        }
        Some(Commands::DebugShare { lines, expire_days, local_only }) => {
            hermes_cli::debug_share_cmd::cmd_debug_share(lines, expire_days, local_only)?;
        }
        Some(Commands::Dump { session_id, show_keys }) => {
            match session_id {
                Some(sid) => {
                    hermes_cli::debug_cmd::cmd_dump_session(&sid, show_keys)?;
                }
                None => {
                    hermes_cli::dump_cmd::cmd_dump(show_keys)?;
                }
            }
        }
        Some(Commands::Tools { action }) => {
            match action {
                Some(ToolAction::List { platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_list(&platform)?;
                }
                Some(ToolAction::Info { name }) => {
                    hermes_cli::tools_cmd::cmd_tools_info(&name)?;
                }
                Some(ToolAction::Disable { names, platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_disable(&names, &platform)?;
                }
                Some(ToolAction::Enable { names, platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_enable(&names, &platform)?;
                }
                Some(ToolAction::DisableAll { platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_disable_all(&platform)?;
                }
                Some(ToolAction::EnableAll { platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_enable_all(&platform)?;
                }
                Some(ToolAction::DisableBatch { names, platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_disable_batch(&names, &platform)?;
                }
                Some(ToolAction::EnableBatch { names, platform }) => {
                    hermes_cli::tools_cmd::cmd_tools_enable_batch(&names, &platform)?;
                }
                Some(ToolAction::Summary) => {
                    hermes_cli::config_cmd::cmd_tools_summary()?;
                }
                None => {
                    hermes_cli::tools_cmd::cmd_tools_list("cli")?;
                }
            }
        }
        Some(Commands::Skills { action }) => {
            match action {
                Some(SkillAction::List { source }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("list", None, None, &source, 20, 1, "", false)?;
                }
                Some(SkillAction::Search { query, source, limit }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("search", None, Some(&query), &source, limit, 1, "", false)?;
                }
                Some(SkillAction::Browse { page, size, source }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("browse", None, None, &source, size, page, "", false)?;
                }
                Some(SkillAction::Install { identifier, category, force, yes }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills_install(&identifier, &category, force, yes)?;
                }
                Some(SkillAction::Inspect { identifier }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("inspect", Some(&identifier), None, "all", 10, 1, "", false)?;
                }
                Some(SkillAction::Info { name }) => app.show_skill_info(&name)?,
                Some(SkillAction::Enable { name, platform }) => app.enable_skill(&name, platform.as_deref())?,
                Some(SkillAction::Disable { name, platform }) => app.disable_skill(&name, platform.as_deref())?,
                Some(SkillAction::Uninstall { name }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("uninstall", Some(&name), None, "all", 10, 1, "", false)?;
                }
                Some(SkillAction::Check { name }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("check", name.as_deref(), None, "all", 10, 1, "", false)?;
                }
                Some(SkillAction::Update { name }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("update", name.as_deref(), None, "all", 10, 1, "", false)?;
                }
                Some(SkillAction::Audit { name }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("audit", name.as_deref(), None, "all", 10, 1, "", false)?;
                }
                Some(SkillAction::Commands) => app.list_skill_commands()?,
                Some(SkillAction::Publish { name, registry, repo }) => {
                    hermes_cli::skills_hub_cmd::cmd_skills_publish(&name, registry.as_deref(), repo.as_deref())?;
                }
                Some(SkillAction::Snapshot { snapshot_action }) => {
                    match snapshot_action {
                        Some(SnapshotAction::Export { output }) => {
                            hermes_cli::skills_hub_cmd::cmd_skills("snapshot-export", None, None, "all", 10, 1, output.as_deref().unwrap_or(""), false)?;
                        }
                        Some(SnapshotAction::Import { path, force }) => {
                            hermes_cli::skills_hub_cmd::cmd_skills("snapshot-import", Some(&path), None, "all", 10, 1, "", force)?;
                        }
                        None => {
                            hermes_cli::skills_hub_cmd::cmd_skills("snapshot-export", None, None, "all", 10, 1, "", false)?;
                        }
                    }
                }
                Some(SkillAction::Tap { tap_action }) => {
                    match tap_action {
                        Some(TapAction::List) => {
                            hermes_cli::skills_hub_cmd::cmd_skills("tap-list", None, None, "all", 10, 1, "", false)?;
                        }
                        Some(TapAction::Add { repo }) => {
                            hermes_cli::skills_hub_cmd::cmd_skills("tap-add", Some(&repo), None, "all", 10, 1, "", false)?;
                        }
                        Some(TapAction::Remove { name }) => {
                            hermes_cli::skills_hub_cmd::cmd_skills("tap-remove", Some(&name), None, "all", 10, 1, "", false)?;
                        }
                        None => {
                            hermes_cli::skills_hub_cmd::cmd_skills("tap-list", None, None, "all", 10, 1, "", false)?;
                        }
                    }
                }
                Some(SkillAction::Config) => {
                    hermes_cli::skills_hub_cmd::cmd_skills("config", None, None, "all", 10, 1, "", false)?;
                }
                None => app.list_skills()?,
            }
        }
        Some(Commands::Gateway { action }) => {
            match action {
                Some(GatewayAction::Run { verbose, quiet, replace }) => {
                    app.run_gateway_with_opts(verbose, quiet, replace)?;
                }
                None => {
                    app.run_gateway_with_opts(false, false, false)?;
                }
                Some(GatewayAction::Start { all, system }) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_start(all, system)
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
                Some(GatewayAction::Stop { all, system }) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_stop(all, system)
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
                Some(GatewayAction::Restart { system, all }) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_restart(system, all)
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
                Some(GatewayAction::Status { deep, system }) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_status(deep, system)
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
                Some(GatewayAction::Install { force, system, run_as_user }) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_install(force, system, run_as_user.as_deref())
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
                Some(GatewayAction::Uninstall { system }) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_uninstall(system)
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
                Some(GatewayAction::Setup) => {
                    hermes_cli::gateway_mgmt::cmd_gateway_setup()
                        .map_err(|e| anyhow::anyhow!(e))?;
                }
            }
        }
        Some(Commands::Doctor { fix }) => {
            if fix {
                app.run_doctor_fix()?;
            } else {
                app.run_doctor()?;
            }
        }
        Some(Commands::Models) => {
            app.list_models()?;
        }
        Some(Commands::Profiles { action }) => {
            match action {
                Some(ProfileAction::List) => {
                    hermes_cli::profiles_cmd::cmd_profile_list()?;
                }
                Some(ProfileAction::Create { name, clone, clone_all, clone_from, no_alias }) => {
                    hermes_cli::profiles_cmd::cmd_profile_create(
                        &name, clone, clone_all, clone_from.as_deref(), no_alias,
                    )?;
                }
                Some(ProfileAction::Use { name }) => {
                    hermes_cli::profiles_cmd::cmd_profile_use(&name)?;
                }
                Some(ProfileAction::Delete { name, force, yes }) => {
                    hermes_cli::profiles_cmd::cmd_profile_delete(&name, force || yes)?;
                }
                Some(ProfileAction::Show { name }) => {
                    hermes_cli::profiles_cmd::cmd_profile_show(&name)?;
                }
                Some(ProfileAction::Alias { name, remove, alias_name }) => {
                    let target = alias_name.as_deref();
                    hermes_cli::profiles_cmd::cmd_profile_alias(&name, target, remove)?;
                }
                Some(ProfileAction::Rename { old_name, new_name }) => {
                    hermes_cli::profiles_cmd::cmd_profile_rename(&old_name, &new_name)?;
                }
                Some(ProfileAction::Export { name, output }) => {
                    hermes_cli::profiles_cmd::cmd_profile_export(&name, output.as_deref())?;
                }
                Some(ProfileAction::Import { path, name: profile_name }) => {
                    hermes_cli::profiles_cmd::cmd_profile_import(&path, profile_name.as_deref())?;
                }
                None => {
                    hermes_cli::profiles_cmd::cmd_profile_list()?;
                }
            }
        }
        Some(Commands::Sessions { action }) => {
            let db = hermes_state::SessionDB::open_default()?;
            match action {
                Some(SessionAction::List { limit, source }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_list(&db, limit, source.as_deref(), false)?;
                }
                Some(SessionAction::Delete { session_id, yes }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_delete(&db, &session_id, yes)?;
                }
                Some(SessionAction::Search { query, limit }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_search(&db, &query, limit)?;
                }
                Some(SessionAction::Stats { source }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_stats(&db, source.as_deref())?;
                }
                Some(SessionAction::Rename { session_id, title }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_rename(&db, &session_id, &title)?;
                }
                Some(SessionAction::Prune { older_than, source, yes }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_prune(&db, older_than, source.as_deref(), yes)?;
                }
                Some(SessionAction::Browse { source, limit }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_list(&db, limit, source.as_deref(), true)?;
                }
                Some(SessionAction::Export { path, source, session_id }) => {
                    hermes_cli::sessions_cmd::cmd_sessions_export(&db, &path, source.as_deref(), session_id.as_deref())?;
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
                Some(ConfigAction::Path) => {
                    hermes_cli::config_cmd::cmd_config_path()?;
                }
                Some(ConfigAction::EnvPath) => {
                    hermes_cli::config_cmd::cmd_config_env_path()?;
                }
                Some(ConfigAction::Check) => {
                    hermes_cli::config_cmd::cmd_config_check()?;
                }
                Some(ConfigAction::Migrate) => {
                    hermes_cli::config_cmd::cmd_config_migrate()?;
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
                Some(CronAction::List { all }) => {
                    hermes_cli::cron_cmd::cmd_cron_list(all)?;
                }
                Some(CronAction::Create { name, schedule, command, prompt, delivery, paused, repeat, skill, script }) => {
                    hermes_cli::cron_cmd::cmd_cron_create(&name, &schedule, &command, prompt.as_deref(), &delivery.unwrap_or_else(|| "local".to_string()), !paused, repeat, skill.as_deref(), script.as_deref())?;
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
                Some(CronAction::Edit { job_id, schedule, name, prompt, deliver, repeat, script, skill, add_skill, remove_skill, clear_skills }) => {
                    hermes_cli::cron_cmd::cmd_cron_edit(&job_id, schedule.as_deref(), name.as_deref(), prompt.as_deref(), deliver.as_deref(), repeat, script.as_deref(), skill.as_deref(), add_skill.as_deref(), remove_skill.as_deref(), clear_skills)?;
                }
                Some(CronAction::Run { job_id }) => {
                    hermes_cli::cron_cmd::cmd_cron_run(&job_id)?;
                }
                Some(CronAction::Status) => {
                    hermes_cli::cron_cmd::cmd_cron_status()?;
                }
                Some(CronAction::Tick) => {
                    hermes_cli::cron_cmd::cmd_cron_tick()?;
                }
                None => {
                    hermes_cli::cron_cmd::cmd_cron_list(false)?;
                }
            }
        }
        Some(Commands::Auth { action }) => {
            match action {
                Some(AuthAction::Add { provider, auth_type, api_key, label, client_id, no_browser, portal_url, inference_url, scope, timeout, insecure, ca_bundle }) => {
                    hermes_cli::auth_cmd::cmd_auth_add(
                        &provider,
                        &auth_type,
                        api_key.as_deref(),
                        label.as_deref(),
                        client_id.as_deref(),
                        no_browser,
                        portal_url.as_deref(),
                        inference_url.as_deref(),
                        scope.as_deref(),
                        timeout,
                        insecure,
                        ca_bundle.as_deref(),
                    )?;
                }
                Some(AuthAction::List { provider }) => {
                    hermes_cli::auth_cmd::cmd_auth_list(provider.as_deref())?;
                }
                Some(AuthAction::Remove { provider, target }) => {
                    hermes_cli::auth_cmd::cmd_auth_remove(&provider, &target)?;
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
        Some(Commands::Status { all, deep }) => {
            hermes_cli::status_cmd::cmd_status(all, deep)?;
        }
        Some(Commands::Insights { days, source }) => {
            hermes_cli::insights_cmd::cmd_insights(days, source.as_deref())?;
        }
        Some(Commands::Completion { shell }) => {
            hermes_cli::completion_cmd::cmd_completion(&shell)?;
        }
        Some(Commands::Version) => {
            hermes_cli::version_cmd::cmd_version();
        }
        Some(Commands::Logs { log_name, lines, follow, level, session, component, since }) => {
            hermes_cli::logs_cmd::cmd_logs(
                log_name.as_deref().unwrap_or("agent"),
                lines,
                follow,
                level.as_deref(),
                session.as_deref(),
                component.as_deref(),
                since.as_deref(),
            )?;
        }
        Some(Commands::Webhook { action }) => {
            match action {
                WebhookAction::Subscribe { name, prompt, events, description, deliver, deliver_chat_id, skills, secret } => {
                    hermes_cli::webhook_cmd::cmd_webhook_subscribe(
                        &name, &prompt, &events, &description, &deliver, deliver_chat_id, &skills, secret.as_deref(),
                    )?;
                }
                WebhookAction::List => {
                    hermes_cli::webhook_cmd::cmd_webhook_list()?;
                }
                WebhookAction::Remove { name } => {
                    hermes_cli::webhook_cmd::cmd_webhook_remove(&name)?;
                }
                WebhookAction::Test { name, payload } => {
                    hermes_cli::webhook_cmd::cmd_webhook_test(&name, &payload)?;
                }
            }
        }
        Some(Commands::Plugins { action }) => {
            match action {
                Some(PluginAction::Install { identifier, force }) => {
                    hermes_cli::plugins_cmd::cmd_plugins_install(&identifier, force)?;
                }
                Some(PluginAction::Update { name }) => {
                    hermes_cli::plugins_cmd::cmd_plugins_update(&name)?;
                }
                Some(PluginAction::Remove { name }) => {
                    hermes_cli::plugins_cmd::cmd_plugins_remove(&name)?;
                }
                Some(PluginAction::List) | None => {
                    hermes_cli::plugins_cmd::cmd_plugins_list()?;
                }
                Some(PluginAction::Enable { name }) => {
                    hermes_cli::plugins_cmd::cmd_plugins_enable(&name)?;
                }
                Some(PluginAction::Disable { name }) => {
                    hermes_cli::plugins_cmd::cmd_plugins_disable(&name)?;
                }
            }
        }
        Some(Commands::Memory { action }) => {
            match action {
                Some(MemoryAction::Setup) => {
                    hermes_cli::memory_cmd::cmd_memory_setup()?;
                }
                Some(MemoryAction::Status) => {
                    hermes_cli::memory_cmd::cmd_memory_status()?;
                }
                Some(MemoryAction::Off) => {
                    hermes_cli::memory_cmd::cmd_memory_off()?;
                }
                None => {
                    hermes_cli::memory_cmd::cmd_memory_status()?;
                }
            }
        }
        Some(Commands::Logout { provider }) => {
            hermes_cli::auth_cmd::cmd_logout(provider.as_deref())?;
        }
        Some(Commands::Import { path, force }) => {
            hermes_cli::backup_cmd::cmd_import(&path, force)?;
        }
        Some(Commands::Mcp { action }) => {
            match action {
                Some(McpAction::List) => {
                    hermes_cli::mcp_cmd::cmd_mcp_list()?;
                }
                Some(McpAction::Add { name, url, command, args, auth, preset, env }) => {
                    hermes_cli::mcp_cmd::cmd_mcp_add(&name, url.as_deref(), command.as_deref(), &args, auth.as_deref(), preset.as_deref(), &env)?;
                }
                Some(McpAction::Remove { name }) => {
                    hermes_cli::mcp_cmd::cmd_mcp("remove", Some(&name), "", &[])?;
                }
                Some(McpAction::Test { name }) => {
                    hermes_cli::mcp_cmd::cmd_mcp("test", Some(&name), "", &[])?;
                }
                Some(McpAction::Configure { name }) => {
                    hermes_cli::mcp_cmd::cmd_mcp_configure(&name)?;
                }
                Some(McpAction::Serve { verbose }) => {
                    hermes_cli::mcp_cmd::cmd_mcp_serve(verbose)?;
                }
                None => {
                    hermes_cli::mcp_cmd::cmd_mcp_list()?;
                }
            }
        }
        None => {
            // Default: interactive chat
            app.run_chat(None, None, None, None, None, None, None, None, false, false, None, false, false, None, false, false, false, false, false)?;
        }
        Some(Commands::Model { action }) => {
            match action {
                Some(ModelAction::Browse) | Some(ModelAction::List) | None => {
                    hermes_cli::model_cmd::cmd_model()?;
                }
                Some(ModelAction::Switch { model }) => {
                    hermes_cli::model_cmd::cmd_model_switch(&model)?;
                }
                Some(ModelAction::Info { model }) => {
                    hermes_cli::model_cmd::cmd_model_info(&model)?;
                }
            }
        }
        Some(Commands::Skin { action }) => {
            match action {
                Some(SkinAction::List) | None => {
                    hermes_cli::skin_engine::cmd_skin_list()?;
                }
                Some(SkinAction::Apply { name }) => {
                    hermes_cli::skin_engine::cmd_skin_apply(&name)?;
                }
                Some(SkinAction::Preview { name }) => {
                    hermes_cli::skin_engine::cmd_skin_preview(&name)?;
                }
            }
        }
        Some(Commands::Login { provider, client_id, no_browser, scopes, portal_url, inference_url, timeout, ca_bundle, insecure }) => {
            hermes_cli::login_cmd::cmd_login(&provider, client_id.as_deref(), no_browser, scopes.as_deref(), portal_url.as_deref(), inference_url.as_deref(), timeout, ca_bundle.as_deref(), insecure)?;
        }
        Some(Commands::Pairing { action }) => {
            match action {
                PairingAction::List => {
                    hermes_cli::pairing_cmd::cmd_pairing_list()?;
                }
                PairingAction::Approve { platform, code } => {
                    hermes_cli::pairing_cmd::cmd_pairing_approve(&platform, &code)?;
                }
                PairingAction::Revoke { platform, code } => {
                    hermes_cli::pairing_cmd::cmd_pairing_revoke(&platform, &code)?;
                }
                PairingAction::ClearPending => {
                    hermes_cli::pairing_cmd::cmd_pairing_clear_pending()?;
                }
            }
        }
        Some(Commands::Update { preview, force, gateway }) => {
            hermes_cli::update_cmd::cmd_update(preview, force, gateway)?;
        }
        Some(Commands::Uninstall { keep_data, keep_config, yes }) => {
            hermes_cli::uninstall_cmd::cmd_uninstall(keep_data, keep_config, yes)?;
        }
        Some(Commands::Dashboard { port, host, no_open, insecure }) => {
            hermes_cli::dashboard_cmd::cmd_dashboard_with_opts(&host, port, no_open, insecure)?;
        }
        Some(Commands::WhatsApp { action, token, phone_id }) => {
            hermes_cli::whatsapp_cmd::cmd_whatsapp(&action, token.as_deref(), phone_id.as_deref())?;
        }
        Some(Commands::Acp { action, editor }) => {
            hermes_cli::acp_cmd::cmd_acp(action.as_deref().unwrap_or("status"), editor.as_deref())?;
        }
        Some(Commands::Claw { action, source, force, dry_run, preset, overwrite, migrate_secrets, yes, workspace_target, skill_conflict }) => {
            hermes_cli::claw_cmd::cmd_claw(&action, source.as_deref(), force, dry_run, &preset, overwrite, migrate_secrets, yes, workspace_target.as_deref(), &skill_conflict)?;
        }
    }

    Ok(())
}
