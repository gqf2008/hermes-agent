<!-- Hermes Agent - Development Guide for AI Coding Assistants -->

# Hermes Agent - Development Guide

Instructions for AI coding assistants and developers working on the hermes-agent codebase.

## Project Overview

Hermes Agent is a self-improving AI agent built by Nous Research. It is a Python-first project (Python 3.11+) with a Rust rewrite in progress, a React/Vite web dashboard, and a Docusaurus documentation site. The agent features an interactive CLI, a messaging gateway (Telegram, Discord, Slack, WhatsApp, Signal, Matrix, etc.), a built-in skills system, MCP integration, cron scheduling, and 40+ tools.

Key capabilities:
- **AI conversation loop** with tool calling via `run_agent.py` (`AIAgent` class)
- **Interactive CLI** via `hermes_cli/main.py` with prompt_toolkit + Rich
- **Messaging gateway** in `gateway/run.py` for multi-platform bots
- **Tool registry** in `tools/registry.py` — auto-discovering, self-registering tools
- **Skills system** — procedural memory stored in `skills/`, with optional skills in `optional-skills/`
- **Memory & context compression** — SQLite-backed sessions with FTS5 search, Honcho integration
- **Batch & RL training** — `batch_runner.py`, `environments/`, and `hermes-rs/` for research workloads
- **ACP server** — editor integration (VS Code / Zed / JetBrains) via `acp_adapter/`

### Technology Stack

| Layer | Technology |
|-------|------------|
| Core runtime | Python 3.11+, OpenAI/Anthropic SDKs, Pydantic, Jinja2 |
| CLI / TUI | prompt_toolkit, Rich, curses (stdlib) |
| Web frontend | React 19, Vite, Tailwind CSS 4 (`web/`) |
| Documentation | Docusaurus 3, TypeScript (`website/`) |
| Rust rewrite | Cargo workspace with 13 crates (`hermes-rs/`) |
| Packaging | setuptools (pyproject.toml), uv, Nix (flake.nix), Docker |
| Database | SQLite (FTS5), optional asyncpg for Matrix encryption |
| Testing | pytest, pytest-asyncio, pytest-xdist |
| CI/CD | GitHub Actions (tests, Docker, Nix, docs, supply-chain audit) |

### Entry Points

| Command | Module | Purpose |
|---------|--------|---------|
| `hermes` | `hermes_cli.main:main` | Interactive CLI and subcommands |
| `hermes-agent` | `run_agent:main` | Standalone agent runner (Fire CLI) |
| `hermes-acp` | `acp_adapter.entry:main` | ACP server for editor integration |

Python top-level modules (registered in `pyproject.toml` `[tool.setuptools.py-modules]`):
`run_agent`, `model_tools`, `toolsets`, `batch_runner`, `trajectory_compressor`, `toolset_distributions`, `cli`, `hermes_constants`, `hermes_state`, `hermes_time`, `hermes_logging`, `rl_cli`, `utils`

---

## Development Environment

### Prerequisites

- **Git** with `--recurse-submodules` support
- **Python 3.11+**
- **uv** (fast Python package manager)
- **Node.js 18+** (optional, needed for browser tools and WhatsApp bridge)
- **Rust 1.84+** (optional, for `hermes-rs/`)

### Setup

```bash
# Clone
git clone --recurse-submodules https://github.com/NousResearch/hermes-agent.git
cd hermes-agent

# Python venv + install
uv venv venv --python 3.11
export VIRTUAL_ENV="$(pwd)/venv"
uv pip install -e ".[all,dev]"

# Optional: browser tools
npm install

# Optional: RL training submodule
git submodule update --init tinker-atropos && uv pip install -e "./tinker-atropos"

# Optional: Rust rewrite
cd hermes-rs && cargo build
```

### Activate before running Python

```bash
source venv/bin/activate  # ALWAYS activate before running Python
```

---

## Build and Test Commands

### Python

