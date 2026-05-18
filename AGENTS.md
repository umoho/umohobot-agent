# AGENTS.md

本文件用于指导后续在此仓库工作的代理或开发者。请在修改代码前先阅读 `PLAN.md` 和当前源码。

## 项目定位

这是一个 Rust 写的 AI Chat Bot 项目，首发平台是 Telegram，后续预留 Discord 和 Matrix 接入能力。核心目标不是简单问答，而是一个带工具、带权限、带配额控制的 agent 系统。

## 已确定的设计原则

1. 工具由 LLM 决定是否调用，宿主不做业务路由替代。
2. 宿主负责工具授权、执行、审计、限额与隔离。
3. 模型不能直接访问文件系统、shell 或其他高风险能力。
4. 默认回复方式是先发占位消息，再编辑成最终答案。
5. 先实现 Telegram，但平台适配层必须保持可替换。
6. 允许用户自带 provider key，但不能把密钥当作普通聊天内容处理。
7. 模型名、provider 和其他运行参数必须来自配置文件或环境覆盖，不能硬编码进 Rust 源码。
8. Telegram bot token 只能通过环境变量注入，不要写入配置文件。
9. 线程、存储和 prompt contract 的详细约定见 `DESIGN.md`，实现时不要自行改写这些术语。
10. thread 的活跃状态要在消息接纳时刷新，turn 运行期间靠 lease 保护，避免超时 race。
11. prompt 要按固定 contract 组装，顺序是 `thread summary`、`recent events`、`current turn`、`tool catalog`、`response policy`。

## 开发优先级

1. Telegram 消息接入。
2. `rig` agent 集成。
3. 占位消息 + 编辑回复。
4. 计算器工具。
5. 联网检索工具。
6. 使用量统计与超量切断。
7. 权限系统与审计日志。
8. BYOK provider 配置。
9. 其他平台与更多工具。

## 代码边界建议

- `src/platforms/`：各聊天平台适配层。
- `src/agent/`：AI 核心、prompt 组织、thread/turn 协调、工具调用流程。
- `src/tools/`：工具实现。
- `src/policy/`：权限、配额、风险控制。
- `src/storage/`：数据库访问、thread/event/turn/summary/usage 账本、配置。
- `src/config/`：配置加载与环境变量。
- 所有模块统一采用 `name.rs` + `name/name.rs` 的目录布局，禁止新增 `mod.rs`。
- 如果需要继续拆分子模块，也必须沿用同名目录、同名文件的递归方式。

如果当前目录结构不同，优先按职责拆分，不要让平台逻辑、模型逻辑和权限逻辑混在同一个文件里。

## 安全要求

1. 不要把用户 token 写进普通日志。
2. 不要把模型输出直接当作系统命令执行。
3. 不要给模型开放任意文件读写。
4. 不要绕过权限层直接调用高风险工具。
5. 如果要实现文件或 shell 能力，必须放入最小权限的独立执行环境。
6. 所有工具调用都应可审计。

## 工具实现要求

- 计算器必须确定性、无副作用。
- 联网检索必须只读，并记录来源。
- 每个工具都应有明确 schema、超时、错误类型和风险等级。
- 工具参数不要依赖自然语言猜测，尽量结构化。

## 配额与计量要求

- 每次请求前先预检预算。
- 每次请求后记录 usage。
- provider 不回传 usage 时使用估算值。
- 配额至少支持按用户、群组、会话维度统计。

## 线程与存储要求

- Telegram chat 默认作为 thread 作用域，forum topic 需要把 `message_thread_id` 纳入 thread key。
- thread 关闭后只标记为 `closed`，不要删历史。
- thread 的事实记录用 append-only event，`threads`、`turns`、`summaries`、`usage_totals` 属于派生状态。
- 新 thread 可以继承上一 thread 的摘要，但不要继承完整历史。
- 第一版存储后端先做 SQLite，但要保留 storage 抽象层，业务层不要直接依赖 SQLite 细节。

## Prompt 要求

- prompt 不是临时拼接长文本，而是稳定 contract。
- 上下文顺序固定为 `thread summary`、`recent events`、`current turn`、`tool catalog`、`response policy`。
- prompt 要版本化，`turn` 和 `summary` 都应记录 `prompt_version`。
- 不要把 placeholder 消息、内部推理过程或不该暴露的宿主状态拼进 prompt。

## 交互要求

- 收到用户消息后先回占位消息。
- 有新信息时编辑同一条消息。
- 流式输出要节流，避免频繁编辑。
- 编辑失败时允许降级成新消息。

## BYOK 处理要求

- 首选私聊中的配置流程或安全页面。
- 支持 `provider`、`base_url`、`model`、`api_key` 的绑定。
- 支持测试连接、更新、撤销。
- 不要在群聊里直接收集密钥。

## 配置文件要求

- 默认运行配置文件是 `config.toml`。
- 仓库内保留 `config.example.toml` 作为示例。
- 不要把真实配置值写进 Rust 源代码。
- 不要提交本地真实配置文件。
- 新增运行参数时，优先扩展配置文件结构，再考虑代码默认值。
- Telegram bot token 不属于配置文件内容，只允许从环境变量 `TELEGRAM_BOT_TOKEN` 或 `TELOXIDE_TOKEN` 读取。

## 工作方式

1. 先理解当前文件和计划，再改代码。
2. 小步提交，保持变更边界清晰。
3. 不要重构无关模块。
4. 不要删除用户已有改动，除非明确要求。
5. 如果设计与现有代码冲突，先更新计划或说明，再动手。
6. 修改完成后，至少说明影响的文件和行为变化。
7. 新增模块时先遵守本仓库的 `name.rs` + `name/name.rs` 结构，再考虑进一步拆分。

## 当前仓库状态

当前仓库仍然很小，后续实现应以“从骨架向模块化演进”为目标，不要一开始写成难维护的单文件脚本。
