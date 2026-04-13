# Hermes Agent — Python vs Rust 移植版本核心差异对比

> 分析日期: 2026-04-13  
> 更新日期: 2026-04-13 (自我进化机制 + Fuzzy Match + 可靠性改进已补齐)  
> Python 版本: `run_agent.py` (530KB, ~10524 行) + 工具目录 (~60 个文件)  
> Rust 版本: `hermes-rs/` (11 个 crate, ~50 个源文件, Cargo.toml workspace)

---

## 一、架构对比：巨石 vs 模块化

### Python: 单文件巨石

| 文件 | 行数 | 职责 |
|------|------|------|
| `run_agent.py` | ~10524 行 (530KB) | **一切**：AIAgent 类、对话循环、系统 prompt 构建、工具执行、API 调用、重试、回退、压缩、子 agent、后台 review、标题生成、轨迹保存 |
| `model_tools.py` | 工具注册 + 分发 |
| `cli.py` | 交互式 CLI 界面 |
| `hermes_state.py` | SQLite 会话存储 |
| `agent/` | prompt 构建、压缩、子 agent、缓存、显示 |
| `tools/` | ~60 个工具实现 |

**问题**: `run_agent.py` 单个文件 530KB，包含所有核心逻辑。修改任意功能都需要在这个巨型文件中导航。

### Rust: 11 个 Crate 的职责分离

```
hermes-rs/
├── crates/
│   ├── hermes-core/          # 配置、错误、Hermes Home、Result 类型
│   ├── hermes-state/         # SQLite 会话存储 (rusqlite)
│   ├── hermes-llm/           # LLM API 客户端 (async-openai, reqwest)
│   ├── hermes-tools/         # 工具实现 (file_ops, memory, skills, web, terminal...)
│   ├── hermes-prompt/        # 系统 prompt 构建、压缩、缓存控制
│   ├── hermes-agent-engine/  # AIAgent 核心对话循环
│   ├── hermes-cli/           # CLI 界面 (clap, reedline)
│   ├── hermes-cron/          # 定时任务调度
│   ├── hermes-batch/         # 批量轨迹生成
│   ├── hermes-compress/      # 上下文压缩
│   └── hermes-gateway/       # 消息平台网关
└── binaries/
    ├── hermes/               # 主 CLI 入口
    ├── hermes-agent/         # Agent 对话入口
    └── hermes-acp/           # IDE 集成入口
```

**优势**:
- 每个 crate 有明确的职责边界和独立测试
- `hermes-agent-engine` (10 个文件, ~1305 行 agent.rs) vs Python `run_agent.py` (10524 行)
- 编译期类型安全，工具注册通过 trait 约束
- 依赖边界清晰：`hermes-tools` 不依赖 `hermes-llm`，避免循环导入

---

## 二、自我改进机制对比

### Python: 完整实现

| 组件 | 实现状态 | 位置 |
|------|----------|------|
| Skill 系统 | ✅ 完整 | `tools/skill_manager_tool.py` (761 行) |
| Fuzzy Match | ✅ 完整 | `tools/fuzzy_match.py` (566 行, 9 级匹配链) |
| Nudge 计数器 | ✅ 完整 | `run_agent.py:1093-1196` |
| 后台 Review Agent | ✅ 完整 | `run_agent.py:2089-2168` |
| Skills Guard | ✅ 完整 | `tools/skills_guard.py` (977 行) |
| Memory 系统 | ✅ 完整 | `tools/memory_tool.py` (560 行) |

**完整闭环**:
```
计数器递增 → 阈值触发 → _spawn_background_review() → 
创建独立 Agent 副本 → 注入 review prompt → 
调用 skill_manage create/patch → 写入磁盘 → 
下次对话自动出现在 system prompt 中
```

### Rust: **已补齐**后台 Review + Nudge 机制