```bash
# Full test suite (~3000 tests, ~3 min)
python -m pytest tests/ -q

# Specific areas
python -m pytest tests/test_model_tools.py -q        # Toolset resolution
python -m pytest tests/test_cli_init.py -q           # CLI config loading
python -m pytest tests/gateway/ -q                   # Gateway tests
python -m pytest tests/tools/ -q                     # Tool-level tests
python -m pytest tests/hermes_cli/ -q                # CLI tests
python -m pytest tests/agent/ -q                     # Agent internals

# E2E tests
python -m pytest tests/e2e/ -v --tb=short

# With uv (as CI does)
uv venv .venv --python 3.11
source .venv/bin/activate
uv pip install -e ".[all,dev]"
python -m pytest tests/ -q --ignore=tests/integration --ignore=tests/e2e --tb=short -n auto
```

### Rust (`hermes-rs/`)

```bash
cd hermes-rs
cargo build
cargo test
cargo build --release
```

### Web (`web/`)

```bash
cd web
npm install
npm run dev      # Vite dev server
npm run build    # Production build
npm run lint     # ESLint
```

### Documentation (`website/`)

```bash
cd website
npm install
npm run build    # Docusaurus build
npm run lint:diagrams   # ASCII art linting
```

### Docker

```bash
# Build (amd64 smoke test)
docker build -t hermes-agent:test .

# Run
docker run --rm -v /tmp/hermes-data:/opt/data hermes-agent:test --help
```

### Nix

```bash
nix flake check --print-build-logs
nix build --print-build-logs
nix develop       # Enter dev shell
```

---

## Code Organization

