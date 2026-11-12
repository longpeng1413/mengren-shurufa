# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 仓库现状

本工作区为基于青简改版的猛人输入法，当前产品版本为 Android / Windows v0.4.7（2026-11-12，Windows 自定义短语页新增与导入导出移到最上方、安卓更新取消确认后可重试并显示 GitHub 下载页；0.4.6 起支持纯数字输入码）。Core 平台无关，各平台只做壳；下面的 crate 地图及上游 macOS/Linux 资料继续作为架构参考，不表示本轮要扩展所有平台。
当前范围见 `docs/design/mengren-public.md`，猛人进度与剩余项见 `docs/plan/todo.md`；`docs/plan/roadmap.md` 主体为上游路线图。2026-11-07 v0.3.8 发布后，用户确认原有手机到另一手机及桌面设备的私密路线通过；新增 Windows 设备的普通/私密互传尚未联通，用户已确认仅看见附近/中转设备，尚未配对；先逐对完成六位码配对。0.4.0 修复 Windows 小键盘配对码输入顺序，并加入自定义短语同步及紧凑竖排剪贴板。源码没有三台设备上限，六台逻辑设备中转互传扩容测试已通过，不将逻辑测试当作新增真实设备已通过。此前 v0.3.7 故障历史见 `docs/notes/private-transfer-037-review.md`。

## 目录地图

一行一个，只说它是什么、入口在哪；实现要点（数据文件、常数、生成命令）在 `docs/notes/crate-notes.md`，改了实现要同步那里。