| 组件 | 实现状态 | 差异说明 |
|------|----------|----------|
| Skill 系统 | ✅ 已移植 | `hermes-tools/src/skills/mod.rs` (106KB, 2267 行) — 包含 create/edit/patch/delete/view |
| Fuzzy Match | ✅ 完整 | `hermes-tools/src/fuzzy_match.rs` — 已移植 9 级降级匹配链 |
| Nudge 计数器 | ✅ 已实现 | `AIAgent` struct 包含 `turns_since_memory`、`iters_since_skill` |
| 后台 Review Agent | ✅ 已实现 | `review_agent.rs` — `tokio::spawn` fire-and-forget 模式 |
| Skills Guard | ✅ 已移植 | `hermes-tools/src/skills_guard.rs` (17.6KB) — 信任分级 + 威胁模式扫描 |
| Memory 系统 | ✅ 已移植 | `hermes-tools/src/memory.rs` (757 行) — MEMORY.md + USER.md |
| Patch Parser | ✅ 已移植 | `hermes-tools/src/patch_parser.rs` (537 行) — V4A patch 格式 |

**完整闭环 (已对齐 Python)**:
```
计数器递增 → 阈值触发 → spawn_background_review() → 
创建独立 Agent 副本 → 注入 review prompt → 
调用 skill_manage create/patch → 写入磁盘 → 
下次对话自动出现在 system prompt 中
```

### 额外已对齐的 Python 可靠性改进

| 组件 | 实现状态 | 说明 |
|------|----------|------|
| Stale-call timeout | ✅ 已实现 | 默认 300s，>100K tokens → 600s，本地端点禁用 |
| HTTP 错误码诊断 | ✅ 已实现 | 429/402/500/504/524/503 各出具体失败提示 |
| Provider 默认模型回退 | ✅ 已实现 | 无 model 配置时使用 provider 默认模型 |
| `_disable_streaming` 标记 | ✅ 预留 | Provider 不支持流式时自动切换非流式 |

---

## 三、对话循环实现对比

### Python: 同步阻塞式

```python
# run_agent.py — 单线程同步执行
def run_conversation(self, user_message, conversation_history=None):
    messages = conversation_history or []
    # 系统 prompt 构建
    system_prompt = self._build_system_prompt()
    # 主循环：同步 API 调用 + 工具执行
    while not self.iteration_budget.exhausted():
        response = self.client.chat.completions.create(...)  # 阻塞
        tool_calls = response.choices[0].message.tool_calls
        for tool_call in tool_calls:
            result = self.execute_tool_call(tool_call)  # 同步
            messages.append(result)
    return {"final_response": ..., "messages": messages}
```

### Rust: 原生异步

```rust
// hermes-agent-engine/src/agent.rs — 全异步
pub async fn run_conversation(
    &mut self,
    user_message: &str,
    system_message: Option<&str>,
    conversation_history: Option<&[Value]>,
) -> TurnResult {
    // 系统 prompt 构建
    let active_system_prompt = self.build_system_prompt(system_message);
    // 主循环：异步 API 调用 + 工具执行
    while !self.budget.exhausted() {
        let response = self.client.chat(&messages).await;  // 异步
        let tool_calls = response.tool_calls();
        for tool_call in tool_calls {
            let result = self.execute_tool_call(&tool_call).await;  // 异步
            messages.push(result);
        }
    }
    TurnResult { response, messages, api_calls, exit_reason }
}
```

### 对比

| 维度 | Python | Rust |
|------|--------|------|
| 并发模型 | 同步阻塞 (线程池包装) | 原生 async/await |
| API 调用 | `openai` 库同步调用 | `async-openai` 异步流 |
| 工具执行 | 同步函数调用 | `async fn` + `tokio::spawn` |
| 子 Agent | `threading.Thread` (操作系统线程) | `tokio::task::JoinSet` (轻量任务) |
| 循环阻断 | 阻塞整个事件循环 | 非阻塞，可并发处理多个会话 |

---

## 四、Fuzzy Match 对比

### Python: 9 级降级匹配链

```python
# tools/fuzzy_match.py:73-83
strategies = [
    ("exact", _strategy_exact),                    # 精确匹配
    ("line_trimmed", _strategy_line_trimmed),       # 行级裁剪空白
    ("whitespace_normalized", ...),                 # 多空格折叠
    ("indentation_flexible", ...),                  # 忽略缩进差异
    ("escape_normalized", ...),                     # \n 字面量转换行
    ("trimmed_boundary", ...),                      # 首尾行空白裁剪
    ("unicode_normalized", ...),                    # Unicode 标准化
    ("block_anchor", ...),                          # 首尾锚定 + 中间相似度
    ("context_aware", ...),                         # 50% 行相似度阈值
]
```

**优势**: 专门为代码修补设计的 9 级降级策略，针对 LLM 生成的补丁格式做了深度优化。