```
hermes-agent/
├── run_agent.py          # AIAgent class — core conversation loop
├── model_tools.py        # Tool orchestration, discover_builtin_tools(), handle_function_call()
├── toolsets.py           # Toolset definitions, _HERMES_CORE_TOOLS list
├── cli.py                # HermesCLI class — interactive CLI orchestrator
├── hermes_state.py       # SessionDB — SQLite session store (FTS5 search)
├── batch_runner.py       # Parallel batch processing with checkpointing
├── trajectory_compressor.py # Trajectory compression for RL training
├── rl_cli.py             # RL training CLI
├── mcp_serve.py          # MCP server entry point
├── hermes_constants.py   # HERMES_HOME resolution, path constants
├── hermes_logging.py     # Structured logging setup
├── hermes_time.py        # Timezone utilities
├── agent/                # Agent internals
│   ├── prompt_builder.py     # System prompt assembly
│   ├── context_compressor.py # Auto context compression
│   ├── prompt_caching.py     # Anthropic prompt caching
│   ├── auxiliary_client.py   # Auxiliary LLM client (vision, summarization)
│   ├── model_metadata.py     # Model context lengths, token estimation
│   ├── models_dev.py         # models.dev registry integration
│   ├── display.py            # KawaiiSpinner, tool preview formatting
│   ├── skill_commands.py     # Skill slash commands (shared CLI/gateway)
│   ├── trajectory.py         # Trajectory saving helpers
│   ├── memory_manager.py     # Memory context assembly
│   ├── credential_pool.py    # API key rotation and fallback
│   ├── context_engine.py     # Context file processing
│   ├── error_classifier.py   # API error classification and failover
│   └── ...
├── hermes_cli/           # CLI subcommands and setup
│   ├── main.py           # Entry point — all `hermes` subcommands
│   ├── config.py         # DEFAULT_CONFIG, OPTIONAL_ENV_VARS, migration
│   ├── commands.py       # Slash command definitions + SlashCommandCompleter
│   ├── callbacks.py      # Terminal callbacks (clarify, sudo, approval)
│   ├── setup.py          # Interactive setup wizard
│   ├── skin_engine.py    # Skin/theme engine
│   ├── skills_config.py  # `hermes skills` — enable/disable skills per platform
│   ├── tools_config.py   # `hermes tools` — enable/disable tools per platform
│   ├── skills_hub.py     # `/skills` slash command (search, browse, install)
│   ├── models.py         # Model catalog, provider model lists
│   ├── model_switch.py   # Shared /model switch pipeline (CLI + gateway)
│   ├── auth.py           # Provider credential resolution
│   ├── gateway.py        # Gateway service management
│   ├── web_server.py     # Built-in FastAPI web server
│   └── ...
├── tools/                # Tool implementations (one file per tool)
│   ├── registry.py       # Central tool registry (schemas, handlers, dispatch)
│   ├── approval.py       # Dangerous command detection
│   ├── terminal_tool.py  # Terminal orchestration (6 backends)
│   ├── process_registry.py # Background process management
│   ├── file_tools.py     # File read/write/search/patch
│   ├── web_tools.py      # Web search/extract (Parallel + Firecrawl)
│   ├── browser_tool.py   # Browserbase browser automation
│   ├── code_execution_tool.py # execute_code sandbox
│   ├── delegate_tool.py  # Subagent delegation
│   ├── mcp_tool.py       # MCP client (~1050 lines)
│   ├── skills_tool.py    # Skills execution
│   ├── skills_guard.py   # Skills security audit
│   ├── skills_hub.py     # Skills Hub network client
│   ├── skills_sync.py    # Bundled skills sync
│   └── environments/     # Terminal backends (local, docker, ssh, modal, daytona, singularity)
├── gateway/              # Messaging platform gateway
│   ├── run.py            # Main loop, slash commands, message dispatch
│   ├── session.py        # SessionStore — conversation persistence
│   ├── config.py         # Gateway configuration
│   ├── delivery.py       # Cross-platform message delivery
│   ├── stream_consumer.py # Streaming response consumer
│   └── platforms/        # Adapters: telegram, discord, slack, whatsapp, homeassistant, signal, qqbot, matrix, mattermost, dingtalk, feishu
├── acp_adapter/          # ACP server (VS Code / Zed / JetBrains integration)
├── cron/                 # Scheduler (jobs.py, scheduler.py)
├── environments/         # RL training environments (Atropos)
├── skills/               # Bundled skills (broadly useful, ship with install)
├── optional-skills/      # Official but not universally needed skills
├── plugins/              # Plugin system (context_engine, memory)
├── web/                  # React/Vite dashboard
├── website/              # Docusaurus documentation site
├── hermes-rs/            # Rust rewrite (Cargo workspace, 13 crates)
├── tests/                # Pytest suite (~3000 tests)
├── nix/                  # Nix flake modules
├── docker/               # Docker entrypoint and SOUL.md
└── scripts/              # Release, install, skills index builder
```

### File Dependency Chain

```
tools/registry.py  (no deps — imported by all tool files)
       ↑
tools/*.py  (each calls registry.register() at import time)
       ↑
model_tools.py  (imports tools/registry + triggers tool discovery)
       ↑
run_agent.py, cli.py, batch_runner.py, environments/
```

---

## AIAgent Class (run_agent.py)

```python
class AIAgent:
    def __init__(self,
        model: str = "anthropic/claude-opus-4.6",
        max_iterations: int = 90,
        enabled_toolsets: list = None,
        disabled_toolsets: list = None,
        quiet_mode: bool = False,
        save_trajectories: bool = False,
        platform: str = None,           # "cli", "telegram", etc.
        session_id: str = None,
        skip_context_files: bool = False,
        skip_memory: bool = False,
        # ... plus provider, api_mode, callbacks, routing params
    ): ...

    def chat(self, message: str) -> str:
        """Simple interface — returns final response string."""

    def run_conversation(self, user_message: str, system_message: str = None,
                         conversation_history: list = None, task_id: str = None) -> dict:
        """Full interface — returns dict with final_response + messages."""
```

### Agent Loop

The core loop is inside `run_conversation()` — entirely synchronous:

