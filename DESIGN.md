# 线程、存储与 Prompt 设计说明

本文档整理当前已经确定的实现边界，重点覆盖 Telegram thread 设计、存储结构、prompt contract，以及它们之间的关系。

## 1. 设计目标

- 让每个 Telegram chat 具备稳定的 thread 语义。
- 支持群聊多人共享上下文，但保持说话人身份可追踪。
- 支持 thread 超时、超量、手工关闭后自动开启新 thread。
- 支持 tool calling 闭环，同时保留审计与限额控制。
- 支持线程摘要和上下文压缩，避免 prompt 无限膨胀。

## 2. 核心概念

### 2.1 thread_key

thread_key 是外部稳定作用域，用来判断一条消息应该进入哪个 thread。

- Telegram 私聊：`telegram:<chat_id>`
- Telegram 普通群聊：`telegram:<chat_id>`
- Telegram forum topic：`telegram:<chat_id>:topic:<message_thread_id>`

如果平台以后切换到 Discord 或 Matrix，也要保留同样的“平台 + 房间 + 子线程”思路。

### 2.2 thread_id

thread_id 是内部实例 ID，用来表示某个 thread 的一次具体生命周期。

- 同一个 thread_key 可以在不同时间对应多个 thread_id。
- thread 关闭后，下一条消息会开启新的 thread_id。
- 新 thread 可以引用旧 thread 的摘要，但不继承完整历史。

### 2.3 turn

turn 表示一次完整的“消息接纳 -> LLM 推理 -> tool loop -> 最终回复”过程。

- 一个 turn 只对应一个最终回复目标。
- turn 内部可以有多次 tool call。
- turn 不等于一条 Telegram 消息；它更接近一次 agent 运行。

### 2.4 event

event 是 thread 中的原子事实记录。

常见 event 类型：

- 用户消息
- assistant 最终回复
- tool call
- tool result
- thread summary
- system 注记

event 应尽量 append-only，作为审计和回放的事实来源。

### 2.5 thread lease

thread lease 表示“这个会话 thread 还处于活跃窗口内”。

- 它决定同一条新消息是否应该继续复用当前 thread。
- 每次接纳新消息时刷新 `lease_until`，通常设为 `now + thread_idle_timeout_secs`。
- turn 结束时也可以再次延长 thread lease，但不能缩短已有的更晚截止时间。
- `lease_until` 过期后，thread 不再被视为活跃；reaper 可以将其关闭。
- `NULL` 不应被解释为“永久可复用”；它只能表示未设置或历史脏数据，需要在实现中显式处理。

### 2.6 turn lease

turn lease 表示“一次 agent 运行还在进行中”。

- 它用于并发保护和运行超时回收。
- `start_turn` 时设置，`finish_turn` 时结束。
- 它不决定 thread 是否进入新一轮对话，也不承担群聊闲置判定。
- 若 turn 超时未结束，turn reaper 负责将其标记为失败或超时。

### 2.7 reaper

系统需要至少两个后台回收器：

- `turn reaper`：回收超时的 `running` turn，并释放相关运行锁。
- `thread reaper`：关闭已过期且没有正在运行 turn 的 active thread。

两者职责不要混用。thread 是否关闭，不能只看 turn 是否结束；turn 是否超时，也不能靠 thread 的闲置时间推断。

## 3. Thread 生命周期

推荐状态只有三类：

- `active`
- `draining`
- `closed`

### 3.1 接纳消息

当一条消息到达时：

1. 根据 thread_key 查找当前活跃 thread。
2. 如果没有活跃 thread，或已超时 / 超量，则创建新 thread。
3. 立即刷新 `last_activity_at`。
4. 刷新 thread lease，延长这个会话的活跃窗口。
5. 把消息作为 event 写入 thread。

### 3.2 运行 turn

turn 运行期间：

- 由单独的 turn lease 保护运行中的 agent 调用。
- 如果模型调用或工具调用超时，turn reaper 负责收尾。
- 如果模型需要工具，由宿主执行 tool call。
- tool result 进入 event 流，然后继续喂回模型。
- placeholder 消息属于 UI 行为，不应直接取代 thread 事实记录。

### 3.3 关闭 thread

thread 结束条件建议包括：

- idle timeout
- 最大 turn 数
- 最大上下文 token
- thread 级预算上限

关闭规则：

- 只标记为 `closed`，不要删除历史。
- 关闭后下一个消息自然进入新 thread。
- 只有 thread lease 过期，且没有正在运行的 turn 时，thread 才能被 reaper 关闭。
- 如果有摘要，则新 thread 可继承最后摘要。

### 3.4 群聊规则

- 同一群聊中的多个用户消息默认都属于同一个 thread。
- “是否进入上下文”和“是否触发回复”要分开。
- 触发回复的默认策略可以是 mention、reply-to-bot、命令触发。
- 工具调用的授权主体永远是当前触发者，而不是整个群组。