- `crates/qingjian-core`：引擎。`Engine` 是对外唯一门面，`Translator` / `Learner` 等 trait 在 `engine` 模块；拼音解析、纠错、候选、排序、整句、双拼、注音、九宫格（`nine_key`）、英文模式、繁体输出、emoji 与符号都在这里。
- `crates/qingjian-dictionary`：词库（TSV 或 `.qj` mmap），按音节位置二分查询。
- `crates/qingjian-translate`：释义表 `Glossary`、词汇等级表 `LevelTable`。
- `crates/qingjian-learning`：用户侧落盘：词频 / 用户词 / 个人 n-gram / 敲错表（`FrequencyLearner`）、输入日志（`InputLog`）、输入统计（`UsageStats`）、词汇记录（`VocabularyBook`）。
- `crates/qingjian-predict`：云联想 `CloudPredictor`（OpenAI 兼容接口）与释义兜底 `CloudGlossFiller`；`PredictConfig` 是 `[predict]` 分节。
- `crates/qingjian-voice`：云端语音识别，实现 Core 的 `Transcriber`；现在是火山引擎豆包流式语音识别 `VolcengineTranscriber`（WebSocket + rustls，后台线程）。
- `crates/qingjian-voice-offline`：本机离线语音识别，实现 Core 的 `Transcriber`：sherpa-onnx（运行时加载）跑 SenseVoice，含模型下载、校验、清理；安卓「语音识别」的「本机离线」选项用它。
- `crates/qingjian-lm`：整句转换的 bigram 语言模型 `BigramModel`，另有读知意 `zh.ngram` 的 `TrigramModel` 与插值用的 `MixedModel`。
- `crates/qingjian-neural`：本地神经模型推理，`hanzhang/`（含章字级 Transformer：`CharScorer` 打分、`P2c` 按拼音生成整句）与 `zhiyi/`（知意选词模型）。
- `crates/qingjian-format`：`.qj` 数据容器（mmap 读、零拷贝视图、写入器、哈希索引）。
- `crates/qingjian-platform`：平台层共用：`Config`（TOML 配置）、`extra_dictionaries`、`protocol` 线上类型（Windows Server ↔ DLL，以及 `protocol::linux` 的 Server ↔ 各前端）。
- `crates/qingjian-render`：候选窗与状态条：帧数据 + 主题 → 场景树，画在 bamboo-slip 上，各平台只贴图。见 `docs/design/rendering.md`。
- `crates/bamboo-slip`：自绘 UI 引擎（场景树、布局、文字、动画 → 位图，另有控件库与窗口宿主），候选窗与设置程序共用；不放任何青简的概念，约定见该目录的 `CLAUDE.md`。双许可，见该目录的 `LICENSE.md`。
- `crates/bamboo-slip-i18n`：界面文案多语言：各 crate 的 `locales/*.toml` 编译时嵌入，`tr` 按进程级的界面语言查，回退英文再回退中文。双许可，见该目录的 `LICENSE.md`。
- `crates/qingjian-update`：更新：读猛人 GitHub 的签名索引，按平台挑新版、下载并校验安装包，壳负责系统安装；发版侧的签名工具是 `tools/release-sign`。见 `docs/design/update.md`。
- `crates/qingjian-code`：认出一段文字像不像代码并切高亮片段，输入法与剪贴板共用；core 里转出为 `qingjian_core::code`。
- `crates/qingjian-clipboard`：剪贴板历史存储（SQLite，留好同步要用的字段）与面板位置，平台无关，不放窗口与界面。
- `crates/qingjian-clipboard-sync`：剪贴板的设备间同步：配对、加密通道、合并、发现与扫描，不依赖具体存储；Windows 与 Android 共用。
- `apps/settings`：设置程序（三平台共用，画在 bamboo-slip 上，读写同一份配置文件），页面与设置项按 `docs/design/settings.md`；macOS 装在 `Qingjian.app/Contents/Applications/`，Windows、Linux 与 Server 同目录。`--png <目录>` 拷一份配置按脚本截图。
- `apps/clipboard`：青简剪贴板（独立进程，Windows 与 macOS）：监听复制、全局快捷键呼出卡片面板、粘贴回原窗口，画在 bamboo-slip 上；系统相关的在 `src/system/{windows,macos}/`，霓虹灯的节奏与像素在共用的 `src/system/neon/`。macOS 版是输入法包里的 `QingjianClipboard.app`，由输入法按配置拉起。
- `apps/cli`：Core 的验证工具：查询、逐键计时、输入日志回放、整句评测、常数扫描。排序 / 整句 / 纠错的改动先跑它再合。
- `apps/macos`：IMK 壳，按 `app / host / imk / candidates / menubar / preferences` 分目录（`preferences` 是旧版设置，新版 `apps/settings` 为主、旧版只作兜底）；`scripts/bundle.sh --install` 装到本机，`--pkg` 出分发包。
- `apps/windows`：`server`（Server 进程：Engine + IPC + 自绘候选窗与状态条）+ `tsf`（TSF DLL）+ `settings`（旧版设置，WinUI 3，只作兜底，新版 `apps/settings` 为主）+ `installer`（Inno）。DLL 不能带 Engine 的依赖树，所以是两个 package。
- `apps/linux`：`server`（Engine + Unix socket）+ `fcitx5` / `fcitx4`（框架事件、默认面板）+ `ibus`（第二个前端：IBus，按键 / 焦点 / 面板已通，对着真 daemon 验过）+ `scripts`（用户级安装卸载、`package.sh` 打 Fcitx5 预编译包）+ `packaging`（`build.sh` 在容器里打 deb / rpm，见 `docs/notes/linux-packaging.md`）；Server 手动启动或装成 systemd 用户服务，见 `docs/notes/linux-fcitx5.md` 和 `docs/notes/linux-fcitx4.md`。
- `apps/android`：`core`（Rust crate：Engine 装配 + `Session` + JNI，编成 `libqingjian_android.so`）+ `app`（Kotlin `InputMethodService` 壳，自绘键盘，候选条贴渲染器位图）；Gradle 构建时自动跑 `cargo ndk`，见 `apps/android/README.md`。
- `tools/dict-convert`、`tools/gloss-gen`、`tools/corpus`：产品数据生成（词库 / 语言模型 / 释义表 / emoji / 英文词表），输出到 `data/generated/`（gitignore）。
- `assets/`：随包数据源与样例，各目录有 README 写来源与许可。雾凇拼音（GPL）已彻底移除，不要再引入。

`docs/` 分四类（索引在 `docs/README.md`）：`design/` 设计与决定、`plan/` 路线与待办、`notes/` 工程记录（性能、复盘、踩坑、crate 实现要点）、
`user/` 用户文档（官网构建时拉取渲染，约定见 `docs/user/README.md`，措辞面向用户、不出现实现词）。

## 常用命令

```bash
cargo build                                   # 整个 workspace
cargo test                                    # 全部测试；-p <crate> 单个，加测试名过滤
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo run -p qingjian-cli -- <拼音>...          # Core 的主要验证方式；--replay / --eval-text 见 crate-notes
apps/macos/scripts/bundle.sh --install        # mac 壳装到 ~/Library/Input Methods/（IMK 不能 cargo run 验证）
```

本机 Windows 使用 MSVC 工具链构建，猛人两端构建入口为 `mengren/build.ps1`；Android 的中文路径与独立缓存约束见本机私人开发笔记。macOS 端到端验证可用 `osascript` 往 TextEdit 发按键再读回文本，相关部署见各平台 README。

## 约定

