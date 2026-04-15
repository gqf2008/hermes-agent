# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Hermes Agent** is a self-improving AI agent by Nous Research. It's a conversational AI agent with tool-calling capabilities that runs as a CLI application or a messaging gateway (Telegram, Discord, Slack, WhatsApp, Signal, Feishu, etc.).

- **Closed learning loop**: Creates skills from experience, self-improves during use
- **Multi-platform**: CLI + 15+ messaging platforms from a single gateway
- **Model agnostic**: OpenRouter, OpenAI, Anthropic, and many others
- **6 terminal backends**: local, Docker, SSH, Daytona, Singularity, Modal

## Tech Stack

- **Python 3.11+**, package management via `uv` (pip fallback)
- **Core deps**: `openai`, `anthropic`, `pydantic`, `rich`, `prompt_toolkit`, `jinja2`, `httpx`, `tenacity`, `fire`
- **Testing**: pytest, pytest-asyncio, pytest-xdist

## Key Commands

```bash
# Setup (first time)
uv venv venv --python 3.11
uv pip install -e ".[all,dev]"

# Always activate venv first
source venv/bin/activate  # (or activate on Windows)

# Run the agent CLI
hermes                          # interactive CLI
hermes-agent                    # core agent conversation loop
hermes gateway                  # start messaging gateway
hermes setup                    # interactive setup wizard
hermes tools                    # manage tool configurations
hermes skills                   # manage skill configurations

# Tests
python -m pytest tests/ -q                          # full suite (~3000 tests, parallel)
python -m pytest tests/test_model_tools.py -q       # single test file
python -m pytest tests/gateway/ -q                  # by subsystem
python -m pytest tests/tools/ -q
python -m pytest tests/hermes_cli/ -q
python -m pytest tests/agent/ -q
python -m pytest tests/integration/ -v              # integration tests (needs API keys)
```

## Architecture

### Entry Points (defined in pyproject.toml)

| Command | Entry Point | Purpose |
|---------|-------------|---------|
| `hermes` | `hermes_cli.main:main` | CLI subcommands (setup, tools, skills, gateway, etc.) |
| `hermes-agent` | `run_agent:main` | Core agent conversation loop |
| `hermes-acp` | `acp_adapter.entry:main` | VS Code/Zed/JetBrains IDE integration |

### Core File Dependency Chain

```
tools/registry.py  (no deps — imported by all tool files)
       ↑
tools/*.py  (each calls registry.register() at import time)
       ↑
model_tools.py  (imports registry + triggers tool discovery)
       ↑
run_agent.py, cli.py, batch_runner.py, environments/
```

### Key Directories

| Directory | Purpose |
|-----------|---------|
| `run_agent.py` | `AIAgent` class — core conversation loop |
| `model_tools.py` | Tool orchestration, `_discover_tools()`, `handle_function_call()` |
| `toolsets.py` | Toolset definitions, `_HERMES_CORE_TOOLS` list |
| `cli.py` | `HermesCLI` class — interactive CLI orchestrator |
| `hermes_state.py` | `SessionDB` — SQLite session store with FTS5 search |
| `agent/` | Agent internals: prompt building, context compression, prompt caching, auxiliary LLM, model metadata, display, skill commands |
| `hermes_cli/` | CLI subcommands: `main.py`, `config.py` (defaults/env vars/migration), `commands.py` (slash commands), `setup.py`, `skin_engine.py`, `skills_hub.py`, `models.py`, `model_switch.py`, `auth.py` |
| `tools/` | ~60 tool implementations: file ops, web, browser, code exec, delegation, MCP client, terminal, TTS, voice, memory, cron, RL training |
| `tools/environments/` | Terminal backends: local, Docker, SSH, Modal, Daytona, Singularity |
| `gateway/` | Messaging platform gateway: `run.py` (main loop), `session.py` (session store), `platforms/` (15+ adapters) |
| `acp_adapter/` | Agent Client Protocol server for IDE integration |
| `cron/` | Scheduler: `jobs.py`, `scheduler.py` |
| `skills/` | Bundled skills organized by category |
| `environments/` | RL training environments (Atropos integration) |
| `tests/` | ~3000 pytest tests organized by subsystem |

### User Config Location

- `~/.hermes/config.yaml` (settings)
- `~/.hermes/.env` (API keys)
- Multi-profile support via `HERMES_HOME` env var — all state is scoped per-profile

## Critical Development Rules

### Path Handling
- **DO NOT hardcode `~/.hermes` paths** — use `get_hermes_home()` from `hermes_constants` for code, `display_hermes_home()` for user-facing messages (profile support)

### Testing
- **Tests must NOT write to `~/.hermes/`** — the `_isolate_hermes_home` fixture in `tests/conftest.py` redirects `HERMES_HOME` to a temp directory
- Tests have a **30-second per-test timeout** (enforced by `_enforce_test_timeout` fixture, skipped on Windows)
- Integration tests are **excluded by default** via pytest config

### UI/Terminal
- **DO NOT use `simple_term_menu`** — use `curses` instead (rendering bugs in tmux/iTerm2)
- **DO NOT use `\033[K`** in spinner code — it leaks under prompt_toolkit; use space-padding instead

### Prompt Caching
- **Prompt caching must not break** — never alter past context, change toolsets, or reload memories mid-conversation

## How to Add Things

### Adding a Tool (3 files required)
1. Create `tools/your_tool.py` with `registry.register()` call
2. Import in `model_tools.py` `_discover_tools()`
3. Assign toolset in `toolsets.py`

### Adding a Slash Command
1. Add `CommandDef` entry in `hermes_cli/commands.py`
2. Add handler in `cli.py` `process_command()`
3. Optionally update `gateway/run.py` for gateway support

### Adding Configuration
1. Update `DEFAULT_CONFIG` and `OPTIONAL_ENV_VARS` in `hermes_cli/config.py`
2. Bump `_config_version` for migration support