## 4. 存储设计

当前第一版实现使用 SQLite，并通过 `sqlx` 管理迁移。
同时要保留 storage 抽象层，让业务层只依赖仓储接口，不直接绑定到 SQLite 细节。

### 4.1 设计原则

- 事实记录和派生状态分开。
- 原始 JSON 和规范化字段同时保留。
- 所有时间使用 UTC。
- 采用事务保证 thread 接纳、turn 续租、seq 更新等操作的一致性。

### 4.2 核心表

#### threads

线程主表，保存 thread 当前状态。

建议字段：

- `id`
- `thread_key`
- `platform`
- `chat_id`
- `topic_id`
- `state`
- `opened_at`
- `last_activity_at`
- `lease_until`
- `closed_at`
- `parent_thread_id`
- `summary_cursor`
- `turn_count`
- `version`

其中 `threads.lease_until` 表示 thread 的活跃截止时间。

#### turns

一次完整 agent 运行的记录。

建议字段：

- `id`
- `thread_id`
- `trigger_event_id`
- `status`
- `started_at`
- `ended_at`
- `provider`
- `model`
- `prompt_version`
- `context_hash`
- `lease_until`
- `placeholder_message_id`
- `final_message_id`
- `prompt_tokens`
- `completion_tokens`
- `tool_calls`
- `estimated_usage`
- `error_code`

其中 `turns.lease_until` 表示 turn 的运行截止时间。

#### events

thread 内的原子事实流。

建议字段：

- `id`
- `thread_id`
- `seq`
- `turn_id`
- `kind`
- `sender_id`
- `sender_name`
- `platform_message_id`
- `reply_to_platform_message_id`
- `content_json`
- `visible_to_model`
- `created_at`

#### tool_calls

每一次工具调用都独立记录。

建议字段：

- `id`
- `turn_id`
- `tool_name`
- `args_json`
- `policy_decision`
- `status`
- `result_json`
- `started_at`
- `ended_at`
- `error_kind`

#### summaries

线程摘要表，用于压缩历史。

建议字段：

- `id`
- `thread_id`
- `upto_seq`
- `summary_text`
- `created_at`
- `model`
- `prompt_version`

#### usage_ledger / usage_totals

- `usage_ledger`：流水账。
- `usage_totals`：聚合账，用于预算预检。

### 4.3 写入和维护

- `events`、`tool_calls`、`usage_ledger` 应保持 append-only。
- `threads`、`turns`、`summaries`、`usage_totals` 属于派生状态。
- 后台任务至少三个：
  - thread reaper：关闭过期且无正在运行 turn 的 thread。
  - turn reaper：回收超时的 running turn。
  - summarizer：压缩过长 thread 的历史。
- 具体后端先实现 SQLite，后续如果需要切换数据库，只替换 storage backend，不改上层调用。

## 5. Prompt Contract

### 5.1 是否需要

需要，而且应该长期存在。但它要被当成“上下文协议”，而不是临时拼接的大段提示词。

### 5.2 组装顺序

每次模型调用建议按固定顺序组装：

1. system rules
2. thread summary
3. recent events
4. current turn input
5. tool catalog
6. response policy

### 5.3 具体要求

- prompt 需要版本化，`turns` 和 `summaries` 都要记录 `prompt_version`。
- thread summary 负责长期记忆压缩。
- recent events 负责短期事实和最近对话。
- current turn input 负责当前任务。
- tool result 的优先级高于旧上下文。
- 群聊里必须保留说话人身份，不能把多人消息折叠成匿名文本。
- 不要把 placeholder 消息、内部推理过程或宿主敏感状态拼进 prompt。
- context builder 必须负责 token budget，超限时优先裁剪旧 event 和冗长历史。

### 5.4 与 tool calling 的关系

- prompt 的职责是让模型理解“当前上下文”和“可用工具”。
- 工具是否能执行，仍然由宿主的权限与额度层决定。
- 在工具未实现阶段，`tool catalog` 也要保持结构占位，避免以后接口重排。

## 6. 配置建议

建议后续在配置文件中加入以下参数：

- `thread_idle_timeout_secs`
- `turn_lease_secs`
- `thread_max_turns`
- `thread_soft_context_tokens`
- `thread_hard_context_tokens`
- `thread_summary_max_chars`
- `group_listen_mode`
- `prompt_version`

这些参数都应通过配置文件或环境覆盖，而不是硬编码在 Rust 源码里。

## 7. 实现顺序建议

1. 先完成 thread / turn / event 的存储骨架。
2. 再接 thread lease、turn lease 和 reaper。
3. 再接 prompt contract 和 context builder。
4. 再接 tool calling 闭环。
5. 再扩展总结压缩、预算控制和更多平台。
