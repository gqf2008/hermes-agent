# Python-Rust 源码级完整对齐报告

> 生成时间：2026-04-17
> 范围：Hermes Agent 全模块（不含 Gateway 国内平台以外的适配器）
> Python 基准：`E:\Users\gxh\Documents\GitHub\hermes-agent`
> Rust 目标：`E:\Users\gxh\Documents\GitHub\hermez-ai`

---

## 1. 执行摘要

| 维度 | Python | Rust | 整体完成度 |
|------|--------|------|-----------|
| 总源码文件 | ~896 .py | 245 .rs | — |
| 测试文件 | 577 (独立 tests/) | 0 (全内联 `#[cfg(test)]`) | — |
| 技能文件 | 545 (skills/ + optional-skills/) | 1 (skills/mod.rs) | — |
| **核心运行时** | 598 KB (`run_agent.py`) | 138 KB (`agent.rs`) | **~85%** |
| **CLI 命令** | 33 命令处理器 | 33 `*_cmd.rs` | **~90%** |
| **LLM 客户端** | 分散在 `agent/` | 18 模块 + tool_call/ | **~90%** |
| **工具实现** | 75 文件 | ~40 模块 | **~75%** |
| **Gateway 平台** | 24 适配器 | 7 文件(6实现) | **~30%** |
| **Prompt 系统** | 分散在 `agent/` | 10 模块 | **~85%** |
| **配置系统** | `config.py` (复杂) | `hermes-core::config` | **~90%** |

---

## 2. 入口点对齐

| Python 入口 (pyproject.toml) | Rust 入口 | 状态 | 说明 |
|------------------------------|-----------|------|------|
| `hermes_cli.main:main` → `hermes` | `src/main.rs` | **Complete** | 主 CLI 二进制 |
| `run_agent:main` → `hermes-agent` | `src/hermes_agent/main.rs` | **Complete** | 独立 Agent 对话循环 |
| `acp_adapter.entry:main` → `hermes-acp` | `src/hermes_acp/main.rs` | **Partial** | ACP JSON-RPC 服务器骨架存在，功能未全对齐 |
| `mcp_serve.py` | — | **Missing** | 无独立 MCP serve 二进制 |
| `batch_runner.py` | `hermes-batch` crate | **Partial** | 批处理核心存在，分布/轨迹功能完整 |
| `rl_cli.py` | `hermes-rl` crate | **Partial** | 5 个 RL 环境实现，缺少 CLI 包装 |

---

## 3. 核心 Agent 引擎 (`run_agent.py` ↔ `hermes-agent-engine`)

### 3.1 主对话循环

| Python 模块 | Rust 模块 | 状态 | 源码级差距 |
|-------------|-----------|------|-----------|
| `run_agent.py` (598 KB) | `agent.rs` (138 KB) | **Partial** | Rust 已覆盖：预算、中断、工具调用、上下文压缩、会话持久化、子代理委托、流式回调、推理回调、失败转移链。Python 额外有：更复杂的流式分块逻辑、中间助手消息、更丰富的平台钩子 |
| `agent/anthropic_adapter.py` | `hermes-llm::anthropic` | **Complete** | Messages API 转换、缓存控制、thinking 预算检测全对齐 |
| `agent/bedrock_adapter.py` | `hermes-llm::bedrock` | **Complete** | Bedrock 调用适配 |
| `agent/auxiliary_client.py` | `hermes-llm::auxiliary_client` | **Complete** | 辅助模型客户端 |
| `agent/context_engine.py` | `hermes-prompt::context_references` | **Partial** | `@file`/`@folder`/`@git`/`@url`/`@diff` 注入。Python 额外有：更丰富的上下文引用解析器 |
| `agent/context_compressor.py` | `hermes-prompt::context_compressor` | **Complete** | 上下文压缩策略全对齐 |
| `agent/prompt_builder.py` | `hermes-prompt::builder` | **Complete** | 系统提示构建、SOUL.md 加载、平台提示注入、工具使用强制 |
| `agent/prompt_caching.py` | `hermes-prompt::cache_control` | **Complete** | Anthropic 缓存控制 |
| `agent/display.py` | `hermes-cli::display` | **Partial** | 基础显示/输出格式化。Python `display.py` 有更丰富的平台格式化、贴纸缓存、网关特定显示 |
| `agent/retry_utils.py` | `hermes-llm::retry` | **Complete** | 指数退避、重试配置 |
| `agent/error_classifier.py` | `hermes-llm::error_classifier` | **Complete** | 12 类错误分类、ActionHints 全对齐 |
| `agent/rate_limit_tracker.py` | `hermes-llm::rate_limit` | **Complete** | 速率限制跟踪、x-ratelimit 头解析 |
| `agent/nous_rate_guard.py` | — | **Missing** | Nous 专属速率守卫未实现 |
| `agent/smart_model_routing.py` | `hermes-agent-engine::smart_model_routing` | **Partial** | 基础路由逻辑存在。Python 有更丰富的模型能力探测 |
| `agent/usage_pricing.py` | `hermes-agent-engine::usage_pricing` | **Complete** | 价格计算、成本跟踪 |
| `agent/title_generator.py` | `hermes-agent-engine::title_generator` | **Complete** | 会话标题生成 |
| `agent/models_dev.py` | `hermes-llm::models_dev` | **Complete** | 模型元数据、能力查询 |
| `agent/model_metadata.py` | `hermes-llm::model_metadata` | **Complete** | 模型上下文长度、提供商信息 |
| `agent/credential_pool.py` | `hermes-llm::credential_pool` | **Complete** | 凭证池、轮换策略、OAuth 刷新、冷却机制全对齐 |
| `agent/redact.py` | `hermes-core::redact` | **Complete** | PII 脱敏 |
| `agent/subdirectory_hints.py` | `hermes-prompt::subdirectory_hints` | **Complete** | 子目录提示跟踪 |
| `agent/manual_compression_feedback.py` | `hermes-prompt::manual_compression_feedback` | **Complete** | 手动压缩反馈 |
| `agent/trajectory.py` | `hermes-agent-engine::trajectory` | **Partial** | 轨迹记录存在。Python 有更丰富的轨迹分析和可视化 |
| `agent/insights.py` | `hermes-state::insights` | **Complete** | 30 天使用分析 |

