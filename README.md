# 侧载助手 (SideloadHelper)

一个精简版的 iOS 侧载工具,专门用来把 **SideStore** 装进 iPhone / iPad。

配套图文教程:<https://sideloadstore.pages.dev>

## 这个版本做了什么裁剪

相比上游的 iloader,这个版本只保留新手真正需要的东西:

| | 上游 iloader | 本版本 |
|---|---|---|
| 安装器 | SideStore 稳定版 / 每夜版 / LiveContainer 合并版 ×2 / 导入 IPA | **只有 SideStore 稳定版** |
| 高级页面 | 证书、App ID 管理 | 已移除 |
| 自动更新 | 指向上游仓库 | **已完全移除** |
| 默认语言 | 英文 | **简体中文** |

去掉 LiveContainer 合并包是有意为之:合并包安装时存在 keychain 权限分配问题,
会让 LiveContainer 的多开功能失效。正确做法是先用本工具装好 SideStore,
再在手机上用 SideStore 安装 LiveContainer。

移除自动更新也是必须的——上游的更新地址指向原作者仓库,
不移除的话本版本装到用户电脑后会自动"更新"回原版,裁剪全部失效。

## 开发

需要 [Rust](https://rustup.rs) 和 [bun](https://bun.sh)。

```bash
bun install
bun run tauri dev
```

构建:

```bash
bun run tauri build
```

> 前端脚本用 `bunx --bun vite` 运行,因为 Vite 8 需要 Node 20+,
> 走 bun 自己的运行时可以避开本机 Node 版本过旧的问题。

Windows 版无法从 macOS 交叉编译,由 GitHub Actions 构建
(推送到 `main` 或手动触发 workflow 即可)。

## 致谢与许可

本项目 fork 自 [nab138/iloader](https://github.com/nab138/iloader),
原项目代码以 **MIT 许可证** 发布,版权归原作者所有(见 [LICENSE](LICENSE))。

本项目**未使用**原项目的名称与图标资源,采用了独立的名称和图标,
因此不受原项目[品牌条款](LICENSE-BRANDING)约束,
也**与原作者无任何隶属或背书关系**。

本项目的修改部分同样以 MIT 许可证发布。
