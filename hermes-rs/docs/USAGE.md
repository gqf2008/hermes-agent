# Hermes Agent CLI 使用文档

> 由 Nous Research 构建的自进化 AI Agent

## 安装

### Windows

1. 下载 `hermes.exe` 到任意目录（如 `C:\hermes\`）
2. 将该目录加入 `PATH` 环境变量，或直接使用绝对路径

```powershell
# 验证安装
hermes --version
```

### 从源码构建

```bash
cd hermes-rs
cargo build --release
# 二进制位于 target/release/hermes (Linux/macOS) 或 hermes.exe (Windows)
```

---

## 快速开始

### 1. 首次配置

```bash
hermes setup
```

交互式引导会引导你完成：
- 选择 AI 模型提供商（OpenRouter / OpenAI / Anthropic 等）
- 输入 API Key
- 选择默认模型
- 配置工具集

配置文件存储在：
- `~/.hermes/config.yaml` — 设置
- `~/.hermes/.env` — API 密钥

### 2. 开始对话

```bash
hermes chat
```

或指定模型：

```bash
hermes chat -m anthropic/claude-sonnet-4-20250514
```

---

## 命令参考

### chat — 交互式对话

```bash
hermes chat [OPTIONS]

# 常用选项
-m, --model <MODEL>              指定模型（如 openrouter/openai/gpt-4o）
-q, --quiet                      安静模式（抑制调试输出）
-v, --verbose                    详细日志
    --skip-context-files         跳过加载上下文文件
    --skip-memory                跳过加载记忆
    --voice                      语音模式
```

### setup — 交互式配置向导

```bash
hermes setup
```

引导式配置：模型提供商、API Key、默认模型、工具集。

### gateway — 消息平台网关

支持 15+ 消息平台（Telegram、Discord、Slack、微信、飞书、钉钉、企业微信等）。

```bash
# 后台启动
hermes gateway start

# 前台运行（调试用）
hermes gateway run

# 停止
hermes gateway stop

# 查看状态
hermes gateway status

# 安装为系统服务（systemd/launchd/Windows Task Scheduler）
hermes gateway install

# 卸载服务
hermes gateway uninstall
```

### config — 配置管理

```bash
# 查看当前配置
hermes config show

# 编辑配置文件
hermes config edit

# 设置配置值
hermes config set agent.model anthropic/claude-sonnet-4-20250514
hermes config set compression.enabled true
hermes config set terminal.backend docker
```

### tools — 工具管理

```bash
# 列出所有可用工具
hermes tools list

# 查看工具详情
hermes tools info <tool-name>
```

### skills — 技能管理

```bash
# 列出所有技能
hermes skills list

# 查看技能详情
hermes skills info <skill-name>

# 启用/禁用
hermes skills enable <skill-name>
hermes skills disable <skill-name>

# 列出已注册的斜杠命令
hermes skills commands
```

### sessions — 会话管理

```bash
# 列出近期会话
hermes sessions list

# 按关键词搜索会话
hermes sessions search "query"

# 导出会话到 JSON
hermes sessions export --session-id <id> --output session.json

# 删除会话
hermes sessions delete --session-id <id>

# 会话统计
hermes sessions stats
```

### profiles — 多 Profile 管理

```bash
# 列出所有 profile
hermes profiles list

# 创建新 profile
hermes profiles create <name>

# 切换到指定 profile
hermes profiles use <name>
```

所有命令都支持 `--hermes-home <path>` 临时指定数据目录。

### batch — 批量处理

```bash
# 处理 JSONL 数据集
hermes batch run --input data.jsonl --output results.jsonl

# 查看可用的工具集发行版
hermes batch distributions

# 查看批处理状态
hermes batch status
```

### cron — 定时任务

```bash
# 列出已计划的任务
hermes cron list

# 创建定时任务
hermes cron create --cron "0 9 * * *" --prompt "每日晨报"

# 删除定时任务
hermes cron delete --id <job-id>

# 暂停/恢复
hermes cron pause --id <job-id>
hermes cron resume --id <job-id>
```

### doctor — 诊断

```bash
hermes doctor
```

检查：
- 配置文件是否存在
- API 密钥是否有效
- 会话数据库是否正常
- 工具是否正确注册
- 常见配置错误

---

## 高级用法

### 环境变量

| 变量 | 说明 |
|------|------|
| `HERMES_HOME` | 自定义数据目录（替代 `~/.hermes`） |
| `OPENAI_API_KEY` | OpenAI API 密钥 |
| `ANTHROPIC_API_KEY` | Anthropic API 密钥 |
| `OPENROUTER_API_KEY` | OpenRouter API 密钥 |
| `DEEPSEEK_API_KEY` | DeepSeek API 密钥 |
| `GOOGLE_API_KEY` | Google/Gemini API 密钥 |

### 配置文件结构

`~/.hermes/config.yaml`:

```yaml
agent:
  model: anthropic/claude-sonnet-4-20250514
  provider: anthropic
  quiet: false
  toolsets:
    - filesystem
    - web
    - terminal

compression:
  enabled: true
  target_tokens: 50

terminal:
  backend: local
  docker_image: ubuntu:latest
```

### 多 Profile 隔离

每个 profile 有独立的数据目录：

```bash
hermes --hermes-home ~/.hermes-dev setup
hermes --hermes-home ~/.hermes-dev chat

hermes --hermes-home ~/.hermes-prod setup
hermes --hermes-home ~/.hermes-prod chat
```

---

## Gateway 平台支持

| 平台 | 适配器 | 备注 |
|------|--------|------|
| Telegram | telegram | Bot Token |
| Discord | discord | Bot Token |
| Slack | slack | Bot Token |
| 微信 | weixin | 微信公众号/个人号 |
| 飞书 | feishu | App ID + App Secret |
| 钉钉 | dingtalk | Client ID + Client Secret |
| 企业微信 | wecom | Corp ID + Agent ID |
| Signal | signal | signal-cli |
| WhatsApp | whatsapp | waha 网关 |
| 飞书(国际) | lark | Lark App ID |
| 飞书国内版 | feishu | Feishu Open API |
| OpenAI API | api_server | OpenAI 兼容 HTTP API |

启动指定平台：

```bash
hermes gateway run --platform telegram
```

---

## 数据目录结构

```
~/.hermes/
├── config.yaml              # 主配置
├── .env                     # API 密钥
├── sessions.db              # SQLite 会话数据库 (含 FTS5 搜索)
├── cron_jobs.json           # 定时任务
├── webhooks.json            # Webhook 订阅
├── .plugin_registry.json    # 插件注册表
├── skills/                  # 技能文件
│   ├── index.json
│   └── *.md
├── plugins/                 # 插件目录
└── logs/                    # 日志
```

---

## 常见问题

### Q: 如何切换模型？

```bash
hermes config set agent.model openrouter/openai/gpt-4o
```

或在对话中随时指定：

```
>m anthropic/claude-sonnet-4-20250514
```

### Q: 如何查看帮助？

```bash
hermes help
hermes chat --help
hermes gateway --help
```

### Q: 配置有问题怎么办？

```bash
hermes doctor
```

### Q: 如何清理旧会话？

```bash
hermes sessions delete --session-id <id>
```

### Q: 如何备份数据？

```bash
hermes backup create
hermes backup list
```