### 3.2 Agent 内部子模块 (`agent/`)

| Python | Rust | 状态 | 说明 |
|--------|------|------|------|
| `agent/memory_manager.py` | `hermes-agent-engine::memory_manager` | **Partial** | 内置 memory + 外部 provider 生命周期管理。Python 有更丰富的 provider 发现机制 |
| `agent/memory_provider.py` | `hermes-agent-engine::memory_provider` | **Complete** | `MemoryProvider` trait 完整对标 Python ABC |
| `agent/skill_commands.py` | `hermes-agent-engine::skill_commands` | **Partial** | 技能执行逻辑存在。Python 有更丰富的技能上下文管理 |
| `agent/skill_utils.py` | `hermes-agent-engine::skill_utils` | **Partial** | 技能工具辅助。Python 版本更复杂 |

### 3.3 失败转移链 (Failover)

| Python (`run_agent.py:9350-10127`) | Rust (`failover.rs`) | 状态 | 说明 |
|------------------------------------|----------------------|------|------|
| Unicode 清理 (2 次上限) | `sanitize_unicode_messages()` | **Complete** | 代理字符清理 |
| 错误分类 | `classify_api_error()` | **Complete** | 通过 `error_classifier` |
| 凭证轮换 (402/401/429) | `mark_exhausted_and_rotate()` | **Complete** | Pool 集成到 `agent.rs` 错误处理分支 |
| Provider Auth 刷新 (OAuth) | `try_refresh_current()` | **Complete** | Anthropic/Codex/Nous 401 刷新 |
| Thinking Signature 恢复 | `strip_reasoning_from_messages()` | **Complete** | 移除 reasoning 字段和内联标签 |
| 速率限制 → 立即 fallback | `FailoverAction::TryFallback` | **Complete** | 连续 429 触发 |
| Payload 过大 (413) → 压缩 | `FailoverAction::CompressContext` | **Complete** | |
| 上下文溢出 → 降级 tier | `FailoverAction::ReduceContextTier` | **Complete** | 先降级再压缩 |
| Client Error → fallback → abort | `FailoverAction::Abort` | **Complete** | |
| Max retries → transport 恢复 | `RetryWithBackoff` | **Complete** | 指数退避 |
| Message rollback | `rollback_to_last_assistant()` | **Complete** | 回滚到最后助手回合 |

---

## 4. LLM 客户端层 (`agent/` + `hermes_cli/` ↔ `hermes-llm`)

| Python 模块 | Rust 模块 | 状态 | 源码级差距 |
|-------------|-----------|------|-----------|
| `agent/anthropic_adapter.py` | `anthropic.rs` | **Complete** | Messages API、thinking、tool use、cache control |
| `hermes_cli/codex_models.py` | `codex.rs` | **Complete** | Responses API：message→input items 转换、SSE 流解析、`call_codex_responses_stream()`、tool call 提取 |
| `hermes_cli/runtime_provider.py` | `runtime_provider.rs` | **Complete** | 运行时提供商解析链：custom → explicit → pool → OAuth → Anthropic → API-key → OpenRouter |
| `agent/models_dev.py` | `models_dev.rs` | **Complete** | 模型搜索、能力查询、提供商信息 |
| `agent/model_metadata.py` | `model_metadata.rs` | **Complete** | 上下文长度、功能标志 |
| `agent/pricing.py` | `pricing.rs` | **Complete** | 令牌定价 |
| `hermes_cli/models.py` | `provider.rs` + `model_metadata.rs` | **Partial** | 模型管理分散在多个模块。Python 有更丰富的模型切换逻辑 |
| `hermes_cli/model_switch.py` | `agent::control::switch_model()` | **Partial** | 基础模型切换存在。Python 有更复杂的运行时切换和缓存失效 |
| `agent/copilot_acp_client.py` | — | **Missing** | Copilot ACP 客户端未移植 |
| `agent/error_classifier.py` | `error_classifier.rs` | **Complete** | 401/402/403/429/413/500+ 分类、retryable/fallback/compress/rotate 提示 |
| `agent/rate_limit_tracker.py` | `rate_limit.rs` | **Complete** | 头部解析、令牌桶跟踪 |
| `agent/retry_utils.py` | `retry.rs` | **Complete** | 退避策略 |
| `agent/token_estimate.py` | `token_estimate.rs` | **Complete** | 分词估算 |
| `agent/tool_call/` 目录 | `tool_call/` 目录 | **Complete** | 11 个提供商特定工具调用适配器 (deepseek/glm/kimi/llama/mistral/qwen 等) |
| `hermes_cli/providers.py` | `provider.rs` | **Partial** | 12 个 ProviderType 枚举。Python 有更丰富的提供商配置管理 |

