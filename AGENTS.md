# Hermes Agent - Development Guide

Instructions for AI coding assistants and developers working on the hermes-agent codebase.

Hermes Agent is a self-improving AI agent built by Nous Research. It features a synchronous Python core, an async messaging gateway, a Rust rewrite in progress, a React web frontend, a Docusaurus documentation site, and a VS Code / JetBrains / Zed IDE integration via ACP.

---

## Project Overview

Hermes is a personal AI agent with a built-in learning loop. It creates skills from experience, improves them during use, persists knowledge across sessions, searches its own conversation history, and runs on multiple surfaces:

- **Interactive CLI** (`hermes`) — TUI with multiline editing, slash commands, and streaming output
- **Messaging Gateway** (`hermes gateway`) — Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Email, and more
- **Web Dashboard** (`web/`) — React + Vite frontend for the analytics dashboard
- **IDE Integration** (`acp_adapter/`, `hermes-rs/crates/hermes-acp`) — Agent Client Protocol server for editor integration
- **Batch Runner** (`batch_runner.py`) — Parallel trajectory generation on JSONL datasets
- **RL Training** (`environments/`, `tinker-atropos/`) — Atropos RL environments for training tool-calling models

The project has **two parallel implementations**:
- **Python** (`run_agent.py`, `cli.py`, `hermes_cli/`, `agent/`, `tools/`, `gateway/`) — the current production system
- **Rust** (`hermes-rs/`) — a full rewrite with workspace crates for core, state, LLM, tools, CLI, gateway, cron, batch, compression, ACP, and RL

**User config:** `~/.hermes/config.yaml` (settings), `~/.hermes/.env` (API keys)

---

## Technology Stack

| Layer | Technology |
|-------|-----------|
| **Python runtime** | CPython 3.11+ (managed by `uv`) |
| **Rust runtime** | Rust 1.84+ (`hermes-rs/`) |
| **Package manager** | `uv` for Python; `cargo` for Rust |
| **Core deps** | `openai`, `anthropic`, `httpx`, `rich`, `prompt_toolkit`, `pydantic`, `jinja2`, `tenacity`, `pyyaml` |
| **Gateway deps** | `python-telegram-bot`, `discord.py`, `slack-bolt`, `aiohttp` |
| **Browser tools** | Playwright (`agent-browser`, `@askjo/camofox-browser`) |
| **Database** | SQLite with FTS5 full-text search (`hermes_state.py`) |
| **Web frontend** | React 19, Vite 7, Tailwind CSS 4, TypeScript 5.9 (`web/`) |
| **Docs site** | Docusaurus 3.9, TypeScript (`website/`) |
| **Container** | Debian 13 (trixie) base, non-root UID 10000 |
| **Nix** | `flake.nix` with `flake-parts`, `uv2nix`, `pyproject-nix` |
| **CI/CD** | GitHub Actions (tests, e2e, Docker publish, supply chain audit, docs checks) |

---

## Key Configuration Files

| File | Purpose |
|------|---------|
| `pyproject.toml` | Python package metadata, dependencies, optional extras, pytest config, setuptools config |
| `hermes-rs/Cargo.toml` | Rust workspace definition with 12 crates |
| `flake.nix` | Nix flake for reproducible builds, dev shell, and NixOS modules |
| `Dockerfile` | Multi-stage container build (uv + gosu + Debian) |
| `package.json` | Root Node.js deps for browser automation tools |
| `web/package.json` | React frontend dependencies (Vite, Tailwind, React Router) |
| `website/package.json` | Docusaurus documentation site dependencies |
| `cli-config.yaml.example` | Example user configuration file |
| `.github/workflows/tests.yml` | CI test runner (pytest + e2e) |
| `.github/workflows/docker-publish.yml` | Multi-arch Docker image build and push |
| `.github/workflows/supply-chain-audit.yml` | PR security scanning for supply chain risks |

---

## Build, Test, and Development Commands

### Python Development

```bash
# Install uv first: curl -LsSf https://astral.sh/uv/install.sh | sh

# Create venv and install all extras
cd hermes-agent
uv venv venv --python 3.11
source venv/bin/activate  # ALWAYS activate before running Python
uv pip install -e ".[all,dev]"

# Optional: RL training submodule
git submodule update --init tinker-atropos
uv pip install -e "./tinker-atropos"

# Optional: browser tools (Node.js 18+ required)
npm install
npx playwright install --with-deps chromium
```

