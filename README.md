# umohobot

> 一个由 LLM 驱动的 AI 机器人。（当前实现了 Telegram 接入）

## 架构

```
Telegram 更新 → trigger-telegram ──┐
                                   ├─→ agent ──→ tools-telegram（Telegram 相关工具）
[其他事件源] → (你的 trigger) ───────┘    │           tools-web（网页相关工具）
                                          │           tools-image（图像相关工具）
                                          │           tools-subagent（子代理相关工具）
                                          │           tools-time（时间相关工具）
```

- **`agent/`** — Agent 核心。管理对话线程、工具注册、子代理、模型池及 API key 负载均衡、持久化。对外接口是 `AgentHandle` trait
- **`trigger-telegram/`** — Telegram 触发器。监听 Telegram 更新，通过 `AgentHandle` 驱动 agent。也是写其他触发器时的参考实现
- **`tools-telegram/`** — Telegram 相关工具，赋予 agent 操作 Telegram 的能力
- **其他 `tools-*`** — 网页抓取、OCR、定时器、子代理管理等

## 快速开始

```bash
# 1. 配置 config.toml，设置模型来源
# 2. 导出环境变量
export TELEGRAM_BOT_TOKEN="xxx"
export DEEPSEEK_KEY="sk-xxx"
export GLM_KEY="xxx"

# 3. 编译并运行
cargo r -r -- --system-prompt "$(cat prompt.txt)"
```

## 配置

`config.toml` 支持多个模型来源，`api-key` 指向环境变量名（也可以用 `api-key-raw` 直接填写密钥）：

```toml
default-model = "deepseek/deepseek-v4-flash"

[[model-accounts]]
provider = "deepseek"
model = "deepseek-v4-flash"
api-key = "DEEPSEEK_KEY"

[[model-accounts]]
provider = "bigmodel"
model = "glm-4.6v"
api-key = "GLM_KEY"
base-url = "https://open.bigmodel.cn/api/paas/v4"
capabilities = ["image"]
```

常见 provider 有内置的 base URL（openai、deepseek、openrouter、groq、together、mistral），其他需要手动指定。

## CLI 参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--telegram-token` | Telegram Bot Token（或环境变量 `TELEGRAM_BOT_TOKEN`） | 必填 |
| `--config` | 配置文件路径 | `config.toml` |
| `--system-prompt` | 注入到系统 prompt 中的额外要求 | `"You are a helpful Telegram bot."` |
| `--idle-timeout-seconds` | 对话线程空闲超时 | `300` |
| `--max-thread-length` | 线程最大消息数，超限后压缩 | `100` |
| `--max-turns` | 每轮请求最大 LLM 调用次数 | `10` |

`--system-prompt` 的内容会被注入到 `trigger-telegram` 系统 prompt 末尾，用于填写人设或额外行为约束。

## 各 crate 说明

| crate | 作用 |
|-------|------|
| `agent/` | Agent 核心、线程管理、子代理、模型池 |
| `trigger-telegram/` | Telegram 触发器实现 |
| `telegram-host/` | Telegram API 封装（供 trigger 和 tools 共用） |
| `tools-telegram/` | Telegram 相关工具 |
| `tools-web/` | 网页相关工具 |
| `tools-image/` | 图像相关工具（OCR） |
| `tools-time/` | 时间相关工具（定时器） |
| `tools-subagent/` | 子代理管理工具 |
| `data-buffer/` | 进程内共享数据缓冲区 |

## TODO

- [ ] Telegram trigger 更多消息信息
- [ ] 更好用的历史查询
- [ ] notes 工具
- [ ] 更多 images 工具
- [ ] 后台管理 dashboard
- [ ] 许可证
