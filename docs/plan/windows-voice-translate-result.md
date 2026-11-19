# Windows 语音、翻译和剪贴板开发结果

## 完成项

- [x] 缺省长按右 Alt 听写。松手结束，Esc 取消。语音设置提供右 Alt、右 Ctrl、长按 Win、自定义组合键和不用快捷键。服务器热加载语音服务及快捷键。
- [x] TSF 空闲轮询可取回识别结果。快捷键长按生效后补发遮罩键并放行抬键。语音面板区分按住与点按触发，错误提示保留约 2.5 秒。
- [x] 悬浮条加入「说」「译」。点「说」开始或结束录音，录音中高亮。点「译」或按 Ctrl+Alt+F 呼出翻译窗口。
- [x] 独立翻译窗口实现中外互译、停顿后自动翻译、Ctrl+Enter、对调、两侧系统朗读和复制译文。互译语言保存在用户配置目录的 `translator-language` 文件。
- [x] 修复剪贴板列表刷新打断删除及滚动跳转，补充回归。新界面文案补齐七种语言。安装脚本已接入翻译程序。

## 文件

| 范围 | 文件与职责 |
| --- | --- |
| Windows Server | `apps/windows/server/src/hotkey.rs` 管理热键状态。 `ipc/{work,pipe}.rs` 与 `main.rs` 接入控制事件。 `dispatch/{config,message,mode,reload,status,voice}` 协调热加载、录音及结果交付。 `ui/{mod,voice,status}` 增加六格状态条、语音面板和翻译入口。 |
| TSF 与协议 | `apps/windows/tsf/src/client/` 的模式轮询与回包，以及 `com/{poll,service}` 的焦点文档编辑。 `crates/qingjian-platform/src/protocol/` 定义可选的轮询能力和待上屏文字。 |
| 翻译窗口 | 新建 `apps/windows/translator/`，内含 Win32 控件、请求代次与防抖、SAPI 朗读。根 `Cargo.toml` 和 `Cargo.lock` 纳入新程序。 `apps/windows/installer/{build.ps1,qingjian.iss}` 纳入构建、签名、安装和卸载流程。 |
| 设置与文案 | `apps/settings/src/{shortcut,settings,view/pages}` 接入录音及翻译快捷键。 `crates/qingjian-platform/src/config/` 定义配置与模板。 `apps/settings/locales/`、`crates/qingjian-platform/locales/windows/` 维护简中、繁中、英语、日语、西语、越语及德语。 |
| 剪贴板 | `apps/clipboard/src/app/{mod,menu,items,signal,window}.rs` 保留记录位置与菜单目标。 `crates/qingjian-clipboard/src/store/{mod,tests}.rs` 处理已有记录时间戳领先本机时的删除。 |
| 文档 | `CLAUDE.md`、`docs/design/{voice-and-swipe,mengren-public}.md`、`docs/notes/crate-notes.md`、`docs/plan/todo.md`、`docs/user/getting-started/keys.md` 和 `docs/user/platforms/windows-voice-translate.md`。 |

## 剪贴板根因

后台复制或同步完成都会刷新历史列表。旧刷新逻辑每次把选中项重置为第 0 条并关闭右键菜单。用户在菜单打开期间遇到后台刷新时，「删除」可能失去目标。随后界面把第 0 条滚入视野，表现为跳回顶部。菜单本身还以列表下标定位，刷新重排后原下标已经可能属于另一条记录。

现在刷新时按记录 ID 重定位菜单和选中项。菜单目标已消失时关闭菜单。删除成功后，选中原位置的相邻记录。回归测试先在菜单打开时插入一条新记录并刷新，再执行删除，验证删的是原目标且选中邻居。另有一项存储层回归覆盖设备时钟偏差：当原记录比本机当前时间更新时，删除标记也要晚于原记录，才能在同步中生效。

## 验证