### Rust Development

```bash
cd hermes-rs
cargo build --release
cargo test
```

Bins: `hermes` (CLI), `hermes-agent` (agent engine), `hermes-acp` (ACP server).

### Web Frontend

```bash
cd web
npm install
npm run dev      # Vite dev server
npm run build    # Production build
npm run lint     # ESLint
```

### Documentation Site

```bash
cd website
npm install
npm run start    # Docusaurus dev server
npm run build    # Static site build
```

### Testing

```bash
source venv/bin/activate

# Full suite (~3000 tests, ~3 min)
python -m pytest tests/ -q

# Specific areas
python -m pytest tests/test_model_tools.py -q        # Toolset resolution
python -m pytest tests/test_cli_init.py -q           # CLI config loading
python -m pytest tests/gateway/ -q                   # Gateway tests
python -m pytest tests/tools/ -q                     # Tool-level tests
python -m pytest tests/hermes_cli/ -q                # CLI subcommand tests
python -m pytest tests/agent/ -q                     # Agent internals
python -m pytest tests/e2e/ -v                       # End-to-end tests

# With integration tests (requires real API keys)
python -m pytest tests/ -q -m integration
```

Always run the full suite before pushing changes.

### Docker

```bash
# Build locally
docker build -t hermes-agent .

# Run with persistent data volume
docker run -v /path/to/data:/opt/data hermes-agent
```

---

## Code Organization and Module Divisions

