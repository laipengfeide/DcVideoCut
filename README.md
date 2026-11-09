# Dc Video Cut

一个基于 **Tauri 2 + Rust + Vue 3** 的视频抽帧与切割桌面工具（Windows），数据存储使用 SQLite。

## 功能

### 1. 视频抽帧
- 把视频按指定间隔提取成一张张图片（0.5s / 1s / 2s / 5s / 10s 或自定义间隔）
- 缩略图网格预览，点击勾选想要的帧
- 支持保存为 **JPG**（可调质量）或 **PNG**
- 自定义输出目录，文件名自动带上序号和时间点

### 2. 视频切割
- 在时间轴上**插入切割点**：插 1 个点切成 2 段，插 2 个点切成 3 段，依次类推
- 支持**手动输入**开始 / 结束时间来精确切割（支持 `12.5`、`00:12.5`、`01:02:03`、`1h2m3s` 等格式）
- 时间轴带视频缩略图条、播放头拖动、切割点拖动 / 双击插入 / 右键删除
- 两种导出模式：
  - **快速切割**（`-c copy`）：不重新编码，秒级完成、无画质损失，起点对齐关键帧
  - **精确切割**（H.264 重编码）：帧级精准，速度较慢
- 实时进度条，任务可随时取消

### 3. 账号与配额（后端 Rust 强制校验，不仅靠前端）

| 身份 | 切割点数量 | 抽帧视频大小 | 单视频上限 |
|------|-----------|-------------|-----------|
| 游客 | 最多 2 个（切成 3 段） | ≤ 30 MB | 2 GB |
| 注册用户 | 不限 | 不限 | 2 GB |

- 注册 / 登录基于 **Argon2** 密码哈希，会话 token 持久化 30 天
- 数据库为 SQLite，位于 `%APPDATA%\com.dc.videocut\dcvc.db`
- 记住上次输出目录，下次打开自动带出

## 环境要求

- **Node.js** ≥ 16（项目使用 Vite 4，兼容 Node 16）
- **Rust** ≥ 1.77.2（依赖已锁定为 MSRV 1.77.2 的版本组合，rustc 1.89 实测通过）
- **ffmpeg / ffprobe**：程序按以下顺序查找
  1. 环境变量 `DCVC_FFMPEG_DIR` 指定的目录
  2. 打包资源内的 `ffmpeg/bin`
  3. exe 同级的 `ffmpeg\bin`、`bin`、`ffmpeg` 目录
  4. 系统 PATH

## 如何运行（开发模式）

```powershell
cd "Dc Video Cut"
npm install          # 安装前端依赖
npm run app:dev      # 启动 Vite + Tauri 开发窗口
```

首次运行会自动创建数据目录与 SQLite 数据库。

## 如何打包

```powershell
npm run app:build
```

产物位置：

- 可执行文件：`src-tauri\target\release\dc-video-cut.exe`（可直接双击运行）
- NSIS 安装包：`src-tauri\target\release\bundle\nsis\Dc Video Cut_0.1.0_x64-setup.exe`

> 提示：如果构建时报 `拒绝访问 (os error 5)`（个别环境下 cargo 写 D 盘 target 目录受限），把 target 目录指到别的盘即可：
>
> ```powershell
> $env:CARGO_TARGET_DIR="C:\build\dcvc"
> npm run app:build
> ```

分发到没有 ffmpeg 的机器时，把 `ffmpeg.exe`、`ffprobe.exe` 放到 exe 同级的 `ffmpeg\bin\` 目录即可，程序会自动识别。

## 项目结构

```
Dc Video Cut/
├── src/                    # Vue 3 前端
│   ├── components/
│   │   ├── AuthView.vue        # 登录 / 注册 / 游客入口
│   │   ├── WorkspaceView.vue   # 主工作区（拖拽选视频、信息栏）
│   │   ├── ExtractPanel.vue    # 抽帧面板
│   │   ├── CutPanel.vue        # 切割面板
│   │   ├── Timeline.vue        # 时间轴组件
│   │   └── ToastHost.vue
│   ├── api.ts              # invoke 封装与类型定义
│   ├── format.ts           # 时间 / 大小格式化与解析
│   ├── progress.ts         # 全局进度事件监听
│   └── styles.css          # 深色 studio 主题设计系统
└── src-tauri/              # Rust 后端
    └── src/
        ├── lib.rs              # 应用入口、插件与命令注册
        ├── auth.rs             # SQLite、注册登录、会话、配额常量
        ├── commands.rs         # 全部 Tauri 命令（探测/抽帧/保存/切割/取消…）
        ├── ffmpeg.rs           # ffmpeg/ffprobe 探测、进度解析
        └── models.rs           # 前后端共享数据结构
```

## 技术要点

- 视频处理通过外部 `ffmpeg` CLI 完成，重活跑在 `spawn_blocking` 线程，不阻塞 UI
- 进度通过读取 ffmpeg 的 `-progress pipe:1` 输出并向前端 emit `job-progress` 事件
- 抽帧任务缓存于 `%LOCALAPPDATA%\com.dc.videocut\`，旧的帧缓存会自动清理
- 切割导出单次最多 40 段（防误操作），游客最多 3 段
