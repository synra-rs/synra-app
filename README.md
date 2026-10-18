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
| `src/`（前端） | 57 个文件、约 5.8k 行（36 个生产文件 + 21 个测试与夹具） |
| `src-tauri/src/`（壳） | 17 个文件、约 5.2k 行（14 个生产 + 3 个测试） |
| 命令 | 契约 41 条 · **注册 33 条** · 其中被调用 10 条（清单在 [`src/ipc/command-inventory.ts`](./src/ipc/command-inventory.ts)） |
| 页面 | 4 页（Home / Devices / Plugins / Settings） |
| 前端 store | 1 / 8（只有 `devices`） |

### 已经能跑的

- **进程内是真内核，不是假内核**（[`src-tauri/src/state.rs`](./src-tauri/src/state.rs)）：`SynraDb`、`Identity`、
  `SharedHost`、`Board`、`PluginSet`、`Discovery`、`RingRecorder` 都在 `AppState` 里；扫描是真的 UDP 扫描
  （[`discovery.rs`](./src-tauri/src/discovery.rs)）。
- **插件宿主是真的**（[`plugin_host.rs`](./src-tauri/src/plugin_host.rs)）：平台动作、插件存储、受控 `fetch`、
  审计环都是真的；事件 / 设备发送 / 密钥 / 传输 / 动作组返回 `NOT_IMPLEMENTED`，是**明确的拒绝**而不是静默。
- 前端有类型化 client、`IpcError` 映射与重试分类、`EventDispatcher` + `SequenceTracker`（seq / gap / 未知事件兜底）、
  4 页、集中文案（`src/messages/`），假内核被**关在测试目录里**（`tests/browser`），有守卫盯着。
- 验证：`pnpm test` 139 条、`pnpm build` 通过、`pnpm shapes` 7 条守卫通过；Rust 侧测试在
  `commands/{devices,diag,discovery}`。

### 还没有的（下一步就是这些）

1. **事件桥**：壳一次 `emit` 都没有（没有 `events.rs`），所以前端的事件分发器**没有生产者**，界面只能靠刷新快照。
2. **19 条命令**：`pairing_*`（3）、`transfer_*`（4）、`devices_connect`/`disconnect`、8 条插件生命周期
   （catalog / install / uninstall / update / sync）、`plugin_ui_rpc` / `plugin_ui_ready`。
3. **插件 UI 承载没有 Rust 那一半**：没有第二个 WebView、没有插件 origin、没有令牌签发、没有注入脚本。
   现在只有一个单窗口 + `default-src 'self'`。
4. **没有托盘 / 单实例 / 优雅关停**（没有 `lifecycle.rs`），也没有 `tauri-plugin-*`。
5. **没有组件测试**：vitest 是 `environment: 'node'`，139 条测试里没有一条渲染页面。
   CI 已建（`.github/workflows/ci.yml`），但它跑的就是这 139 条 —— 所以它**不能**被读成"页面能渲染"的证据。

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
pnpm test            # vitest（169 条；默认 node 环境，四个页面各有一个 jsdom 挂载测试）
pnpm build           # vue-tsc -b + vite build
pnpm shapes          # 注入面 / 命令名 / 类型唯一性等 7 条守卫

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