---

## 5. 工具层 (`tools/` ↔ `hermes-tools`)

### 5.1 工具注册与简单工具

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `tools/registry.py` | `registry.rs` | **Complete** | 工具注册表 |
| `tools/budget_config.py` | `budget_config.rs` | **Complete** | 预算配置 |
| `tools/url_safety.py` | `url_safety.rs` | **Complete** | URL 安全检查 |
| `tools/website_policy.py` | `website_policy.rs` | **Complete** | 网站访问策略 |
| `tools/ansi_strip.py` | `ansi_strip.rs` | **Complete** | ANSI 转义序列剥离 |
| `tools/binary_extensions.py` | `binary_extensions.rs` | **Complete** | 二进制文件扩展名 |
| `tools/debug_helpers.py` | `debug_helpers.rs` | **Complete** | 调试辅助 |
| `tools/fuzzy_match.py` | `fuzzy_match.rs` | **Complete** | 模糊匹配 |
| `tools/patch_parser.py` | `patch_parser.rs` | **Complete** | 补丁解析 |
| `tools/osv_check.py` | `osv_check.rs` | **Complete** | OSV 漏洞检查 |
| `tools/clipboard.py` | `clipboard.rs` | **Complete** | 跨平台剪贴板图片提取 (macOS/Windows/Linux/WSL2) |
| `tools/credential_files.py` | `credential_files.rs` | **Complete** | 凭证文件检测 |
| `tools/openrouter_client.py` | `openrouter_client.rs` | **Complete** | OpenRouter 客户端 |
| `tools/transcription.py` | `transcription.rs` | **Complete** | 语音转录 |

### 5.2 复杂工具

| Python 模块 | Rust 模块 | 状态 | 源码级差距 |
|-------------|-----------|------|-----------|
| `tools/approval.py` | `approval.rs` | **Complete** | 审批系统、永久白名单、智能模式 |
| `tools/file_operations.py` + `file_tools.py` | `file_ops.rs` + `shell_file_ops.rs` | **Partial** | 核心文件读写/搜索/补丁存在。Python 有更丰富的文件操作变体 |
| `tools/terminal_tool.py` | `terminal.rs` | **Partial** | 终端工具存在。Python 有 6 个后端 (local/docker/ssh/modal/singularity/daytona)，Rust 有 environments/ 但集成度待确认 |
| `tools/process_reg.py` | `process_reg.rs` | **Complete** | 进程管理 |
| `tools/web_tools.py` | `web.rs` | **Partial** | 网页搜索/提取。Python 支持更多后端 (tavily/exa/serper 等) |
| `tools/browser_tool.py` | `browser/mod.rs` | **Partial** | 浏览器自动化骨架存在 (camofox/resolver/session/providers)。Python 功能更完整 |
| `tools/code_execution_tool.py` | `code_exec/mod.rs` | **Partial** | 代码执行沙箱骨架存在。Python 有更完整的 Python 沙箱 |
| `tools/delegate_tool.py` | `delegate.rs` | **Complete** | 子代理委托 |
| `tools/mcp_tool.py` + `mcp_oauth.py` | `mcp_client/mod.rs` + `mcp_oauth.rs` | **Partial** | MCP 客户端骨架存在 |
| `tools/memory_tool.py` | `memory.rs` | **Complete** | 记忆工具 |
| `tools/todo_tool.py` | `todo.rs` | **Complete** | 待办事项 |
| `tools/skills_tool.py` + `skills_hub.py` + `skills_sync.py` + `skills_guard.py` | `skills.rs` + `skills_hub.rs` + `skills_sync.rs` + `skills_guard.rs` | **Partial** | 技能管理核心存在。Python 技能目录有 545 个文件，Rust 仅 `skills/mod.rs` (108 KB) 一个文件 |
| `tools/tts_tool.py` | `tts.rs` | **Complete** | 文本转语音 |
| `tools/voice_mode.py` | `voice.rs` | **Partial** | 语音模式。Python 有更丰富的语音交互 |
| `tools/vision_tools.py` | `vision.rs` | **Complete** | 视觉分析 |
| `tools/image_generation_tool.py` | `image_gen.rs` | **Complete** | 图像生成 |
| `tools/homeassistant_tool.py` | `homeassistant.rs` | **Complete** | Home Assistant |
| `tools/send_message_tool.py` | `send_message.rs` | **Complete** | 跨平台消息发送 |
| `tools/checkpoint_manager.py` | `checkpoint.rs` | **Complete** | 检查点管理 |
| `tools/cronjob_tools.py` | `cron_tools.rs` | **Complete** | 定时任务工具 |
| `tools/rl_training_tool.py` | `rl_training.rs` | **Partial** | RL 训练工具存在。Python 集成更紧密 |
| `tools/mixture_of_agents_tool.py` | `moa.rs` | **Complete** | 混合智能体 |
| `tools/clarify.py` | `clarify.rs` | **Complete** | 澄清问题 |
| `tools/session_search.py` | `session_search.rs` | **Complete** | 会话搜索 |
| `tools/path_security.py` | `path_security.rs` | **Complete** | 路径安全 |
| `tools/tirith_security.py` | `tirith.rs` | **Complete** | Tirith 安全扫描 |

