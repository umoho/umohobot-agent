# umohobot

## 架构

三层分层设计：

- **agent/** — Agent 核心。对外接口是抽象的 `AgentHandle` trait，实现是 `AgentRuntime`。负责对话线程管理、工具注册、子代理、模型池及 API key 负载均衡、持久化。agent 不感知具体事件源——它只认 `AgentHandle` 这个抽象接口
- **trigger-*/** — 触发器的具体实现。每种 trigger 实现一个事件源，通过 `AgentHandle` 驱动 agent。当前只有 `trigger-telegram`（监听 Telegram 更新），你可以参考它对接其他事件源（Discord、CLI、Webhook 等）
- **tools-*/** — 工具集，注册到 agent 上由 LLM 调用。包括 `tools-telegram`、`tools-web`、`tools-image`、`tools-time`、`tools-subagent` 等

数据流：`事件源 → trigger → AgentHandle::run_turn() → agent 调用 tools → 响应`

## 代码风格

- **不使用 mod.rs**：模块根文件用 `module.rs`，子模块放在 `module/` 目录下（如 `storage.rs` + `storage/file.rs`）
- 保持文件职责单一，每个文件一个主要类型或功能
- tool 实现文件在对应 `tools-*/src/` 下，每类工具一组函数

## 工具命名格式

`<namespace>_<action>` — namespace 是功能域，action 是操作描述。action 中动词和名词直接用下划线或 CamelCase 拼接，以清晰为准。

示例：
- `telegram_sendMessage` / `telegram_sendPhoto` / `telegram_deleteMessage`
- `telegram_query_message` / `telegram_query_messages`
- `time_timer_set` / `time_timer_cancel`
- `web_scrape` / `web_fetch` / `web_find`
- `subagent_create` / `subagent_destroy`
