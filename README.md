# synra-app

> **应用仓库。** Tauri 2 桌面端 + 移动端，以及宿主 UI 与插件 UI 的承载。
>
> 这是 **`synra` 五仓库拓扑**中的第 2 个（[设计文档 §3](https://github.com/synra-rs/design-docs/blob/main/README.md)）：
>
> | 仓库                                                       | 装什么                                                        |
> | ---------------------------------------------------------- | ------------------------------------------------------------- |
> | [`synra-core`](https://github.com/synra-rs/synra-core)       | Rust 内核 + CLI + 插件工具链                                    |
> | **`synra-app`** ← 本仓库                                     | Tauri 应用（桌面 + 移动 + 宿主 UI）                               |
> | [`synra-js`](https://github.com/synra-rs/synra-js)           | 插件 UI 的类型库（`@synra/client` / `@synra/protocol` / `@synra/theme`） |
> | [`docs`](https://github.com/synra-rs/docs)                   | 官网 + 用户文档 + schema                                         |
> | [`design-docs`](https://github.com/synra-rs/design-docs)     | 设计文档（唯一权威）                                             |
>
> **域名**：[synra-app.dev](https://synra-app.dev)（官网 + 文档 + schema 托管）

## 状态

✅ **已在实现中。**

这份 README 曾经写着「尚未开始实现。目前只有这份 README」——那句话在写下它的当天就不再成立。留着这个更正，
是因为**一份没跟着代码改的 README 比没有 README 更容易骗人**：它会让下一个人以为不必读代码。

| | |
| --- | --- |
| `src/`（前端） | 72 个文件、约 6.8k 行（42 个生产文件 + 30 个测试与夹具） |
| `src-tauri/src/`（壳） | 30 个文件、约 8.1k 行（21 个生产 + 9 个测试） |
| 命令 | 契约 45 条 · **注册 42 条** · 其中被调用 25 条（清单在 [`src/ipc/command-inventory.ts`](./src/ipc/command-inventory.ts)） |
| 页面 | 4 页（Home / Devices / Plugins / Settings） |
| 前端 store | 5 / 8（devices / pairing / plugins / intents / settings） |

### 已经能跑的

- **进程内是真内核，不是假内核**（[`src-tauri/src/state.rs`](./src-tauri/src/state.rs)）：`SynraDb`、`Identity`、
  `SharedHost`、`Board`、`PluginSet`、`Discovery`、`RingRecorder` 都在 `AppState` 里；扫描是真的 UDP 扫描
  （[`discovery.rs`](./src-tauri/src/discovery.rs)）。
- **插件宿主是真的**（[`plugin_host.rs`](./src-tauri/src/plugin_host.rs)）：平台动作、插件存储、受控 `fetch`、
  审计环都是真的；事件 / 设备发送 / 密钥 / 传输 / 动作组返回 `NOT_IMPLEMENTED`，是**明确的拒绝**而不是静默。
- 前端有类型化 client、`IpcError` 映射与重试分类、`EventDispatcher` + `SequenceTracker`（seq / gap / 未知事件兜底）、
  4 页、集中文案（`src/messages/`），假内核被**关在测试目录里**，有守卫盯着。
- **事件桥是真的**：`src-tauri/src/events.rs` 订阅内核总线并 `emit` 到 `kernel-event`，四个 store（devices / plugins /
  intents / settings）按事件更新；配对、传输、插件、配置、扫描都有发布点。
- **网络那一半在窗口里**：`discovery.rs` 扫、`listener.rs` 收、`transfers.rs` 与 `sync_run.rs` 拨号发；
  插件页有自己的 origin（`synra-plugin://`）、令牌与初始化脚本。
- **插件页有 `window.synra`**：`src/ipc/injection/bootstrap.ts` 构建成 `src-tauri/inject/facade.js`
  （`pnpm run build:facade`），壳把它和 seed 一起拼进 `initialization_script`（seed 是包装函数的参数，不在全局 —— token
  按设计留在闭包里）；`device.list` 已经走 `plugin_ui_rpc`。
  生成物提交进仓库，`pnpm shapes` 的 `facade-fresh` 会重建并逐字节比较。
- 验证：`pnpm test` 235 条（含 29 个挂载测试）、`pnpm run lint` 0 error、`pnpm run build` 通过、`pnpm shapes` 8 项一致；
  Rust 侧 233 条。

### 还没有的（下一步就是这些）

1. **插件页的其余能力**：`window.synra` 的所有命名空间都已接上 `plugin_ui_rpc`；`transfer.*` 现在有**分块通道**
   （`transfer.begin` / `chunk` / `finish` / `abort`，每块 1 MiB，整文件 64 MiB），插件页的 `send` 会自动分块，
   并在**字节交给发送端后**就返回 —— 结束状态用 `status` / `onProgress` 查（19 §8.16）。
2. **插件窗口的收尾**：插件卡上的「打开」按钮已经接上 `plugin_ui_open`，导航范围限制（`on_navigation`）也已装好；
   剩下的是每窗口 CSP —— 受 Tauri 2.12 限制只能继承全局策略。
3. **单实例 / 优雅关停**：单实例锁（`instance.lock`，操作系统文件锁）与「第二次启动唤起已有窗口」（`instance.show`，
   500 ms 轮询）已经做了；退出路径（`ExitRequested`，托盘 Quit 也走它）会取消 listener、discovery、事件桥与 pairing
   监视线程的 token，四个后台线程都会在各自的轮询间隔内退出。
4. **同步的终点**：插件卡上的「Sync to devices」已经接上 `plugins_sync_plan` / `plugins_sync_run`（先读计划再发送）；
   发送侧把已安装目录打成 `.synra`（跳过已解包的 `ui/`），接收侧（窗口与 daemon 都是）先 `install::from_package`
   走完整校验与落盘，再 `record_received` 写历史与对端状态；安装失败时回 `refused` 而不是声称已装。
5. **接收端的接受/拒绝**：已经做了（`file.transfer.request` 的询问步骤、全局提示、拒绝回 abort、按设备“总是接受”），
   设置页可以列出并移除已信任的发送方；忘记设备时信任一并删除；**发送端 abort 时接收端删除半成品并收掉行**。见 19 §8.14 / §8.18。

## 这个仓库装什么

```
synra-app/
├─ package.json / vite.config.ts          # 标准 Vite + pnpm（ADR-012）
├─ src/                                   # 前端：宿主 UI
│  ├─ pages/{Home,Devices,Plugins,Settings}Page.vue
│  ├─ components/                         #   目前一个（ErrorNotice）
│  ├─ stores/devices.ts                   #   ★ 只读镜像：缓存快照 + 应用增量
│  ├─ ipc/                                #   类型化 client、事件分发、注入面
│  └─ messages/                           #   ★ 所有用户可见文案都在这里
└─ src-tauri/                             # 独立 workspace（理由见它的 Cargo.toml 顶部）
   ├─ src/state.rs                        #   AppState：进程内内核
   ├─ src/commands/                       #   命令实现：&AppState -> Result<_, IpcError>，不依赖 Tauri
   ├─ src/shell.rs                        #   唯一一处 #[tauri::command] 与 generate_handler!
   ├─ src/plugin_host.rs                  #   DesktopHost：插件 Host trait 的桌面实现
   ├─ src/discovery.rs                    #   真套接字 + 记录 worker
   └─ tauri.conf.json                     #   identifier = com.synra.app
```

## 它不装什么

| 不装                              | 在哪                                                          |
| --------------------------------- | ------------------------------------------------------------ |
| 内核（发现、传输、协议、插件宿主）    | [`synra-core`](https://github.com/synra-rs/synra-core)         |
| Wasm 运行时（wasmtime）            | 同上（`crates/synra-plugin-host/`，本仓库只开 feature 引入）       |
| 插件校验 / 签名的**逻辑**            | 同上（本仓库只做 UI 承载与令牌注入）                               |
| `@synra/client` 等类型包            | [`synra-js`](https://github.com/synra-rs/synra-js)              |

## 与 core 的边界（判据）

> **如果一个功能在"没有 UI"的情况下也需要工作，它属于 core。**

| 属于 core                              | 属于本仓库                                        |
| -------------------------------------- | ------------------------------------------------- |
| `KernelEvent` 枚举（事件的事实来源）       | 事件 → Tauri `emit` 的桥、前端分发                  |
| `SynraError` + 稳定错误码                 | 面向 UI 的错误映射 + 可操作建议                       |
| UI 插件**会话令牌的签发 / 校验**            | 令牌的**注入方式**（initialization script）、CSP      |
| 插件 artifact 的落盘与校验                 | 插件 WebView 的创建与销毁                           |
| 意图路由（匹配 / 决策 / 下发）              | 候选的展示与用户选择 UI                              |

**两条硬性检查**（`pnpm shapes` 已实现）：

1. `src-tauri/src/inject/` 的注入对象必须 `satisfies SynraClient` —— **漏字段/多字段在编译期报错**；
2. 本仓库**不得**出现第二处 `SynraResponse` / `SynraError` 声明（类型只在 `@synra/client` 里定义一次）。

## 运行

```powershell
pnpm install
pnpm test             # vitest（219 条；默认 node 环境，页面各有一个 jsdom 挂载测试）
pnpm run build:facade # 生成 src-tauri/inject/facade.js —— Rust 侧 include_str! 它
pnpm build            # vue-tsc -b + vite build
pnpm shapes           # 8 条守卫（含 facade-fresh：重建门面并逐字节比较）

pnpm tauri dev       # 真正跑起来：需要 WebView2（Windows）+ Rust 1.90+
```

`src-tauri` 是**独立 workspace**，它的逻辑测试不编译 Tauri，所以这是最快的一条反馈路径：

```powershell
cd src-tauri
cargo test --no-default-features
```

`@synra/client` / `@synra/protocol` / `@synra/theme` 在本地通过 `pnpm-workspace.yaml` 的 `overrides: link:`
指向同级的 `synra-js/`（理由写在那份文件里）。CI 里同样用 `link:` —— 三个仓库并排 checkout —— 所以那三行
`overrides` 在两边都是真的；要换成版本依赖是**发布**时的事，不是 CI 的事。

## 已知不一致

- **包标识符**：`src-tauri/tauri.conf.json` 是 `com.synra.app`，[设计文档拍板](https://github.com/synra-rs/design-docs/blob/main/README.md)
  的是 `dev.synra-app.app`。发布前必须定一个 —— Android 的 `applicationId` 发版后改不了，而改它要重新生成
  `gen/android`。
- **没有 CI 之外的东西**：`.github/workflows/ci.yml` 会跑 `pnpm test` / `build` / `shapes` 与
  `cargo test --no-default-features`，但它**不跑** `pnpm tauri build`（需要 WebView2 与平台依赖），
  也不做端到端 —— 打包产物仍然是手工的。

## 下一步

1. 事件桥（`KernelEvent` → `emit`），否则所有页面只能轮询；
2. 补 `pairing_*` 与 `transfer_*` 命令，再补对应的页面；
3. 插件页把已有的 `plugins_enable/disable/consent_respond/audit` 接上界面；
4. 插件 UI 承载（第二 WebView / origin / 令牌 / 注入脚本）—— 这是 M4 的关键验收，做之前先跑
   [设计文档 19 §0.1](https://github.com/synra-rs/design-docs/blob/main/19-roadmap.md) 的 spike 5 与 6。

设计依据：[08 IPC 契约与前端集成](https://github.com/synra-rs/design-docs/blob/main/08-ipc-and-frontend.md)、
[28 App UI 规范](https://github.com/synra-rs/design-docs/blob/main/28-app-ui.md)、
[23 客户端库](https://github.com/synra-rs/design-docs/blob/main/23-client-library.md)。

## 许可

MIT