### 5.3 执行环境 (`tools/environments/`)

| Python 环境 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `local` | `environments/mod.rs` | **Complete** | 本地 shell |
| `docker` | `environments/docker_env.rs` | **Complete** | Docker 后端 |
| `ssh` | `environments/ssh.rs` | **Complete** | SSH 后端 |
| `modal` | `environments/modal.rs` | **Partial** | Modal 后端骨架 |
| `daytona` | `environments/daytona.rs` | **Partial** | Daytona 后端骨架 |
| `singularity` | `environments/singularity.rs` | **Partial** | Singularity 后端骨架 |
| `managed_modal` | — | **Missing** | 托管 Modal 未实现 |

---

## 6. CLI 层 (`hermes_cli/` ↔ `hermes-cli`)

### 6.1 命令模块 (`*_cmd.rs`)

| Python 命令源 | Rust 模块 | 状态 | 说明 |
|---------------|-----------|------|------|
| `main.py: cmd_chat` | `app.rs::run_chat()` | **Partial** | 核心聊天功能在 `app.rs` 中。无独立 `chat_cmd.rs`。Python `cmd_chat` 参数更丰富 |
| `main.py: cmd_gateway` | `gateway_mgmt.rs` | **Partial** | start/stop/status/install/uninstall/restart/setup。缺少统一 `cmd_gateway` 调度器 |
| `main.py: cmd_setup` | `setup_cmd.rs` | **Partial** | 基础 setup 存在。缺少 provider-specific setup (`cmd_setup_provider`) |
| `main.py: cmd_model` | `model_cmd.rs` | **Partial** | 模型切换基础存在 |
| `main.py: cmd_auth` | `auth_cmd.rs` | **Complete** | add/list/remove/reset/status + auth.json 持久化 |
| `main.py: cmd_config` | `config_cmd.rs` | **Complete** | show/edit/set/path/env_path/check/migrate |
| `main.py: cmd_doctor` | `doctor_cmd.rs` | **Complete** | 诊断 + auto-fix |
| `main.py: cmd_dump` | `dump_cmd.rs` | **Complete** | 会话导出 |
| `main.py: cmd_debug` | `debug_cmd.rs` | **Complete** | 调试信息导出 |
| `main.py: cmd_backup` | `backup_cmd.rs` | **Complete** | 备份/恢复/导入 |
| `main.py: cmd_version` | `version_cmd.rs` | **Complete** | 版本显示 |
| `main.py: cmd_uninstall` | `uninstall_cmd.rs` | **Complete** | 卸载 |
| `main.py: cmd_update` | `update_cmd.rs` | **Complete** | 更新检查 |
| `main.py: cmd_profile` | `profiles_cmd.rs` | **Complete** | CRUD + alias/rename/export/import/use |
| `main.py: cmd_dashboard` | `dashboard_cmd.rs` | **Complete** | 仪表板 |
| `main.py: cmd_completion` | `completion_cmd.rs` | **Complete** | Shell 补全 |
| `main.py: cmd_logs` | `logs_cmd.rs` | **Complete** | 日志查看 |
| `main.py: cmd_pairing` | `pairing_cmd.rs` | **Complete** | 设备配对 |
| `main.py: cmd_skills` | `skills_hub_cmd.rs` | **Complete** | list/search/browse/inspect/install/uninstall/check/update/audit/publish/snapshot |
| `main.py: cmd_plugins` | `plugins_cmd.rs` | **Complete** | install/update/remove/list/enable/disable |
| `main.py: cmd_memory` | `memory_cmd.rs` | **Complete** | setup/status/off + provider 配置 |
| `main.py: cmd_tools` | `tools_cmd.rs` | **Complete** | list/disable/enable/batch 操作 |
| `main.py: cmd_mcp` | `mcp_cmd.rs` | **Complete** | list/add/remove/test/configure |
| `main.py: cmd_sessions` | `sessions_cmd.rs` | **Complete** | list/export/delete/search/stats/rename/prune |
| `main.py: cmd_insights` | `insights_cmd.rs` | **Complete** | 使用分析 |
| `main.py: cmd_claw` | `claw_cmd.rs` | **Complete** | Claw 工具 |
| `main.py: cmd_acp` | `acp_cmd.rs` | **Complete** | ACP IDE 集成 |
| `main.py: cmd_whatsapp` | `whatsapp_cmd.rs` | **Complete** | WhatsApp 设置 |
| `main.py: cmd_cron` | `cron_cmd.rs` | **Complete** | list/create/delete/pause/resume/edit/run/status/tick |
| `main.py: cmd_webhook` | `webhook_cmd.rs` | **Complete** | subscribe/list/remove/test |
| `main.py: cmd_status` | `status_cmd.rs` | **Complete** | 状态显示 |
| `main.py: cmd_login` | `login_cmd.rs` | **Complete** | OAuth 设备流登录 |
| `main.py: cmd_logout` | `auth_cmd.rs` | **Complete** | 登出 |
| `batch_runner.py` | `batch_cmd.rs` | **Complete** | 批处理运行/分布/状态 |
| — | `debug_share_cmd.rs` | **Extra** | Rust 独有的调试分享命令 |

