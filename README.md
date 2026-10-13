# synra-app

> **应用仓库。** Tauri 2 桌面端 + 移动端，以及宿主 UI 与插件 UI 的承载。
>
> 这是 **`synra` 三仓库拓扑**中的第 2 个（[设计文档 §3](https://github.com/synra-rs/design-docs)）：
>
> | 仓库                                                     | 装什么                                     |
> | -------------------------------------------------------- | ------------------------------------------ |
> | [`synra-core`](https://github.com/synra-rs/synra-core)     | Rust 内核 + CLI + 插件工具链                  |
> | **`synra-app`** ← 本仓库                                   | Tauri 应用（桌面 + 移动 + 宿主 UI）             |
> | [`synra-client`](https://github.com/synra-rs/synra-client) | 插件 UI 的类型库（`@synra/client` 等）          |
> | [`design-docs`](https://github.com/synra-rs/design-docs)   | 设计文档（唯一权威）                           |
>
> ⚠️ **尚未开始实现。** 目前只有这份 README。
> 计划先在 M0 用**假内核**把四页壳层跑起来（不等 core），详见「下一步」。

**域名**：[synra-app.dev](https://synra-app.dev)（官网 + 文档 + schema 托管）

---

## 这个仓库将装什么

```
synra-app/
├─ package.json / vite.config.ts          # 标准 Vite + pnpm（ADR-012）
├─ src/                                   # 前端：宿主 UI
│  ├─ pages/{home,plugins,devices,settings}/
│  ├─ components/{base,layout,discovery,plugins}/
│  ├─ stores/{ui,devices,pairing,plugins,intent,transfers,config,diagnostics}
│  ├─ ipc/                                #   生成的绑定（ts-rs）+ 类型化 client
│  └─ generated/                          # ★ 生成物，禁止手改（CI 有 types-check）
└─ src-tauri/
   ├─ Cargo.toml                          #   依赖 synra-core（发布前用 git+tag）
   ├─ tauri.conf.json                     #   identifier = dev.synra.app
   ├─ capabilities/                       #   Tauri 自身权限（与插件 capability 无关）
   └─ src/
      ├─ main.rs                          #   ≤ 60 行
      ├─ state.rs                         #   AppState { kernel: Arc<Kernel>, ui_sessions }
      ├─ commands/                        #   ~40 个 #[tauri::command]（薄适配层）
      ├─ events.rs                        #   KernelEvent → Tauri emit 的桥
      ├─ platform.rs                      #   trait Platform 的 GUI 侧实现
      ├─ plugin_ui.rs                     #   ★ UI 插件 WebView/origin/CSP/令牌注入
      ├─ inject/                          #   ★ 注入 window.synra（必须 satisfies SynraClient）
      └─ lifecycle.rs                     #   启动顺序、托盘、单实例、优雅关停
```

## 它不装什么

| 不装                              | 在哪                                                          |
| --------------------------------- | ----------------------------------------------------------- |
| 内核（发现、传输、协议、插件宿主）    | [`synra-core`](https://github.com/synra-rs/synra-core)         |
| Wasm 运行时（wasmtime）            | 同上（`crates/synra-plugin-host/`）                             |
| 插件加载 / 校验 / 签名的**逻辑**     | 同上（本仓库只做 UI 承载与令牌注入）                              |
| `@synra/client` 等类型包            | [`synra-client`](https://github.com/synra-rs/synra-client)     |

## 与 core 的边界（判据）

> **如果一个功能在"没有 UI"的情况下也需要工作，它属于 core。**

| 属于 core                              | 属于本仓库                                        |
| -------------------------------------- | ------------------------------------------------- |
| `KernelEvent` 枚举（事件的事实来源）       | 事件 → Tauri `emit` 的桥、前端分发                  |
| `SynraError` + 稳定错误码                 | 面向 UI 的错误映射 + 可操作建议                       |
| UI 插件**会话令牌的签发 / 校验**            | 令牌的**注入方式**（initialization script）、CSP      |
| 插件 artifact 的落盘与校验                 | 插件 WebView 的创建与销毁                           |
| 意图路由（匹配 / 决策 / 下发）              | 候选的展示与用户选择 UI                              |

**两条硬性检查**（CI）：

1. `src-tauri/src/inject/` 的注入对象必须 `satisfies SynraClient` —— **漏字段/多字段在编译期报错**；
2. 本仓库**不得**出现第二处 `SynraResponse` / `SynraError` 声明（类型只在 `synra-client` 里定义一次）。

## 下一步

1. 读 [`design-docs`](https://github.com/synra-rs/design-docs) 的
   [README §2 已拍板决策](https://github.com/synra-rs/design-docs/blob/main/README.md)；
2. 读 [08 IPC 契约与前端集成](https://github.com/synra-rs/design-docs/blob/main/08-ipc-and-frontend.md)（整篇归本仓库）；
3. 读 [13 插件写法 §5](https://github.com/synra-rs/design-docs/blob/main/13-plugin-authoring.md)（UI 部分）；
4. 读 [23 客户端库](https://github.com/synra-rs/design-docs/blob/main/23-client-library.md)（注入对象的契约）；
5. **M0**：用假内核把四页壳层跑起来（不等 core 完成）。

## 许可

MIT