架构约束、代码组织、版本号、提交信息、文档同步、提交前检查与发版都在 `docs/contributing.md`，随本文件一起载入：

@docs/contributing.md

交流用中文。


## 猛人输入法发版边界

- 2026-11-07 用户已明确恢复打包与发布授权：发布 v0.3.8 供手机/电脑通过检查更新进行真机复测，替代此前暂停要求。保留原 v0.3.7 标签与附件，不将本版编译/自动测试通过写成真机故障已全面解决。
- 本机主仓库 origin 指向 `longpeng1413/mengren-shurufa-private-archive`（私有）。公开仓库为 `longpeng1413/mengren-shurufa`，只接受审查后的独立源码变更；禁止把主仓库的分支、历史或 --mirror 推过去。
- **按平台独立发版（2026-11-08 用户最新要求）：哪边有实际改动，哪边才递增产品版本、构建和发布。** 仅安卓改动不升级 Windows，仅 Windows 改动不升级安卓；未改动平台保持原版本和原安装包，不为统一版本号、发布标签或满足旧脚本检查而重新打包。正式包不用 -dev；不跳过必要检查，不把编译通过称为真机验收。
- 共享代码改动先评估各平台实际运行行为和兼容性，只发布受影响的平台；文件位于共享 crate 本身不是两端同时发版的理由。确实影响两端时分别验证后发布，版本号仍独立管理。协议变化须说明最低兼容版本、旧端是否仍可互传及必要升级范围，不能仅按两端版本号是否相同判断兼容。
- 构建时显式选平台：安卓用 `mengren/build.ps1 -Android`，Windows 用 `mengren/build.ps1 -Windows`；只有两端都需交付时才使用默认的两端构建。每个平台随包组件内部保持安装器要求的版本一致，跨平台不要求相同。
- 发布说明和交接记录分别列出 Android / Windows 的版本与实际变化；只改文档不触发软件版本递增或安装包发布。已发布的 0.4.4 两端包是历史事实，保留标签和附件，不回退、不重发。
- **发布工具已按平台独立适配（2026-11-09，用于 0.4.5）。** `mengren/release.py` 必须用 `--platform android` / `--platform windows` 指明本次发版平台（两端都发写两次），只校验、只上传所选平台的新包；签名索引以 GitHub latest 上一版已验签索引为底稿，把本版加在最前，其他平台原有版本、下载地址和摘要原样保留。混合版本的客户端挑选由 `qingjian-update` 的测试覆盖，要求见 `docs/notes/release.md`。
- 通过相关检查后，沿用 `mengren/release.py` 的源码导出、隐私扫描、独立公开历史追加、签名和草稿附件校验流程。脚本须保留 GitHub 独立修改，遇同文件冲突先合并，禁止覆盖或强推。
- 更新仅来自猛人仓库，信任猛人专用公钥。签名私钥、安卓签名备份、私人说明和早期设计素材不公开；私钥在 `mengren/private/` 以 DPAPI 加密保存，不打印、不提交，不换掉已安装 APK 的签名。
- 每次新包成功并校验后，本机安装包目录每个平台只留最新包，旧包送回收站。发布附件用 ASCII 拼音名；用户设置和已安装目录不能作为旧包清理。

## 0.4.1 后恢复开发（用户最新授权）

用户已取消冻结，授权修复安卓私密提示阻挡操作、工具栏长按拖动排序、已配对设备名称同步，并核查异常清理和中转恢复验收。录音识别、自定义短语同步、Windows 小剪贴板、安卓中文命名已获用户确认；电脑到安卓私密传输通过，但魅族提示卡住单独作为缺陷。橙色光圈确认为提醒，不再排障。

用户确认不开发 API 用量长期上限与统一占用展示，从本轮待办移除，保留现有统计数据、历史条数上限和清空入口。VPS 已按用户明确确认改为 50M / 10 天，仅重启日志服务并验证原业务 PID 未变；不把此许可扩大到其他删除、重启或网络变更。移动网络、手机后台和锁屏已确认；休眠未单独测，用户决定不再验收。数字输入码补全继续保留为后续需求，本轮先完成明确列出的修复。恢复开发记录见 `docs/plan/resume-042.md`。仍不得清理用户资料、配对身份或更换原签名。

0.4.3 当前范围见 `docs/plan/resume-043.md`。兼容标识和上游署名保留，候选建议不申请短信权限，不将编译/自动回归等同真实短信与机型验收。

0.4.4 当前范围见 `docs/plan/resume-044.md`：安卓逐字母上滑符号与候选左右留白，布局和点击区域同步，保留原有双拼/九宫格特殊行为。真机手感仍需用户确认。