### 6.2 UI/UX 组件

| Python 模块 | Rust 模块 | 状态 | 源码级差距 |
|-------------|-----------|------|-----------|
| `hermes_cli/curses_ui.py` | `tui/curses.rs` | **Partial** | Rust 有基础菜单/选择器/确认框 (177 行)。Python curses_ui.py 是完整的 curses 交互式 UI (~2000+ 行功能)，支持实时更新、颜色主题、窗口管理 |
| `hermes_cli/cli_output.py` | `display.rs` | **Partial** | 基础输出格式化。Python 有更丰富的输出模式 |
| `hermes_cli/colors.py` | — | **Missing** | 独立颜色管理模块未移植（功能并入 `console`/`skin_engine`） |
| `hermes_cli/banner.py` | — | **Missing** | 启动横幅未移植 |
| `hermes_cli/skin_engine.py` | `skin_engine.rs` | **Complete** | 6 个内置皮肤 + hex→ANSI256 转换 |
| `hermes_cli/tips.py` | `tips.rs` | **Complete** | 80+ 提示语料库 |
| `hermes_cli/default_soul.py` | `hermes-prompt::soul` | **Complete** | SOUL.md 加载 + 默认身份 |
| `hermes_cli/clipboard.py` | `clipboard.rs` | **Complete** | 跨平台剪贴板图片提取 |

### 6.3 配置与认证

| Python 模块 | Rust 模块 | 状态 | 源码级差距 |
|-------------|-----------|------|-----------|
| `hermes_cli/config.py` | `hermes-core::config` | **Complete** | YAML 配置、_config_version=22、22 步迁移链、${VAR} 环境变量展开、provider preferences、credential pool strategies |
| `hermes_cli/env_loader.py` | `hermes-core::env_loader` | **Complete** | .env 文件加载 |
| `hermes_cli/auth.py` + `auth_commands.py` | `auth_cmd.rs` | **Complete** | auth.json 持久化、provider state、Codex tokens、凭证池读写 |
| `hermes_cli/copilot_auth.py` | — | **Missing** | Copilot 认证未移植 |
| `hermes_cli/nous_subscription.py` | — | **Missing** | Nous 订阅功能状态检测未独立移植。相关功能碎片在 `managed_tool_gateway.rs` 和 `tool_backend_helpers.rs` |
| `hermes_cli/memory_setup.py` | `memory_cmd.rs` | **Partial** | 基础 setup 存在。Python 有更丰富的 provider-specific 配置向导 |
| `hermes_cli/mcp_config.py` | `mcp_cmd.rs` | **Complete** | MCP 服务器管理 |
| `hermes_cli/plugins.py` + `plugins_cmd.py` | `plugins_cmd.rs` | **Complete** | 插件管理 |
| `hermes_cli/profiles.py` | `profiles_cmd.rs` | **Complete** | 配置文件管理 |
| `hermes_cli/providers.py` | `provider.rs` | **Partial** | 基础 provider 枚举。Python 有更丰富的 provider 元数据 |
| `hermes_cli/runtime_provider.py` | `runtime_provider.rs` | **Complete** | 运行时 provider 解析链 |
| `hermes_cli/gateway.py` | `gateway_mgmt.rs` | **Partial** | 基础网关管理。Python 有更丰富的网关配置 |
| `hermes_cli/webhook.py` | `webhook_cmd.rs` | **Complete** | Webhook 订阅管理 |
| `hermes_cli/tools_config.py` | `tools_cmd.rs` | **Complete** | 工具配置 |
| `hermes_cli/skills_config.py` + `skills_hub.py` | `skills_hub_cmd.rs` | **Complete** | 技能配置管理 |

### 6.4 诊断与维护

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `hermes_cli/doctor.py` | `doctor_cmd.rs` | **Complete** | 诊断 + 自动修复 |
| `hermes_cli/debug.py` | `debug_cmd.rs` | **Complete** | 调试信息 |
| `hermes_cli/dump.py` | `dump_cmd.rs` | **Complete** | 会话导出 |
| `hermes_cli/logs.py` | `logs_cmd.rs` | **Complete** | 日志管理 |
| `hermes_cli/backup.py` | `backup_cmd.rs` | **Complete** | 备份恢复 |
| `hermes_cli/uninstall.py` | `uninstall_cmd.rs` | **Complete** | 卸载 |
| `hermes_cli/completion.py` | `completion_cmd.rs` | **Complete** | Shell 补全生成 |
| `hermes_cli/status.py` | `status_cmd.rs` | **Complete** | 状态显示 |