```python
while api_call_count < self.max_iterations and self.iteration_budget.remaining > 0:
    response = client.chat.completions.create(model=model, messages=messages, tools=tool_schemas)
    if response.tool_calls:
        for tool_call in response.tool_calls:
            result = handle_function_call(tool_call.name, tool_call.args, task_id)
            messages.append(tool_result_message(result))
        api_call_count += 1
    else:
        return response.content
```

Messages follow OpenAI format: `{"role": "system/user/assistant/tool", ...}`. Reasoning content is stored in `assistant_msg["reasoning"]`.

---

## CLI Architecture (cli.py)

- **Rich** for banner/panels, **prompt_toolkit** for input with autocomplete
- **KawaiiSpinner** (`agent/display.py`) — animated faces during API calls, `┊` activity feed for tool results
- `load_cli_config()` in cli.py merges hardcoded defaults + user config YAML
- **Skin engine** (`hermes_cli/skin_engine.py`) — data-driven CLI theming; initialized from `display.skin` config key at startup
- `process_command()` is a method on `HermesCLI` — dispatches on canonical command name resolved via `resolve_command()` from the central registry
- Skill slash commands: `agent/skill_commands.py` scans `~/.hermes/skills/`, injects as **user message** (not system prompt) to preserve prompt caching

### Slash Command Registry (`hermes_cli/commands.py`)

All slash commands are defined in a central `COMMAND_REGISTRY` list of `CommandDef` objects. Every downstream consumer derives from this registry automatically:

- **CLI** — `process_command()` resolves aliases via `resolve_command()`, dispatches on canonical name
- **Gateway** — `GATEWAY_KNOWN_COMMANDS` frozenset for hook emission, `resolve_command()` for dispatch
- **Gateway help** — `gateway_help_lines()` generates `/help` output
- **Telegram** — `telegram_bot_commands()` generates the BotCommand menu
- **Slack** — `slack_subcommand_map()` generates `/hermes` subcommand routing
- **Autocomplete** — `COMMANDS` flat dict feeds `SlashCommandCompleter`
- **CLI help** — `COMMANDS_BY_CATEGORY` dict feeds `show_help()`

### Adding a Slash Command

1. Add a `CommandDef` entry to `COMMAND_REGISTRY` in `hermes_cli/commands.py`:
```python
CommandDef("mycommand", "Description of what it does", "Session",
           aliases=("mc",), args_hint="[arg]"),
```
2. Add handler in `HermesCLI.process_command()` in `cli.py`:
```python
elif canonical == "mycommand":
    self._handle_mycommand(cmd_original)
```
3. If the command is available in the gateway, add a handler in `gateway/run.py`:
```python
if canonical == "mycommand":
    return await self._handle_mycommand(event)
```
4. For persistent settings, use `save_config_value()` in `cli.py`

**CommandDef fields:**
- `name` — canonical name without slash (e.g. `"background"`)
- `description` — human-readable description
- `category` — one of `"Session"`, `"Configuration"`, `"Tools & Skills"`, `"Info"`, `"Exit"`
- `aliases` — tuple of alternative names (e.g. `("bg",)`)
- `args_hint` — argument placeholder shown in help (e.g. `"<prompt>"`, `"[name]"`)
- `cli_only` — only available in the interactive CLI
- `gateway_only` — only available in messaging platforms
- `gateway_config_gate` — config dotpath (e.g. `"display.tool_progress_command"`); when set on a `cli_only` command, the command becomes available in the gateway if the config value is truthy. `GATEWAY_KNOWN_COMMANDS` always includes config-gated commands so the gateway can dispatch them; help/menus only show them when the gate is open.

**Adding an alias** requires only adding it to the `aliases` tuple on the existing `CommandDef`. No other file changes needed — dispatch, help text, Telegram menu, Slack mapping, and autocomplete all update automatically.

---

## Adding New Tools

Requires changes in **2 files**:

