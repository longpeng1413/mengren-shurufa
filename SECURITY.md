# 安全政策 / Security Policy

## 支持的版本

猛人输入法基于青简，当前维护 Windows/Android，只修最新正式版（当前 v0.4.7）。部分功能已有用户确认；具体设备与目标应用的验收范围见[真机测试清单](docs/user/help/mengren-testing.md)。

| 版本 | 支持 |
|---|---|
| 最新的猛人正式版（当前 0.4.7） | 接收报告，按影响评估处理 |
| 更早 | ❌，请升级 |

## 报告漏洞

**不要在公开 issue 里写漏洞细节。** 请用 GitHub 的私密漏洞报告：仓库页面「Security」→「Report a vulnerability」。

维护者按影响评估修复与披露，不承诺固定响应或发版时限，不公开报告中的个人资料或真实凭据。

输入法能看到你敲的每一个键，所以我们特别在意这些：

- 敲的内容外泄：日志、诊断信息、云端请求里出现不该出现的输入；密码框（Secure Input）里仍在组句或发请求。
- 密钥：`.env` / 配置里的 API 密钥被写进日志、诊断信息或别的文件。
- 安装包完整性：Release 上的 pkg 与 `SHA256SUMS`、`releases.json` 里的哈希对不上，或签名有问题。
- 解析崩溃：恶意构造的词库文件（TSV、Rime yaml、`.qj`）、配置文件或云端返回让输入法崩溃或越界。

不在范围内：需要本机管理员权限或物理接触才能利用的问题；你自己填写的 AI 服务商那一侧的问题。

---

**English.** Mengren is based on Qingjian and currently maintains Windows and Android (latest stable release: v0.4.7). Please report vulnerabilities privately through GitHub's "Report a vulnerability" (Security tab), not in public issues. Reports are assessed by impact; no fixed response or release timeline is promised. An input method sees every keystroke, so we care most about keystroke leakage (logs, diagnostics, cloud requests, Secure Input), API key exposure, installer integrity, and crashes from malformed dictionaries, configuration, or cloud responses.