### Rust: **已移植** 9 级降级匹配链

```rust
// hermes-tools/src/fuzzy_match.rs — 已按 Python 顺序实现 9 级链:
pub fn fuzzy_find_and_replace(content: &str, old: &str, new: &str) -> (String, usize, &'static str) {
    // 1. exact
    // 2. line_trimmed
    // 3. whitespace_normalized
    // 4. indentation_flexible
    // 5. escape_normalized
    // 6. trimmed_boundary
    // 7. unicode_normalized
    // 8. block_anchor
    // 9. context_aware
    // 每个策略返回 (新内容, 匹配数, 策略名)，匹配数 > 0 时短路返回
}
```

**已补齐**: 所有 9 个策略函数，使用早期返回模式短路，策略顺序与 Python 一致。

---

## 五、Memory 系统对比

### Python

```python
# tools/memory_tool.py
class MemoryStore:
    def __init__(self, memory_char_limit=2200, user_char_limit=1375):
        self.memory_entries = []
        self.user_entries = []
        self.system_prompt_snapshot = ""
    
    def add(self, target, content):
        # 添加条目到 MEMORY.md 或 USER.md
        # 带安全扫描（prompt injection 检测）
```

### Rust

```rust
// hermes-tools/src/memory.rs
pub struct MemoryStore {
    pub memory_entries: Vec<String>,
    pub user_entries: Vec<String>,
    pub system_prompt_snapshot: String,
    skip_auto_load: bool,
}

impl MemoryStore {
    pub fn add(&mut self, target: &str, content: &str) -> Result<String> {
        // 安全扫描 + 字符限制 + 持久化
    }
}
```

**差异**: 基本一致。Rust 版本使用 `Vec<String>` 代替 Python 的列表，类型安全更好。安全扫描在 Rust 中使用简单的字符串匹配而不是 Python 的正则表达式。

---

## 六、类型安全对比

### Python: 动态类型

```python
def run_conversation(self, user_message, conversation_history=None):
    messages = conversation_history or []  # List[Dict]，无类型约束
    response = self.client.chat.completions.create(...)  # 返回类型依赖运行时
    tool_calls = response.choices[0].message.tool_calls  # 可能为 None，运行时崩溃
```

### Rust: 编译期类型安全

```rust
pub struct TurnResult {
    pub response: String,       // 编译期保证
    pub messages: Vec<Value>,   // 不是 Option，必定存在
    pub api_calls: usize,
    pub exit_reason: String,    // 所有分支必须设置
}
```

---

## 七、Skill Patch 修补机制对比

### Python: `skill_manage` 工具直接集成

```python
# tools/skill_manager_tool.py
def _patch_skill(name, old_string, new_string, file_path=None, replace_all=False):
    # 1. 查找 skill
    existing = _find_skill(name)
    # 2. fuzzy_find_and_replace (9 级匹配链)
    new_content, match_count, strategy, match_error = fuzzy_find_and_replace(...)
    # 3. 验证 frontmatter
    # 4. 原子写入
    # 5. 安全扫描
    # 6. 失败回滚
```

### Rust: `handle_skill_manage` 函数

```rust
// hermes-tools/src/skills/mod.rs:1211
pub fn handle_skill_manage(args: Value) -> Result<String, HermesError> {
    let action = args["action"].as_str().ok_or(...)?;
    match action {
        "create" => handle_skill_create(args),
        "edit" => handle_skill_edit(args),
        "patch" => handle_skill_patch(args),  // 使用 patch_parser
        "delete" => handle_skill_delete(args),
        "write_file" => handle_skill_write_file(args),
        "remove_file" => handle_skill_remove_file(args),
        _ => tool_error(format!("Unknown action: {}", action)),
    }
}
```

**差异**: Rust 版本的 patch 使用的是 V4A patch 格式 (`patch_parser.rs`)，而 Python 使用的是简单的 `old_string` / `new_string` 模糊匹配。V4A 格式更强大（支持多文件、hunk、上下文），但 LLM 生成 V4A 格式的要求更高。

---

## 八、功能完成度总结