---

## 7. Prompt 系统 (`agent/` ↔ `hermes-prompt`)

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `agent/prompt_builder.py` | `builder.rs` | **Complete** | 系统提示构建、工具提示、平台提示、技能索引、SOUL.md |
| `agent/context_compressor.py` | `context_compressor.rs` | **Complete** | 上下文压缩引擎 |
| `agent/context_references.py` | `context_references.rs` | **Complete** | @file/@folder/@git/@url/@diff 引用 |
| `agent/prompt_caching.py` | `cache_control.rs` | **Complete** | 缓存控制标记 |
| `agent/subdirectory_hints.py` | `subdirectory_hints.rs` | **Complete** | 子目录提示 |
| `agent/injection_scan.py` | `injection_scan.rs` | **Complete** | 提示注入扫描 |
| `agent/manual_compression_feedback.py` | `manual_compression_feedback.rs` | **Complete** | 手动压缩反馈 |
| `agent/soul.py` (`default_soul.py`) | `soul.rs` | **Complete** | SOUL.md 加载、默认身份、截断 |
| `agent/skills_prompt.py` | `skills_prompt.rs` | **Complete** | 技能提示格式化 |

---

## 8. Gateway 层 (`gateway/` ↔ `hermes-gateway`)

### 8.1 核心网关

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `gateway/run.py` | `runner.rs` | **Partial** | 网关主循环存在。Python 支持 19 个平台，Rust 仅 6 个有实现 |
| `gateway/session.py` | `session.rs` | **Partial** | 会话管理存在。Python 功能更丰富 |
| `gateway/delivery.py` | — | **Missing** | 消息投递层未独立实现 |
| `gateway/channel_directory.py` | — | **Missing** | 频道目录未实现 |
| `gateway/mirror.py` | — | **Missing** | 消息镜像未实现 |
| `gateway/pairing.py` | `pairing_cmd.rs` | **Partial** | 配对命令存在，网关内配对逻辑未完全对齐 |
| `gateway/restart.py` | — | **Missing** | 网关重启逻辑未实现 |
| `gateway/status.py` | `gateway_mgmt.rs` | **Partial** | 基础状态查询 |
| `gateway/stream_consumer.py` | `stream_consumer.rs` | **Partial** | 流消费者骨架存在 |
| `gateway/sticker_cache.py` | — | **Missing** | 贴纸缓存未实现 |
| `gateway/config.py` | `config.rs` | **Partial** | 网关配置存在 |
| `gateway/hooks.py` + `builtin_hooks/` | — | **Missing** | Webhook 钩子系统未实现 |
| `gateway/display_config.py` | — | **Missing** | 网关显示配置未独立实现 |

### 8.2 平台适配器

