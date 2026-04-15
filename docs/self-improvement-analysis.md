# Hermes Agent 自我改进 — 为什么有效（源码分析）

> 分析日期: 2026-04-13  
> 所有结论均来自对实际源码的验证

---

## 问题

Hermes Agent 声称有"自我改进"能力。但它是如何做到的？不是后台有个训练循环在调参——它是一个纯推理系统，没有模型权重的更新。那"改进"体现在哪里？

**答案**: 改进不在模型内部，而在**模型可用的上下文**。它通过修改自己的 prompt（system prompt + tool definitions）来"进化"。

---

## 一、Skill 系统 — 为什么它是有效的知识载体

### 1.1 结构化的程序性记忆

```python
# tools/skill_manager_tool.py:7-12
"""
Skill 是 agent 的程序性记忆：它们基于已验证的经验，捕获*如何做特定类型任务*。
通用记忆（MEMORY.md, USER.md）是宽泛且声明式的。Skill 是窄而可操作的。
"""
```

**为什么有效**: LLM 不是缺乏能力，而是缺乏**上下文**。当 Agent 第一次处理某个任务时，它从零开始推理；但一旦把成功模式固化为 Skill，下次同样的任务就变成了"照着做"。这本质上是一种**prompt 级别的缓存**。

### 1.2 每次对话都出现在 system prompt 中

```python
# agent/prompt_builder.py:164-171 (SKILLS_GUIDANCE)
SKILLS_GUIDANCE = (
    "After completing a complex task (5+ tool calls), fixing a tricky error, "
    "or discovering a non-trivial workflow, save the approach as a "
    "skill with skill_manage so you can reuse it next time.\n"
    "When using a skill and finding it outdated, incomplete, or wrong, "
    "patch it immediately with skill_manage(action='patch') — don't wait to be asked. "
    "Skills that aren't maintained become liabilities."
)
```

```python
# run_agent.py:3072-3077 (行为引导的条件注入)
if "skill_manage" in self.valid_tool_names:
    tool_guidance.append(SKILLS_GUIDANCE)
```

**为什么有效**: Skill 不是一次性的。它作为 system prompt 的一部分在**每次对话中都存在**（`~/.hermes/skills/*/SKILL.md`），意味着 Agent 永远"记得"之前学到的方法。这与人类的工作记忆不同——它不会被遗忘，除非被显式删除。

### 1.3 工具可用性 = 行为能力

```python
# run_agent.py:3070-3079
# 只有当工具实际可用时，才会注入对应的行为引导
if "memory" in self.valid_tool_names:
    tool_guidance.append(MEMORY_GUIDANCE)
if "session_search" in self.valid_tool_names:
    tool_guidance.append(SESSION_SEARCH_GUIDANCE)
if "skill_manage" in self.valid_tool_names:
    tool_guidance.append(SKILLS_GUIDANCE)
```

**为什么有效**: 这不仅仅是文本提示——`skill_manage` 是一个**真实可调用的工具**。Agent 看到 guidance 的同时也拥有执行它的能力。没有工具的提示只是空话，有工具的提示才能转化为行动。

---

## 二、Fuzzy Match — 为什么它是有效的修补机制

### 2.1 9 级降级匹配链

```python
# tools/fuzzy_match.py:73-83
strategies: List[Tuple[str, Callable]] = [
    ("exact", _strategy_exact),                    # 精确匹配
    ("line_trimmed", _strategy_line_trimmed),       # 行级裁剪空白
    ("whitespace_normalized", _strategy_whitespace_normalized),  # 多空格折叠
    ("indentation_flexible", _strategy_indentation_flexible),    # 忽略缩进差异
    ("escape_normalized", _strategy_escape_normalized),          # \n 字面量转真实换行
    ("trimmed_boundary", _strategy_trimmed_boundary),            # 首尾行空白裁剪
    ("unicode_normalized", _strategy_unicode_normalized),        # Unicode 引号标准化
    ("block_anchor", _strategy_block_anchor),                    # 首尾行锚定 + 中间相似度
    ("context_aware", _strategy_context_aware),                  # 50% 行相似度阈值
]
```

**为什么有效**: 这是自我改进**能持续运作**的关键。LLM 生成的补丁几乎从不与原文精确匹配——缩进不同、空格不同、引号风格不同。如果没有模糊匹配，每次修补都会失败，Agent 会放弃改进 Skill。有了模糊匹配，修补的成功率大幅提高，形成了**正向反馈循环**：

