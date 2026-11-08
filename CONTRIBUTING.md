# 应用开发

## 职责与依赖

本仓库承载 Tauri 壳、Vue 管理界面和插件页面。需要脱离界面也成立的规则放入共享核心。
内核事件只由 `src/events.ts` 订阅，页面读取 store；IPC 类型来自 `@synra/protocol`。

将 `synra-rust-core`、`synra-js` 与本仓库并排检出。
Cargo 与 pnpm 的本地路径依赖见 `src-tauri/Cargo.toml`、`pnpm-workspace.yaml`；
它们是当前开发配置，单独克隆应用仓库还不足以安装所有依赖。

## 本地运行

使用根目录 `rust-toolchain.toml` 固定的 Rust；Node 与 pnpm 版本参照 `.github/workflows/ci.yml`。
按 Tauri 要求安装平台依赖，Windows 使用 WebView2；Android 需要 JDK、Android SDK 与 NDK。

```powershell
rustup toolchain install --no-self-update
pnpm install
pnpm exec tauri dev
```

`pnpm dev` 运行浏览器预览；它使用隔离的假内核，不能证明真实网络或原生能力。

## 检查

```powershell
pnpm test
pnpm run lint
pnpm run build
pnpm run shapes
cargo test --no-default-features --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

生产构建同时生成注入门面，`facade-fresh` 会核对提交的生成物。
默认特性的 Cargo 检查包含 Tauri 壳；无默认特性测试覆盖不依赖窗口的逻辑。

### IPC 清单

注册 57 条命令，被调用 44 条。守卫将这里的数量与实际壳注册、前端调用对照。
具体清单见 [command-inventory.ts](./src/ipc/command-inventory.ts)，不要在产品介绍中复制实现数量。

## 构建

```powershell
pnpm exec tauri build
pnpm exec tauri android build --apk --target aarch64 --ci
```

应用标识符以 `src-tauri/tauri.conf.json` 的 `com.synra.app` 为准。
Android 工程源码随版本管理；生成桥接、SDK 路径、构建产物和签名密钥继续忽略。
调试构建不要求本地 release 密钥；分发包应另行验证签名与目标 ABI。

## 插件集成验收

构建独立插件仓库的 `dist/` 后，可运行实际包验收：

```powershell
$env:SYNRA_STARTER_PACKAGE = (Resolve-Path ../synra-plugin-starter/dist).Path
cargo test --no-default-features --features official-plugin-tests --manifest-path src-tauri/Cargo.toml official_starter --lib
$env:SYNRA_TOUCHPAD_PACKAGE = (Resolve-Path ../synra-plugin-touchpad/dist).Path
cargo test --no-default-features --features touchpad-plugin-tests --manifest-path src-tauri/Cargo.toml official_touchpad --lib
```

这些 opt-in 检查覆盖生产安装、授权、启用、页面 RPC 和 Wasm；输入验收替换最后的平台适配器。
它们分别证明调用链，不能代替实体手机、原生窗口或物理鼠标验收。

### Starter 真实桌面验收

先按独立 Starter 仓库的 README 构建 `dist/`，并确保本仓库前端与 facade 已构建。
以下入口只存在于启用 `official-plugin-tests` 的 desktop debug 构建，通过生产命令安装、
授予本机 debug 声明与设备列表读取能力、启用并打开真实插件窗口。

```powershell
$taskPreviousTauriConfig = $env:TAURI_CONFIG
try {
  $env:TAURI_CONFIG = '{"identifier":"com.synra.ui-smoke","plugins":{"deep-link":{"desktop":{"schemes":[]}}}}'
  cargo build --locked --manifest-path src-tauri/Cargo.toml --bin synra-app --features official-plugin-tests -j 1
}
finally {
  $env:TAURI_CONFIG = $taskPreviousTauriConfig
}
$taskStarterDist = (Resolve-Path ../synra-plugin-starter/dist).Path
./src-tauri/target/debug/synra-app.exe --starter-ui-smoke $taskStarterDist
```

入口在数据库初始化之前验证独立 identifier 和空链接 schemes，拒绝在正常应用配置下自动安装。
空验收 profile 执行安装；只有 Starter fixture 时通过生产 update 入口替换，便于重复验收而不丢弃本机身份。
包含其他插件时拒绝自动变更；也可不带参数重开实例，再从插件页打开 Starter。
它保留独立的应用数据与本机密钥以便复查，不能当作手机到电脑操作验收。
普通开发时重新构建默认配置，避免继续运行这个独立 profile 的二进制。

## 修改与提交

先记录工作区基线，保留已有用户改动；多行修改使用编辑工具，格式化限定在本轮文件。
按改动范围运行检查，读取结果后再提交。提交使用 GPG 签名、英文 Conventional Commit、
不超过 50 字符的 subject，body 留空。

设计决定记录在设计仓库的状态账本；可复用经验放入 [ai-docs/good-practices](./ai-docs/good-practices/README.md)。