**1. Create `tools/your_tool.py`:**
```python
import json, os
from tools.registry import registry

def check_requirements() -> bool:
    return bool(os.getenv("EXAMPLE_API_KEY"))

def example_tool(param: str, task_id: str = None) -> str:
    return json.dumps({"success": True, "data": "..."})

registry.register(
    name="example_tool",
    toolset="example",
    schema={"name": "example_tool", "description": "...", "parameters": {...}},
    handler=lambda args, **kw: example_tool(param=args.get("param", ""), task_id=kw.get("task_id")),
    check_fn=check_requirements,
    requires_env=["EXAMPLE_API_KEY"],
)
```

**2. Add to `toolsets.py`** — either `_HERMES_CORE_TOOLS` (all platforms) or a new toolset.

Auto-discovery: any `tools/*.py` file with a top-level `registry.register()` call is imported automatically — no manual import list to maintain.

The registry handles schema collection, dispatch, availability checking, and error wrapping. All handlers MUST return a JSON string.

**Path references in tool schemas**: If the schema description mentions file paths (e.g. default output directories), use `display_hermes_home()` to make them profile-aware. The schema is generated at import time, which is after `_apply_profile_override()` sets `HERMES_HOME`.

**State files**: If a tool stores persistent state (caches, logs, checkpoints), use `get_hermes_home()` for the base directory — never `Path.home() / ".hermes"`. This ensures each profile gets its own state.

**Agent-level tools** (todo, memory): intercepted by `run_agent.py` before `handle_function_call()`. See `todo_tool.py` for the pattern.

---

## Adding Configuration

### config.yaml options:
1. Add to `DEFAULT_CONFIG` in `hermes_cli/config.py`
2. Bump `_config_version` (currently 5) to trigger migration for existing users

### .env variables:
1. Add to `OPTIONAL_ENV_VARS` in `hermes_cli/config.py` with metadata:
```python
"NEW_API_KEY": {
    "description": "What it's for",
    "prompt": "Display name",
    "url": "https://...",
    "password": True,
    "category": "tool",  # provider, tool, messaging, setting
},
```

### Config loaders (two separate systems):

| Loader | Used by | Location |
|--------|---------|----------|
| `load_cli_config()` | CLI mode | `cli.py` |
| `load_config()` | `hermes tools`, `hermes setup` | `hermes_cli/config.py` |
| Direct YAML load | Gateway | `gateway/run.py` |

---

## Skin/Theme System

The skin engine (`hermes_cli/skin_engine.py`) provides data-driven CLI visual customization. Skins are **pure data** — no code changes needed to add a new skin.

### Architecture

```
hermes_cli/skin_engine.py    # SkinConfig dataclass, built-in skins, YAML loader
~/.hermes/skins/*.yaml       # User-installed custom skins (drop-in)
```

- `init_skin_from_config()` — called at CLI startup, reads `display.skin` from config
- `get_active_skin()` — returns cached `SkinConfig` for the current skin
- `set_active_skin(name)` — switches skin at runtime (used by `/skin` command)
- `load_skin(name)` — loads from user skins first, then built-ins, then falls back to default
- Missing skin values inherit from the `default` skin automatically

### What skins customize

| Element | Skin Key | Used By |
|---------|----------|---------|
| Banner panel border | `colors.banner_border` | `banner.py` |
| Banner panel title | `colors.banner_title` | `banner.py` |
| Banner section headers | `colors.banner_accent` | `banner.py` |
| Banner dim text | `colors.banner_dim` | `banner.py` |
| Banner body text | `colors.banner_text` | `banner.py` |
| Response box border | `colors.response_border` | `cli.py` |
| Spinner faces (waiting) | `spinner.waiting_faces` | `display.py` |
| Spinner faces (thinking) | `spinner.thinking_faces` | `display.py` |
| Spinner verbs | `spinner.thinking_verbs` | `display.py` |
| Spinner wings (optional) | `spinner.wings` | `display.py` |
| Tool output prefix | `tool_prefix` | `display.py` |
| Per-tool emojis | `tool_emojis` | `display.py` → `get_tool_emoji()` |
| Agent name | `branding.agent_name` | `banner.py`, `cli.py` |
| Welcome message | `branding.welcome` | `cli.py` |
| Response box label | `branding.response_label` | `cli.py` |
| Prompt symbol | `branding.prompt_symbol` | `cli.py` |