```
修补成功 → Agent 更有信心修补 → 更多 Skill 被改进 → 能力更强
修补失败 → Agent 失去信心 → 不再尝试修补 → Skill 退化
```

### 2.2 安全保护

```python
# tools/skill_manager_tool.py:328-332
# 每次写入后立即安全扫描，失败则回滚
scan_error = _security_scan_skill(skill_dir)
if scan_error:
    shutil.rmtree(skill_dir, ignore_errors=True)
    return {"success": False, "error": scan_error}
```

**为什么有效**: 没有安全保障的自我改进是危险的。Agent 可能生成包含恶意代码的 Skill。安全扫描确保只有安全的改进被保留。

---

## 三、Nudge 机制 + 后台 Review — 为什么能形成闭环

### 3.1 计数器 + 阈值触发

```python
# run_agent.py:1093-1096
self._memory_nudge_interval = 10    # 每 10 轮提醒
self._turns_since_memory = 0
self._iters_since_skill = 0
```

```python
# run_agent.py:7657-7660 (user turn 结束时递增)
self._turns_since_memory += 1
if self._turns_since_memory >= self._memory_nudge_interval:
    _should_review_memory = True
    self._turns_since_memory = 0
```

```python
# run_agent.py:7903-7907 (tool-calling iteration 结束时递增)
if (self._skill_nudge_interval > 0
        and "skill_manage" in self.valid_tool_names):
    self._iters_since_skill += 1
```

```python
# run_agent.py:10239-10245 (turn 结束时检查触发)
if (self._skill_nudge_interval > 0
        and self._iters_since_skill >= self._skill_nudge_interval
        and "skill_manage" in self.valid_tool_names):
    _should_review_skills = True
    self._iters_since_skill = 0
```

### 3.2 后台 Review Agent — 真正的自我改进引擎

```python
# run_agent.py:2054-2087 (三个 review prompt)
_MEMORY_REVIEW_PROMPT = (
    "Review the conversation above and consider saving to memory if appropriate.\n\n"
    "Focus on:\n"
    "1. Has the user revealed things about themselves — their persona, desires, "
    "preferences, or personal details worth remembering?\n"
    "2. Has the user expressed expectations about how you should behave, their work "
    "style, or ways they want you to operate?\n\n"
    "If something stands out, save it using the memory tool. "
    "If nothing is worth saving, just say 'Nothing to save.' and stop."
)

_SKILL_REVIEW_PROMPT = (
    "Review the conversation above and consider saving or updating a skill if appropriate.\n\n"
    "Focus on: was a non-trivial approach used to complete a task that required trial "
    "and error, or changing course due to experiential findings along the way, or did "
    "the user expect or desire a different method or outcome?\n\n"
    "If a relevant skill already exists, update it with what you learned. "
    "Otherwise, create a new skill if the approach is reusable.\n"
    "If nothing is worth saving, just say 'Nothing to save.' and stop."
)
```

```python
# run_agent.py:2089-2168 (后台 review 实现)
def _spawn_background_review(
    self,
    messages_snapshot: List[Dict],
    review_memory: bool = False,
    review_skills: bool = False,
) -> None:
    """
    创建一个完整的 AIAgent 副本，拥有相同的模型、工具和上下文。
    review prompt 作为下一个 user turn 追加到复制的对话中。
    直接写入共享的 memory/skill store。
    绝不修改主对话历史或产生用户可见的输出。
    """
```

**为什么有效**: 这是最精妙的部分。后台 Review 不是在原对话中进行的——它创建了一个**独立的 Agent 副本**，在后台静默运行。这样做有四个关键优势：

1. **不干扰主对话**: Review 不消耗主对话的 token 预算，不影响用户体验
2. **完整的上下文**: Review Agent 拥有完整的对话历史，能看到整个任务流程
3. **相同的工具能力**: Review Agent 有完整的 `skill_manage` 和 `memory` 工具
4. **独立的结果**: Review 的结果直接写入共享存储，立即对下次对话生效

### 3.3 闭环的完整执行路径