| Python 平台 | Rust 平台 | 状态 | 源码级差距 |
|-------------|-----------|------|-----------|
| `telegram.py` + `telegram_network.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `discord.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `slack.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `matrix.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `mattermost.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `signal.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `whatsapp.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `sms.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `email.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `webhook.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `homeassistant.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `bluebubbles.py` | — | **Missing** | 配置 enum 存在，无适配器实现 |
| `qqbot.py` | — | **Missing** | 无配置 enum，无适配器 |
| `feishu.py` | `feishu.rs` + `feishu_ws.rs` | **Partial** | 核心 webhook + WS 实现。Python 有更丰富的富媒体处理 |
| `dingtalk.py` | `dingtalk.rs` | **Partial** | Webhook 接收器 + 会话缓存。功能基本对齐 |
| `wecom.py` | `wecom.rs` | **Partial** | WebSocket 连接 + DM/群聊 + 去重。Python 有回调模式分离 |
| `wecom_callback.py` | — | **Missing** | WeCom 回调模式独立适配器未实现 |
| `wecom_crypto.py` | — | **Missing** | WeCom 加密工具未实现 |
| `weixin.py` | `weixin.rs` | **Partial** | 长轮询 + 消息发送。Python 有更丰富的媒体 CDN 和 QR 登录 |
| `api_server.py` | `api_server.rs` | **Partial** | OpenAI 兼容 API 服务器。功能基本对齐 |
| `base.py` | — | **Missing** | 基础适配器类无直接对应，功能分散在 runner/session |
| `helpers.py` | — | **Missing** | 共享助手函数未独立移植 |

---

## 9. 状态与持久化 (`hermes_state.py` ↔ `hermes-state`)

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `hermes_state.py` (51 KB) | `session_db.rs` (59 KB) | **Complete** | SQLite 会话存储、FTS5 搜索、消息 CRUD、会话列表/导出/删除 |
| `agent/insights.py` | `insights.rs` (37 KB) | **Complete** | 30 天使用分析、令牌统计、成本分析 |
| `hermes-state::models.rs` | — | **Complete** | 数据模型定义 |
| `hermes-state::schema.rs` | — | **Complete** | 数据库 schema |

---

## 10. 批处理与压缩 (`batch_runner.py` + `trajectory_compressor.py` ↔ `hermes-batch` + `hermes-compress`)

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `batch_runner.py` (57 KB) | `hermes-batch` crate | **Partial** | 批处理运行器、分布采样、检查点、轨迹。Python 功能更完整 |
| `trajectory_compressor.py` (65 KB) | `hermes-compress` crate | **Partial** | 压缩引擎 + 摘要器。Python 有更丰富的压缩策略 |
| `toolset_distributions.py` (13 KB) | `distributions.rs` | **Complete** | 工具集分布采样 |

---

## 11. Cron 调度 (`cron/` ↔ `hermes-cron`)

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `cron/scheduler.py` | `scheduler.rs` | **Complete** | 定时调度引擎 |
| `cron/jobs.py` | `jobs.rs` | **Complete** | 任务定义 |
| `cron/__init__.py` | `lib.rs` | **Complete** | Crate 根 |
| `cron/delivery.py` (gateway 集成) | `delivery.rs` | **Complete** | 任务投递到 Agent |

---

## 12. ACP 适配器 (`acp_adapter/` ↔ `hermes-acp` + `src/hermes_acp/`)

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `acp_adapter/entry.py` | `src/hermes_acp/main.rs` | **Partial** | 入口点存在 |
| `acp_adapter/server.py` | `src/hermes_acp/server.rs` | **Partial** | ACP JSON-RPC 服务器骨架存在 |
| `acp_adapter/session.py` | `src/hermes_acp/session.rs` | **Partial** | 会话管理 |
| `acp_adapter/tools.py` | — | **Missing** | ACP 工具暴露未完全对齐 |
| `acp_adapter/events.py` | — | **Missing** | 事件系统未完全对齐 |
| `acp_adapter/auth.py` | — | **Missing** | ACP 认证未完全对齐 |
| `acp_adapter/permissions.py` | — | **Missing** | 权限系统未完全对齐 |

---

## 13. RL 训练 (`rl_cli.py` + `environments/` ↔ `hermes-rl`)

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `rl_cli.py` (17 KB) | — | **Missing** | 无独立 RL CLI 二进制 |
| `environments/` (43 文件) | `hermes-rl` crate | **Partial** | 5 个环境：atropos、math、tool_use、web_research、base。Python 环境配置更丰富 |

---

## 14. 技能系统 (`skills/` ↔ `hermes-tools::skills`)

| Python | Rust | 状态 | 说明 |
|--------|------|------|------|
| `skills/` (407 文件) | `skills/mod.rs` (108 KB) | **Partial** | Rust 将所有技能内联在一个模块。Python 是文件系统级别的技能库，按目录组织 |
| `optional-skills/` (138 文件) | — | **Missing** | 可选技能未移植 |
| `tools/skills_hub.py` | `skills_hub.rs` | **Complete** | 技能中心管理 |
| `tools/skills_sync.py` | `skills_sync.rs` | **Complete** | 技能同步 |
| `tools/skills_guard.py` | `skills_guard.rs` | **Complete** | 技能安全检查 |

---

## 15. 测试覆盖 (`tests/` ↔ 内联 `#[cfg(test)]`)

| Python | Rust | 状态 | 说明 |
|--------|------|------|------|
| 577 独立测试文件 | 0 独立测试文件 | **Architecture Gap** | Rust 采用内联单元测试。按 crate 统计：hermes-core(8)、hermes-llm(大量)、hermes-tools(663+)、hermes-prompt(77)、hermes-agent-engine(255)、hermes-cli(120+)、hermes-batch(39) 等。总测试数约 2000+ |
| `tests/integration/` | — | **Missing** | 无独立集成测试目录 |
| `tests/e2e/` | `scripts/e2e_test.sh` | **Partial** | E2E 测试脚本存在，覆盖度不及 Python |

---

## 16. 杂项模块

| Python 模块 | Rust 模块 | 状态 | 说明 |
|-------------|-----------|------|------|
| `hermes_constants.py` | `hermes-core::constants` | **Complete** | 常量定义 |
| `hermes_logging.py` | `hermes-core::logging` | **Complete** | 日志设置 |
| `hermes_time.py` | `hermes-core::time` | **Complete** | 时间工具 |
| `utils.py` | 分散在各 crate | **Complete** | 通用工具函数 |
| `mcp_serve.py` | — | **Missing** | 独立 MCP 服务器二进制 |
| `mini_swe_runner.py` | — | **Missing** | Mini SWE 运行器 |
| `cli.py` (458 KB, legacy) | — | **N/A** | 旧版 CLI，Rust 直接替代 |

---

## 17. 源码级关键差距详单

### 17.1 完全缺失的模块