```
hermes-agent/
├── run_agent.py              # AIAgent class — core conversation loop
├── cli.py                    # HermesCLI class — interactive CLI orchestrator
├── model_tools.py            # Tool orchestration, discovery, dispatch
├── toolsets.py               # Toolset definitions and platform presets
├── batch_runner.py           # Parallel batch processing on JSONL
├── trajectory_compressor.py  # Trajectory compression for training
├── hermes_state.py           # SessionDB — SQLite with FTS5 search
├── hermes_constants.py       # HERMES_HOME, profile helpers
├── hermes_logging.py         # Structured logging setup
├── hermes_time.py            # Timezone handling
├── utils.py                  # Shared utilities
│
├── agent/                    # Agent internals
│   ├── prompt_builder.py         # System prompt assembly
│   ├── context_compressor.py     # Auto context compression
│   ├── prompt_caching.py         # Anthropic prompt caching
│   ├── auxiliary_client.py       # Auxiliary LLM client (vision, summarization)
│   ├── model_metadata.py         # Model context lengths, token estimation
│   ├── models_dev.py             # models.dev registry integration
│   ├── display.py                # KawaiiSpinner, tool preview formatting
│   ├── skill_commands.py         # Skill slash commands (shared CLI/gateway)
│   ├── trajectory.py             # Trajectory saving helpers
│   ├── memory_manager.py         # Persistent memory context building
│   ├── retry_utils.py            # Jittered backoff, error classification
│   ├── error_classifier.py       # API error classification for failover
│   ├── smart_model_routing.py    # Provider routing logic
│   └── ...
│
├── hermes_cli/               # CLI subcommands and setup
│   ├── main.py                   # Entry point — argument parsing, dispatch
│   ├── config.py                 # DEFAULT_CONFIG, OPTIONAL_ENV_VARS, migration
│   ├── commands.py               # Slash command registry (CommandDef)
│   ├── callbacks.py              # Terminal callbacks (clarify, sudo, approval)
│   ├── setup.py                  # Interactive setup wizard
│   ├── skin_engine.py            # Skin/theme engine
│   ├── skills_config.py          # `hermes skills` subcommands
│   ├── tools_config.py           # `hermes tools` subcommands
│   ├── skills_hub.py             # Skills Hub CLI + /skills slash command
│   ├── models.py                 # Model catalog and provider lists
│   ├── model_switch.py           # Shared /model switch pipeline
│   ├── auth.py                   # Provider credential resolution
│   ├── doctor.py                 # Diagnostics
│   ├── banner.py                 # Welcome banner and ASCII art
│   ├── gateway.py                # Gateway service management
│   ├── profiles.py               # Profile management
│   └── ... (50+ files total)
│
├── tools/                    # Tool implementations (one file per tool)
│   ├── registry.py               # Central tool registry (schemas, handlers, dispatch)
│   ├── approval.py               # Dangerous command detection
│   ├── terminal_tool.py          # Terminal orchestration
│   ├── process_registry.py       # Background process management
│   ├── file_tools.py             # File read/write/search/patch
│   ├── web_tools.py              # Web search/extract (Parallel + Firecrawl)
│   ├── browser_tool.py           # Browserbase / Playwright automation
│   ├── code_execution_tool.py    # execute_code sandbox
│   ├── delegate_tool.py          # Subagent delegation
│   ├── mcp_tool.py               # MCP client (~1050 lines)
│   ├── session_search_tool.py    # FTS5 session search + summarization
│   ├── cronjob_tools.py          # Scheduled task management
│   ├── skill_tools.py            # Skill search, load, manage
│   ├── memory_tool.py            # Persistent memory operations
│   ├── todo_tool.py              # Todo list management
│   └── environments/             # Terminal backends
│       ├── base.py, local.py, docker.py, ssh.py, modal.py, daytona.py, singularity.py
│
├── gateway/                  # Messaging platform gateway
│   ├── run.py                    # Main loop, slash commands, message dispatch
│   ├── session.py                # SessionStore — conversation persistence
│   ├── config.py                 # Platform configuration resolution
│   ├── hooks.py                  # Gateway hook system
│   ├── pairing.py                # Device pairing and authorization
│   ├── status.py                 # Gateway status and token locks
│   └── platforms/                # Adapters
│       ├── telegram.py, discord.py, slack.py, whatsapp.py, signal.py,
│       ├── matrix.py, email.py, homeassistant.py, webhook.py, dingtalk.py,
│       ├── feishu.py, wecom.py, weixin.py, qqbot.py, bluebubbles.py, sms.py,
│       └── mattermost.py
│
├── acp_adapter/              # ACP server (Python implementation)
│   ├── server.py, session.py, tools.py, auth.py, events.py, permissions.py
│
├── cron/                     # Scheduler
│   ├── jobs.py, scheduler.py
│
├── skills/                   # Bundled skills (copied to ~/.hermes/skills/ on install)
├── optional-skills/          # Official optional skills (discoverable, not activated by default)
│
├── environments/             # RL training environments (Atropos integration)
│   ├── agent_loop.py, hermes_base_env.py, agentic_opd_env.py, hermes_swe_env/
│
├── hermes-rs/                # Rust rewrite
│   ├── crates/
│   │   ├── hermes-core, hermes-state, hermes-llm, hermes-tools,
│   │   ├── hermes-prompt, hermes-agent-engine, hermes-cli,
│   │   ├── hermes-gateway, hermes-cron, hermes-batch, hermes-compress,
│   │   ├── hermes-acp, hermes-rl
│   │   └── ...
│   └── src/main.rs             # CLI entry point
│
├── web/                      # React frontend (analytics dashboard)
├── website/                  # Docusaurus documentation site
├── tests/                    # Pytest suite (~3000 tests)
├── scripts/                  # Installers (install.sh, install.ps1, install.cmd)
├── docker/                   # Docker entrypoint and SOUL.md
└── nix/                      # Nix packages, devShell, checks, NixOS modules
```

---

## File Dependency Chain

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

## Core Architecture

### AIAgent Class (`run_agent.py`)

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

## CLI and Gateway Architecture

### CLI (`cli.py` + `hermes_cli/`)

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

## Adding Skills

Skills live in `skills/` (bundled) or `optional-skills/` (official but not activated by default). Each skill is a directory with a `SKILL.md` file and optional `scripts/` or `references/`.

### SKILL.md frontmatter

```yaml
---
name: my-skill
description: Brief description
version: 1.0.0
author: Your Name
license: MIT
platforms: [macos, linux]          # Optional OS restriction
required_environment_variables:
  - name: MY_API_KEY
    prompt: API key
    help: Where to get it
    required_for: full functionality
metadata:
  hermes:
    tags: [Category, Subcategory]
    fallback_for_toolsets: [web]       # Show only when toolset unavailable
    requires_toolsets: [terminal]      # Show only when toolset available
---
```