### Built-in skins

- `default` — Classic Hermes gold/kawaii
- `ares` — Crimson/bronze war-god theme with custom spinner wings
- `mono` — Clean grayscale monochrome
- `slate` — Cool blue developer-focused theme

### Adding a built-in skin

Add to `_BUILTIN_SKINS` dict in `hermes_cli/skin_engine.py`:

```python
"mytheme": {
    "name": "mytheme",
    "description": "Short description",
    "colors": { ... },
    "spinner": { ... },
    "branding": { ... },
    "tool_prefix": "┊",
},
```

### User skins (YAML)

Users create `~/.hermes/skins/<name>.yaml`:

```yaml
name: cyberpunk
description: Neon-soaked terminal theme

colors:
  banner_border: "#FF00FF"
  banner_title: "#00FFFF"
  banner_accent: "#FF1493"

spinner:
  thinking_verbs: ["jacking in", "decrypting", "uploading"]
  wings:
    - ["⟨⚡", "⚡⟩"]

branding:
  agent_name: "Cyber Agent"
  response_label: " ⚡ Cyber "

tool_prefix: "▏"
```

Activate with `/skin cyberpunk` or `display.skin: cyberpunk` in config.yaml.

---

## Testing Strategy

### Test Organization

| Directory | Contents |
|-----------|----------|
| `tests/agent/` | Agent internals (prompt builder, context compressor, memory, etc.) |
| `tests/tools/` | Individual tool tests |
| `tests/gateway/` | Platform adapters and gateway logic |
| `tests/hermes_cli/` | CLI commands, config, skins, models |
| `tests/cron/` | Scheduler tests |
| `tests/acp/` | ACP adapter tests |
| `tests/e2e/` | End-to-end tests |
| `tests/integration/` | External service integration tests (skipped by default) |
| `tests/fakes/` | Mock fixtures and fake implementations |

### Key Fixtures (`tests/conftest.py`)

- `_isolate_hermes_home` (autouse) — redirects `HERMES_HOME` to a temp dir so tests never write to `~/.hermes/`
- `_enforce_test_timeout` (autouse) — kills any test hanging longer than 30 seconds via SIGALRM (Unix only)
- `_ensure_current_event_loop` (autouse) — provides a default event loop for sync tests that call `get_event_loop()`
- `mock_config` — minimal config dict for unit tests

### Running Tests

- Default pytest invocation skips integration tests (`-m 'not integration'`)
- Use `pytest-xdist` (`-n auto`) for parallel execution
- CI runs unit tests with `-n auto` and E2E tests separately with empty API keys to prevent accidental real calls

### Test Markers

```python
pytest.mark.integration   # Requires external services (API keys, Modal, etc.)
```

---

## Security Considerations

### Trust Model

Hermes is a **personal agent** with one trusted operator. Multi-user isolation must happen at the OS/host level.

### Key Security Boundaries

1. **Dangerous Command Approval** (`tools/approval.py`)
   - Terminal commands, file operations, and destructive actions require explicit user confirmation
   - Configurable via `approvals.mode`: `"on"` (default), `"auto"`, `"off"`

2. **Output Redaction** (`agent/redact.py`)
   - Strips secret-like patterns (API keys, tokens) from display output before it reaches terminal or gateway
   - Operates on the display layer only; internal values remain intact

3. **Code Execution Sandbox** (`tools/code_execution_tool.py`)
   - Runs LLM-generated Python in a child process with host credentials stripped
   - Only `env_passthrough` variables are passed through
   - Child accesses Hermes tools via RPC, not direct API calls

4. **MCP Server Isolation** (`tools/mcp_tool.py`)
   - MCP subprocesses receive a filtered environment (`_build_safe_env()`)
   - Only safe baseline variables + explicitly declared `env` config variables are passed
   - `npx`/`uvx` packages are checked against the OSV malware database before spawning

