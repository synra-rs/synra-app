# Synra

Synra 让设备通过插件协作：在手机上打开一个插件，选择电脑，再由电脑完成对应操作。
软件本体负责设备连接、插件管理和授权，具体功能由插件提供。

[使用文档](https://synra-app.dev/zh/guide/introduction) · [快速上手](https://synra-app.dev/zh/guide/quick-start) · [问题反馈](https://github.com/synra-rs/synra-app/issues)

## 可以做什么

- 在局域网发现设备，通过两端相同的配对码和接收端确认建立配对。
- 从本地目录或已配置的注册表安装插件，预览来源与权限，并决定是否信任发布者的签名密钥。
- 安装、确认、启用和管理插件，查看权限、版本、配额及最近调用。
- 检查更新，并按策略接收通知或自动安装已验证且未新增权限的版本。
- 将插件同步到已配对设备，查看任务状态并处理接收确认。
- 打开独立插件页面：桌面使用新窗口，Android 提供宿主返回栏和系统返回。
- 查看连接、传输与运行日志，管理设置和已信任的接收来源。

例如，Touchpad 插件提供手机触摸区域，电脑同意本次连接后才接受鼠标操作。
插件授权与设备配对分别管理，需要逐次确认的操作会另行询问。

## 开始使用

1. 在参与协作的设备上运行相互匹配的 Synra 构建，并保持应用运行。
2. 在“设备”中扫描并发起配对，核对两端配对码，由接收端同意。
3. 安装所需插件，确认权限并启用；跨设备功能通常需要两端安装对应插件。
4. 打开插件，选择目标设备，按插件提示完成操作。

当前为开发版本，已构建 Windows 桌面程序与 Android ARM64 APK。
其他系统的应用分发与原生能力请以[平台说明](https://synra-app.dev/zh/reference/platform-matrix)为准。

> 当前设备通信使用明文 TCP，配对码用于人工核对，不提供传输加密或密码学身份认证。请在可信局域网使用。

## 项目组成

| 项目 | 作用 |
| --- | --- |
| [synra-core](https://github.com/synra-rs/synra-core) | 无界面的 Rust 内核、设备通信、插件运行时与作者工具 |
| 本仓库 | 基于 Tauri 与 Vue 的桌面和移动端应用，承载通用管理界面与插件页面 |
| [synra-js](https://github.com/synra-rs/synra-js) | 插件页面客户端类型、共享协议、测试工具和主题 |
| [Starter](https://github.com/synra-rs/synra-plugin-starter) / [Touchpad](https://github.com/synra-rs/synra-plugin-touchpad) | 独立插件示例与实际功能 |

## 参与开发

本地运行、构建、检查和插件验收见 [CONTRIBUTING.md](./CONTRIBUTING.md)。
本轮总结的协作方法见[开发经验](./ai-docs/good-practices/README.md)。

## 许可

MIT