按方案 §3 从头顺序执行。首次在沙箱中运行时，Windows DPAPI 测试无法访问当前用户密钥。切到本机用户环境后，整轮通过。

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all` | 通过 |
| `cargo build -p qingjian-windows-translator` | 通过，已更新 `Cargo.lock` |
| `cargo build --locked`（Server、TSF、设置、翻译窗口） | 通过 |
| `cargo test -p qingjian-platform` | 135 项通过，覆盖旧配置读入和文案键一致性 |
| `cargo test -p qingjian-windows-server` | 182 项通过，覆盖按住、短按、Esc、翻译键，以及语音结果单次回传与旧端兼容 |
| `cargo test -p qingjian-windows-tsf` | 27 项通过 |
| `cargo test -p qingjian-settings` | 29 项通过。已有的联网模型下载测试按其设置跳过 |
| `cargo test -p qingjian-render` | 63 项通过 |
| `cargo clippy --all-targets -- -D warnings`（platform、Server、TSF、设置、翻译窗口） | 通过 |
| 剪贴板存储、剪贴板应用和翻译窗口附加测试 | 55 项通过，包括两项剪贴板回归和翻译方向测试 |
| Windows 32 位 TSF `cargo check` | 通过 |
| Android arm64 Rust `cargo check` | 通过 |
| Linux musl 新程序的非 Windows 入口 `cargo check` | 通过。共享平台 crate 的 Linux 交叉检查受本机缺少目标 OpenSSL 开发库阻挡 |
| macOS 目标 | 本机无相应 Rust 目标和系统 SDK，未执行 |

本轮产出 Windows 开发构建，版本号保持 0.4.8。安装脚本已修改，本轮无安装包、提交或发布记录。

## 待真机验收

1. 在记事本、浏览器和办公应用用麦克风测试长按说话、松开后直接上屏及 Esc 取消。切换右 Ctrl、Win、自定义键和不用快捷键，确认设置运行中生效，短按和普通组合键照常。
2. 关闭语音服务并触发一次，确认提示停留约 2.5 秒。开启服务后，点悬浮条「说」录音并再次点击结束，确认只有录音时格子高亮。
3. 用真实有道和 AI 服务分别验证翻译。在双屏或不同 DPI 下检查窗口打开、收起、焦点、语言记忆和多行排版。试听 Windows 对应语音包及缺少对应语音时的默认声音，复制译文到目标应用。
4. 打开较长的剪贴板列表，滚到中间打开右键菜单。另一台设备同步一条新记录后点击「删除」，确认原记录移除、滚动位置仍在邻近条目。

## Windows 0.4.9 发布修订

第一轮本机联调使用 0.4.8 开发构建，输入法被宿主拉起时会打开显示运行日志的终端。0.4.9 正式构建使用 Windows 图形子系统，服务从后台启动。联调时现有聊天进程还加载着旧 DLL。应用完全退出再打开后才会使用新版的空闲上屏路径。

修订将听写绑定到 Windows 键盘焦点所在的线程与窗口，编辑会话执行前再核对 TSF 文档。回归测试覆盖后台会话查询后正确交付、同一线程换窗口后丢弃待上屏结果。翻译窗口默认黑底，顶部新增选色按钮，颜色保存到用户配置目录的 `translator-background` 文件。

## Windows 0.4.10 听写交付与翻译窗口

0.4.9 的现场日志出现了识别完成和目标线程编号，随后没有取走文字的记录。安装注册已指向 0.4.9，保持打开的应用仍使用升级前加载的 DLL。安装器按版本并排注册 DLL，旧宿主只能在退出后释放其映射。

- `apps/windows/server/src/dispatch/voice` 保留原控件的句柄、线程和限时待交付文字。正常 TSF 请求仍优先取走文字。超过 520 毫秒无人领取时，`hotkey.rs` 在修饰键释放后核对当前焦点，向原控件发送一批 Unicode 输入。任一路成功即消费结果；识别期间换窗口、结果超时或发送失败会终止交付。目标以 Windows 报告的线程和控件句柄判断。对于使用同一原生控件承载多个网页编辑区的应用，用户仍应保持光标在原编辑区。
- `apps/windows/tsf/src/com/service/key_sink.rs` 放行 Unicode 输入事件。上屏不使用系统剪贴板。失败提示和翻译设置覆盖七种界面语言。
- `apps/windows/translator/src/window` 使用可调主题绘制窗口、原生编辑区边框与按钮；亮色底调淡，文字按亮度设定对比度。菜单可恢复默认黑色。原生编辑框保留输入法、选择和复制能力。
- 已执行格式化、翻译程序构建、锁文件构建、六个相关包的回归及全目标 clippy。常规测试 441 项通过。另在交互式 Windows 桌面运行光标插入测试，确认中文和表情插入两字符之间。设置程序原有的联网下载模型测试按默认设置跳过。
- 在独立桌面使用真实窗口检查黑色与浅黄主题、输入后「翻译」按钮状态，检查结束销毁预览进程。仍待用户在 ChatGPT 里用真实麦克风验证，以及检查其他应用和多显示器 DPI。