| 模块 | Python 位置 | 影响 | 优先级 |
|------|-------------|------|--------|
| Nous Subscription 管理 | `hermes_cli/nous_subscription.py` | 订阅状态检测、功能开关 | 中 |
| Copilot Auth | `hermes_cli/copilot_auth.py` | Copilot 认证集成 | 低 |
| Curses UI 完整实现 | `hermes_cli/curses_ui.py` | 交互式 curses 菜单 | 中 |
| 旧版 CLI (`cli.py`) | `cli.py` | 已被 Rust CLI 替代 | N/A |
| Mini SWE Runner | `mini_swe_runner.py` | 软件工程任务运行 | 低 |
| MCP Serve 二进制 | `mcp_serve.py` | MCP 服务器模式 | 低 |
| QQ Bot 适配器 | `gateway/platforms/qqbot.py` | 国内平台 | 低 |
| Gateway 基础类 | `gateway/platforms/base.py` | 抽象基类 | 中 |
| Gateway 共享助手 | `gateway/platforms/helpers.py` | 共享工具函数 | 中 |
| WeCom 回调适配器 | `gateway/platforms/wecom_callback.py` | 企业微信回调模式 | 低 |
| WeCom 加密 | `gateway/platforms/wecom_crypto.py` | 消息加解密 | 低 |
| 消息镜像 | `gateway/mirror.py` | 跨平台消息镜像 | 低 |
| 贴纸缓存 | `gateway/sticker_cache.py` | 贴纸管理 | 低 |
| Webhook 钩子系统 | `gateway/hooks.py` + `builtin_hooks/` | 可扩展钩子 | 低 |
| Nous Rate Guard | `agent/nous_rate_guard.py` | Nous 速率保护 | 低 |
| 可选技能库 | `optional-skills/` (138 文件) | 扩展技能 | 低 |

### 17.2 部分实现的模块（需要深化）

| 模块 | 差距描述 |
|------|---------|
| **Gateway 国内平台** | Feishu/DingTalk/WeCom/Weixin 已基础实现，但缺少富媒体处理、高级群组策略、完整的错误恢复 |
| **Gateway 海外平台** | Telegram/Discord/Slack/Matrix/Mattermost/Signal/WhatsApp/Email/SMS/Webhook/HA/BlueBubbles 仅有配置 enum，无适配器实现 |
| **Browser 工具** | 骨架存在 (camofox/resolver/session/providers)，但功能未完全展开 |
| **Code Execution** | 沙箱模块骨架存在，Python 沙箱执行逻辑未完全对齐 |
| **TUI/Curses** | 基础菜单存在，缺少完整的 curses 窗口管理、实时更新、颜色主题应用 |
| **Skills 文件系统** | Rust 将所有技能内联在 `skills/mod.rs`，Python 是动态文件系统加载 |
| **Voice TUI** | `voice_tui.rs` 骨架存在，功能待确认 |
| **MCP Client** | 模块骨架存在，完整协议实现待确认 |
| **ACP Server** | 二进制和服务器骨架存在，完整 ACP 协议实现待深化 |
| **批处理** | 核心存在，分布和轨迹分析功能较 Python 简化 |
| **RL 环境** | 5 个环境实现，缺少 CLI 包装器和环境配置系统 |
| **Web 搜索后端** | 基础实现，Python 支持更多搜索后端 (tavily/exa/serper) |

---

## 18. 结论

### 18.1 已完成对齐的核心领域

1. **Agent 对话循环** — 预算管理、工具调用、上下文压缩、失败转移链、子代理委托、流式/推理回调
2. **LLM 客户端** — Anthropic/OpenAI/Codex/Bedrock 适配、凭证池、错误分类、重试、运行时 Provider 解析
3. **Prompt 系统** — 系统提示构建、上下文引用、缓存控制、注入扫描、SOUL.md
4. **配置系统** — 22 版本迁移链、环境变量展开、Provider 偏好、凭证池策略
5. **CLI 命令** — 33 个命令模块，覆盖诊断/备份/技能/会话/定时任务/网关管理等
6. **工具注册表** — 40+ 工具模块，覆盖文件/终端/浏览器/代码执行/记忆/语音/视觉等
7. **状态持久化** — SQLite 会话存储、FTS5 搜索、使用分析
8. **Cron 调度** — 完整的定时任务系统

### 18.2 剩余的主要差距

1. **Gateway 海外平台适配器** — 12 个平台仅有配置 enum，需要逐个实现适配器
2. **Curses TUI 完整实现** — 当前只有基础菜单，需要完整的窗口管理和实时更新
3. **Nous Subscription 管理** — 订阅状态检测和功能开关未独立实现
4. **Skills 文件系统** — 当前内联实现，需要支持动态文件系统加载
5. **测试架构** — Rust 采用内联测试，需要评估是否需要独立集成测试目录
6. **ACP 完整协议** — 骨架存在，需要深化工具暴露/事件/认证/权限
7. **Browser/CodeExec/MCP 模块深化** — 骨架存在，需要功能展开

### 18.3 总体评估

- **核心功能对齐度：~85%**（Agent 引擎、LLM 客户端、Prompt、配置、CLI 命令）
- **工具层对齐度：~75%**（核心工具完整，复杂工具模块为骨架状态）
- **Gateway 对齐度：~30%**（6/19 平台有实现，且多为 Partial）
- **测试对齐度：~60%**（内联单元测试丰富，但缺少独立集成/E2E 测试）
- **技能系统对齐度：~40%**（功能存在但架构不同：内联 vs 文件系统）

> **注**：Gateway 海外平台（Telegram/Discord/Slack 等）根据用户之前的指示不在当前对齐范围内。国内平台（Feishu/DingTalk/WeCom/Weixin）已有基础实现，需要继续深化。