Skills self-register at agent startup via `agent/skill_commands.py`. They are injected into the system prompt, not executed as code.

**When to add a skill vs. a tool:**
- **Skill** — capability expressible as instructions + shell commands + existing tools (most cases)
- **Tool** — requires end-to-end API integration, custom binary processing, streaming, or real-time events

See `CONTRIBUTING.md` for the full skill authoring guide.

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

- `default` — Classic Hermes gold/kawaii (the current look)
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

## Development Conventions

### Code Style

- **PEP 8** with practical exceptions (we don't enforce strict line length)
- **Comments**: Only when explaining non-obvious intent, trade-offs, or API quirks. Don't narrate what the code does
- **Error handling**: Catch specific exceptions. Log with `logger.warning()`/`logger.error()` — use `exc_info=True` for unexpected errors so stack traces appear in logs
- **Cross-platform**: Never assume Unix. See Known Pitfalls below for specific rules

### Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

| Type | Use for |
|------|---------|
| `fix` | Bug fixes |
| `feat` | New features |
| `docs` | Documentation |
| `test` | Tests |
| `refactor` | Code restructuring (no behavior change) |
| `chore` | Build, CI, dependency updates |

Scopes: `cli`, `gateway`, `tools`, `skills`, `agent`, `install`, `whatsapp`, `security`, etc.

Examples:
```
fix(cli): prevent crash in save_config_value when model is a string
feat(gateway): add WhatsApp multi-user session isolation
fix(security): prevent shell injection in sudo password piping
test(tools): add unit tests for file_operations
```

### Branch Naming

```
fix/description        # Bug fixes
feat/description       # New features
docs/description       # Documentation
test/description       # Tests
refactor/description   # Code restructuring
```

### Pull Request Process

1. **Run tests**: `pytest tests/ -v`
2. **Test manually**: Run `hermes` and exercise the code path you changed
3. **Check cross-platform impact**: If you touch file I/O, process management, or terminal handling, consider Windows and macOS
4. **Keep PRs focused**: One logical change per PR

---

## Testing Strategy

The test suite uses **pytest** with the following characteristics:

- **~3000 tests** across unit, integration, and e2e layers
- **pytest-xdist** runs tests in parallel (`-n auto`)
- **pytest-asyncio** for async gateway tests
- **Integration tests** are marked with `@pytest.mark.integration` and skipped by default (require real API keys)
- **Test isolation** via `_isolate_hermes_home` autouse fixture in `tests/conftest.py` — redirects `HERMES_HOME` to a temp dir so tests never write to `~/.hermes/`
- **30-second timeout** per test on Unix (SIGALRM) to prevent hangs
- **Event loop fixture** ensures synchronous tests that call `asyncio.get_event_loop()` have a usable loop

### Test organization

| Directory | Contents |
|-----------|----------|
| `tests/agent/` | Agent internals (prompt builder, compression, memory, routing) |
| `tests/cli/` | CLI interaction tests |
| `tests/gateway/` | Platform adapters and gateway core |
| `tests/tools/` | Individual tool tests |
| `tests/hermes_cli/` | CLI subcommand tests |
| `tests/e2e/` | End-to-end tests |
| `tests/integration/` | Tests requiring external services |
| `tests/fakes/` | Fake/stub implementations for tests |

---

## Deployment and Packaging

### Docker

- Multi-arch builds (`linux/amd64`, `linux/arm64`) published to `nousresearch/hermes-agent`
- Tags: `latest` on main branch pushes, release tags on GitHub releases
- Non-root runtime user (UID 10000) with `gosu`
- Playwright browsers pre-installed at build time
- Volume mount at `/opt/data` for persistent state

### Nix

- `flake.nix` supports `x86_64-linux`, `aarch64-linux`, `aarch64-darwin`
- Imports: `nix/packages.nix`, `nix/nixosModules.nix`, `nix/checks.nix`, `nix/devShell.nix`
- Uses `uv2nix` + `pyproject-nix` for Python dependency locking

### Homebrew

- Formula maintained in `packaging/homebrew/`

### Scripts

- `scripts/install.sh` — Linux/macOS/WSL2 installer
- `scripts/install.ps1` — Windows PowerShell installer (WSL2 recommended)
- `scripts/install.cmd` — Windows CMD wrapper
- `scripts/release.py` — Release automation

---

## Security Considerations

### Existing Protections

| Layer | Implementation |
|-------|---------------|
| **Sudo password piping** | Uses `shlex.quote()` to prevent shell injection |
| **Dangerous command detection** | Regex patterns in `tools/approval.py` with user approval flow |
| **Cron prompt injection** | Scanner in `tools/cronjob_tools.py` blocks instruction-override patterns |
| **Write deny list** | Protected paths (`~/.ssh/authorized_keys`, `/etc/shadow`) resolved via `os.path.realpath()` to prevent symlink bypass |
| **Skills guard** | Security scanner for hub-installed skills (`tools/skills_guard.py`) |
| **Code execution sandbox** | `execute_code` child process runs with API keys stripped from environment |
| **MCP safety** | OSV malware checking for `npx`/`uvx` packages before spawning |
| **Container hardening** | Docker: all capabilities dropped, no privilege escalation, PID limits, size-limited tmpfs |
| **Output redaction** | `agent/redact.py` strips secrets from display output before it reaches terminal or gateway |
| **Supply chain audit** | CI workflow blocks `.pth` files, `base64`+`exec` combos, and mutable Action tags |

### When Contributing Security-Sensitive Code

- **Always use `shlex.quote()`** when interpolating user input into shell commands
- **Resolve symlinks** with `os.path.realpath()` before path-based access control checks
- **Don't log secrets.** API keys, tokens, and passwords should never appear in log output
- **Catch broad exceptions** around tool execution so a single failure doesn't crash the agent loop
- **Test on all platforms** if your change touches file paths, process management, or shell commands

If your PR affects security, note it explicitly in the description.

### Trust Model

Hermes is designed as a **single-tenant personal agent**. The operator is trusted. Gateway platforms (Telegram, Discord, etc.) receive equal trust once authorized. Multi-user isolation must happen at the OS/host level. See `SECURITY.md` for the full trust model, vulnerability reporting process, and out-of-scope scenarios.

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

When `terminal(background=true, notify_on_complete=true)` is used, the gateway runs a watcher that detects process completion and triggers a new agent turn. Control verbosity with `display.background_process_notifications` in config.yaml (or `HERMES_BACKGROUND_NOTIFICATIONS` env var):

- `all` — running-output updates + final message (default)
- `result` — only the final completion message
- `error` — only the final message when exit code != 0
- `off` — no watcher messages at all

---

## Profiles: Multi-Instance Support

Hermes supports **profiles** — multiple fully isolated instances, each with its own `HERMES_HOME` directory (config, API keys, memory, sessions, skills, gateway, etc.).

The core mechanism: `_apply_profile_override()` in `hermes_cli/main.py` sets `HERMES_HOME` before any module imports. All 119+ references to `get_hermes_home()` automatically scope to the active profile.

### Rules for profile-safe code

1. **Use `get_hermes_home()` for all HERMES_HOME paths.** Import from `hermes_constants`.
   NEVER hardcode `~/.hermes` or `Path.home() / ".hermes"` in code that reads/writes state.
   ```python
   # GOOD
   from hermes_constants import get_hermes_home
   config_path = get_hermes_home() / "config.yaml"

   # BAD — breaks profiles
   config_path = Path.home() / ".hermes" / "config.yaml"
   ```

2. **Use `display_hermes_home()` for user-facing messages.** Import from `hermes_constants`.
   This returns `~/.hermes` for default or `~/.hermes/profiles/<name>` for profiles.
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

### Cross-Platform Rules

1. **`termios` and `fcntl` are Unix-only.** Always catch both `ImportError` and `NotImplementedError`.
2. **File encoding.** Windows may save `.env` files in `cp1252`. Always handle encoding errors with fallback to `latin-1`.
3. **Process management.** `os.setsid()`, `os.killpg()`, and signal handling differ on Windows. Use `platform.system() != "Windows"` checks.
4. **Path separators.** Use `pathlib.Path` instead of string concatenation with `/`.
5. **Shell commands in installers.** If you change `scripts/install.sh`, check if the equivalent change is needed in `scripts/install.ps1`.