5. **Subagent Restrictions** (`tools/delegate_tool.py`)
   - `delegate_task` is disabled for child agents (no recursive delegation)
   - Max depth = 2 (parent → child; grandchildren rejected)
   - Child agents run with `skip_memory=True` (no parent memory access)

6. **Skills Guard** (`tools/skills_guard.py`)
   - Audits third-party skills before installation
   - Audit log at `~/.hermes/skills/.hub/audit.log`

7. **SSRF Protection**
   - Enabled by default across all gateway platform adapters
   - Redirect validation on outbound requests

### CI/CD Supply Chain Hardening

- `.github/workflows/supply-chain-audit.yml` scans PRs for:
  - `.pth` files (auto-execute on Python startup)
  - `base64` + `exec`/`eval` combos
  - `subprocess` with encoded commands
  - Unpinned GitHub Actions (mutable tags)
- All GitHub Actions are pinned to full commit SHAs
- Dependencies in `pyproject.toml` are pinned to known-good ranges

### Reporting Vulnerabilities

- Do not open public issues for security vulnerabilities
- Report via [GitHub Security Advisories](https://github.com/NousResearch/hermes-agent/security/advisories/new) or email **security@nousresearch.com**
- Coordinated disclosure: 90-day window or until fix released

---

## Deployment

### Docker

Multi-arch builds (`linux/amd64`, `linux/arm64`) are published to Docker Hub on push to `main` and on releases. The Dockerfile:
- Uses Debian 13 (trixie) base
- Runs as non-root user `hermes` (UID 10000)
- Installs Node.js, Playwright, and Python deps in a virtualenv
- Volume-mounts `/opt/data` for persistent state
- Entrypoint bootstraps config files into the volume at first run

### GitHub Pages

Documentation site (landing page + Docusaurus) is deployed automatically on pushes affecting `website/`, `landingpage/`, or `skills/`.

### Nix

- `flake.nix` defines packages, dev shell, NixOS modules, and checks
- CI validates `nix flake check` on Linux and `nix flake show` on macOS
- Supports `x86_64-linux`, `aarch64-linux`, `aarch64-darwin`

---

## Important Policies

### Prompt Caching Must Not Break

Hermes-Agent ensures caching remains valid throughout a conversation. **Do NOT implement changes that would:**
- Alter past context mid-conversation
- Change toolsets mid-conversation
- Reload memories or rebuild system prompts mid-conversation

Cache-breaking forces dramatically higher costs. The ONLY time we alter context is during context compression.

### Working Directory Behavior
- **CLI**: Uses current directory (`.` → `os.getcwd()`)
- **Messaging**: Uses `MESSAGING_CWD` env var (default: home directory)

### Background Process Notifications (Gateway)

When `terminal(background=true, notify_on_complete=true)` is used, the gateway runs a watcher that detects process completion and triggers a new agent turn. Control verbosity with `display.background_process_notifications` in config.yaml:

- `all` — running-output updates + final message (default)
- `result` — only the final completion message
- `error` — only the final message when exit code != 0
- `off` — no watcher messages at all

---

## Profiles: Multi-Instance Support

Hermes supports **profiles** — multiple fully isolated instances, each with its own `HERMES_HOME` directory (config, API keys, memory, sessions, skills, gateway, etc.).

The core mechanism: `_apply_profile_override()` in `hermes_cli/main.py` sets `HERMES_HOME` before any module imports. All 119+ references to `get_hermes_home()` automatically scope to the active profile.

### Rules for profile-safe code

1. **Use `get_hermes_home()` for all HERMES_HOME paths.** Import from `hermes_constants`. NEVER hardcode `~/.hermes` or `Path.home() / ".hermes"` in code that reads/writes state.
   ```python
   # GOOD
   from hermes_constants import get_hermes_home
   config_path = get_hermes_home() / "config.yaml"

   # BAD — breaks profiles
   config_path = Path.home() / ".hermes" / "config.yaml"
   ```

2. **Use `display_hermes_home()` for user-facing messages.** Import from `hermes_constants`.
   ```python
   # GOOD
   from hermes_constants import display_hermes_home
   print(f"Config saved to {display_hermes_home()}/config.yaml")

   # BAD — shows wrong path for profiles
   print("Config saved to ~/.hermes/config.yaml")
   ```

3. **Module-level constants are fine** — they cache `get_hermes_home()` at import time, which is AFTER `_apply_profile_override()` sets the env var. Just use `get_hermes_home()`, not `Path.home() / ".hermes"`.

4. **Tests that mock `Path.home()` must also set `HERMES_HOME`** — since code now uses `get_hermes_home()` (reads env var), not `Path.home() / ".hermes"`:
   ```python
   with patch.object(Path, "home", return_value=tmp_path), \
        patch.dict(os.environ, {"HERMES_HOME": str(tmp_path / ".hermes")}):
       ...
   ```

5. **Gateway platform adapters should use token locks** — if the adapter connects with a unique credential (bot token, API key), call `acquire_scoped_lock()` from `gateway.status` in the `connect()`/`start()` method and `release_scoped_lock()` in `disconnect()`/`stop()`. This prevents two profiles from using the same credential. See `gateway/platforms/telegram.py` for the canonical pattern.

6. **Profile operations are HOME-anchored, not HERMES_HOME-anchored** — `_get_profiles_root()` returns `Path.home() / ".hermes" / "profiles"`, NOT `get_hermes_home() / "profiles"`. This is intentional — it lets `hermes -p coder profile list` see all profiles regardless of which one is active.

---

## Known Pitfalls

### DO NOT hardcode `~/.hermes` paths
Use `get_hermes_home()` from `hermes_constants` for code paths. Use `display_hermes_home()` for user-facing print/log messages. Hardcoding `~/.hermes` breaks profiles — each profile has its own `HERMES_HOME` directory. This was the source of 5 bugs fixed in PR #3575.

### DO NOT use `simple_term_menu` for interactive menus
Rendering bugs in tmux/iTerm2 — ghosting on scroll. Use `curses` (stdlib) instead. See `hermes_cli/tools_config.py` for the pattern.

### DO NOT use `\033[K` (ANSI erase-to-EOL) in spinner/display code
Leaks as literal `?[K` text under `prompt_toolkit`'s `patch_stdout`. Use space-padding: `f"\r{line}{' ' * pad}"`.

### `_last_resolved_tool_names` is a process-global in `model_tools.py`
`_run_single_child()` in `delegate_tool.py` saves and restores this global around subagent execution. If you add new code that reads this global, be aware it may be temporarily stale during child agent runs.

### DO NOT hardcode cross-tool references in schema descriptions
Tool schema descriptions must not mention tools from other toolsets by name (e.g., `browser_navigate` saying "prefer web_search"). Those tools may be unavailable (missing API keys, disabled toolset), causing the model to hallucinate calls to non-existent tools. If a cross-reference is needed, add it dynamically in `get_tool_definitions()` in `model_tools.py` — see the `browser_navigate` / `execute_code` post-processing blocks for the pattern.

### Tests must not write to `~/.hermes/`
The `_isolate_hermes_home` autouse fixture in `tests/conftest.py` redirects `HERMES_HOME` to a temp dir. Never hardcode `~/.hermes/` paths in tests.

**Profile tests**: When testing profile features, also mock `Path.home()` so that `_get_profiles_root()` and `_get_default_hermes_home()` resolve within the temp dir. Use the pattern from `tests/hermes_cli/test_profiles.py`:
```python
@pytest.fixture
def profile_env(tmp_path, monkeypatch):
    home = tmp_path / ".hermes"
    home.mkdir()
    monkeypatch.setattr(Path, "home", lambda: tmp_path)
    monkeypatch.setenv("HERMES_HOME", str(home))
    return home
```