| 功能模块 | Python | Rust | 备注 |
|----------|--------|------|------|
| **核心对话循环** | ✅ 完整 | ✅ 完整 | Rust 版本为异步实现 |
| **Tool Registry** | ✅ 完整 | ✅ 完整 | Rust 使用 trait 约束 |
| **Skill 系统** | ✅ 完整 | ✅ 完整 | 106KB，已包含所有 CRUD 操作 |
| **Fuzzy Match (代码修补)** | ✅ 9 级链 | ✅ 9 级链 | 已完整移植，顺序匹配+短路 |
| **Skill Patch (V4A)** | ✅ 有 | ✅ 有 | `patch_parser.rs` 已移植 |
| **Memory 系统** | ✅ 完整 | ✅ 完整 | MEMORY.md + USER.md |
| **Memory Manager** | ✅ 完整 | ✅ 完整 | `memory_manager.rs` |
| **Memory Provider** | ✅ 插件 | ✅ trait | Rust 用 trait 抽象 |
| **Skills Guard** | ✅ 完整 | ✅ 完整 | 信任分级 + 威胁扫描 |
| **Session Search** | ✅ FTS5 | ✅ 已移植 | `session_search.rs` |
| **Context Compression** | ✅ 完整 | ✅ 已移植 | `context_compressor.rs` |
| **Prompt Caching** | ✅ Anthropic | ✅ 已移植 | `cache_control.rs` |
| **Subagent Delegation** | ✅ 完整 | ✅ 完整 | `subagent.rs` |
| **Model Routing** | ✅ 有 | ✅ 完整 | `smart_model_routing.rs` |
| **Trajectory** | ✅ 保存 | ✅ 已移植 | `trajectory.rs` |
| **Title Generation** | ✅ 有 | ✅ 已移植 | `title_generator.rs` |
| **Budget** | ✅ 有 | ✅ 已移植 | `budget.rs` |
| **Cron** | ✅ 完整 | ✅ 已移植 | `hermes-cron` |
| **Gateway** | ✅ 15+ 平台 | ⚠️ 部分 | `hermes-gateway` |
| **Nudge 计数器** | ✅ 完整 | ✅ 已实现 | `turns_since_memory` / `iters_since_skill` |
| **后台 Review Agent** | ✅ 完整 | ✅ 已实现 | `review_agent.rs` — fire-and-forget |
| **Stale-call Timeout** | ✅ 有 | ✅ 已实现 | 300s 默认，按 token 缩放 |
| **HTTP 错误码诊断** | ✅ 有 | ✅ 已实现 | 429/402/500/504/524/503 提示 |
| **自我改进闭环** | ✅ 完整 | ✅ 完整 | 已对齐 Python |

---

## 九、核心结论

### Rust 版本移植了什么

1. **完整的 Skill 工具系统** — create/edit/patch/delete/write_file/remove_file 全部实现
2. **完整的 Memory 系统** — 包括安全扫描和字符限制
3. **完整的 Subagent 系统** — 包括 `dispatch_delegation` 异步循环破解
4. **完整的安全机制** — Skills Guard + Memory 安全扫描
5. **异步架构** — 整个对话循环使用 async/await + tokio
6. **自我改进闭环** — nudge 计数器 + 后台 review agent，已对齐 Python
7. **Fuzzy Match 9 级降级链** — 全部移植，早期返回模式短路
8. **可靠性改进** — stale-call timeout、HTTP 错误码诊断、provider 默认模型回退

### Rust 版本已完成对齐（截至 2026-04-13）

所有核心差异已补齐：

- ✅ **Nudge 计数器** — `turns_since_memory`、`iters_since_skill` 已实现
- ✅ **后台 Review Agent** — `review_agent.rs` fire-and-forget 模式
- ✅ **Review Prompts** — `self_evolution.rs` 包含 3 个 review prompt
- ✅ **Fuzzy Match 降级链** — 9 级匹配链完整移植
- ✅ **Stale-call Timeout** — 300s 默认，按 token 缩放
- ✅ **HTTP 错误码诊断** — 429/402/500/504/524/503 具体失败提示
- ✅ **Provider 默认模型回退** — 无 model 配置时自动 fallback

### Rust 版本仍需关注的部分

- ⚠️ **Gateway** — Python 支持 15+ 平台，Rust gateway 目前仅实现部分平台适配器
- ⚠️ **Web Dashboard** — Python 有 React + Vite 前端，Rust 版本暂无对应 Web UI

### 测试状态

- 0 clippy warnings
- 888+ 单元测试通过