```
用户发起任务
    ↓
主 Agent 执行（调用工具、编写代码等）
    ↓
计数器递增 (_iters_since_skill += 1)
    ↓
每 N 轮检查: _iters_since_skill >= _skill_nudge_interval?
    ├─ 否 → 继续
    └─ 是 → 触发 _spawn_background_review()
            ↓
        创建 Review Agent 副本（相同模型、工具、对话历史）
            ↓
        注入 _SKILL_REVIEW_PROMPT
            ↓
        Review Agent 分析对话，寻找可固化的模式
            ├─ 找到 → 调用 skill_manage(action='create'/'patch')
            │        → 写入 ~/.hermes/skills/
            │        → 安全扫描
            │        → 用户看到 "💾 Skill 'xxx' created"
            └─ 没找到 → "Nothing to save."
            ↓
        新 Skill 立即出现在下次对话的 system prompt 中
            ↓
    下次同类任务 → Agent 自动发现并调用新 Skill
```

---

## 四、Skills Guard — 为什么安全机制保证了系统可靠性

### 4.1 信任分级

```python
# tools/skills_guard.py:41-47
INSTALL_POLICY = {
    #                  safe      caution    dangerous
    "builtin":       ("allow",  "allow",   "allow"),   # 内置技能，信任
    "trusted":       ("allow",  "allow",   "block"),   # OpenAI/Anthropic 官方技能
    "community":     ("allow",  "block",   "block"),   # 社区技能，严格
    "agent-created": ("allow",  "allow",   "ask"),     # Agent 自创，宽松但可询问
}
```

### 4.2 威胁模式扫描

```python
# tools/skills_guard.py:82-150 (部分)
THREAT_PATTERNS = [
    # 数据外泄
    (r'curl\s+[^\n]*\$\{?\w*(KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL|API)',
     "env_exfil_curl", "critical", "exfiltration", ...),
    (r'os\.environ\b(?!\s*\.get\s*\(\s*["\']PATH)',
     "python_os_environ", "high", "exfiltration", ...),
    # 持久化
    (r'authorized_keys', "ssh_backdoor", "critical", "persistence", ...),
    # 破坏性命令
    (r'rm\s+-rf\s+/', "destructive_rm_rf", "critical", "destructive", ...),
    # 提示注入
    (r'ignore\s+(previous|all|above|prior)\s+instructions',
     "prompt_injection", "critical", "injection", ...),
]
```

**为什么有效**: 自我改进系统最大的风险是**自我强化错误**。如果没有安全保障，Agent 可能：
- 生成包含恶意代码的 Skill
- 在 Skill 中嵌入 prompt injection
- 创建泄露用户数据的 Skill

安全扫描确保这些情况被拦截，使系统能够**安全地**自我改进。

---

## 五、总结：为什么这个方法是有效的

| 机制 | 为什么有效 | 源码位置 |
|------|-----------|----------|
| **Skill 作为知识载体** | 每次对话都出现在 system prompt 中，是"永不遗忘"的程序性记忆 | `run_agent.py:3076` |
| **Fuzzy Match** | 修补成功率高，形成正向反馈循环，Agent 有信心持续改进 | `tools/fuzzy_match.py:73-83` |
| **Nudge 计数器** | 周期性提醒，确保改进不是偶发事件，而是持续行为 | `run_agent.py:7903-7907, 10239-10245` |
| **后台 Review Agent** | 独立副本不干扰主对话，但拥有完整上下文和工具能力 | `run_agent.py:2089-2168` |
| **Skills Guard** | 防止自我强化错误，确保改进是安全的 | `tools/skills_guard.py:41-47` |
| **原子写入 + 回滚** | 保证数据一致性，失败时不留半写状态 | `tools/skill_manager_tool.py:256-285` |

### 核心洞见

**自我改进不在模型内部，而在模型可用的上下文中。**

传统 AI 的自我改进是通过更新模型权重实现的（RLHF、fine-tuning）。Hermes Agent 的自我改进是通过**更新 prompt** 实现的：

1. 从经验中提取模式 → 写入 Skill 文件
2. Skill 文件出现在下次对话的 system prompt 中
3. Agent 在新的上下文中表现得更好

这不是模型在学习，而是**模型可用的信息在增长**。就像人类通过写笔记来记住东西一样，Agent 通过写 Skill 来"记住"如何做事。

**这个方法的根本优势**: 不需要重新训练模型，不需要 GPU，不需要停机。改进是即时的、可逆的、可审计的。
