# 安全政策 / Security Policy

## 支持的版本

猛人输入法基于青简，当前维护 Windows/Android，只修最新正式版。0.4.1 为本轮冻结版，发布后暂停开发。已有局部真机确认，不能据协议测试推断所有真实流程可靠；后续修复由维护者决定是否恢复开发：

| 版本 | 支持 |
|---|---|
| 最新的猛人正式版（当前 0.4.1） | 接收报告，开发暂停；不承诺修复时间 |
| 更早 | ❌，请升级 |

## 报告漏洞

**不要在公开 issue 里写漏洞细节。** 请用 GitHub 的私密漏洞报告：仓库页面「Security」→「Report a vulnerability」。

暂停维护期间不承诺固定响应或发版时限；维护者恢复处理后按影响评估修复与披露，不公开报告中的个人资料或真实凭据。

输入法能看到你敲的每一个键，所以我们特别在意这些：

- 敲的内容外泄：日志、诊断信息、云端请求里出现不该出现的输入；密码框（Secure Input）里仍在组句或发请求。
- 密钥：`.env` / 配置里的 API 密钥被写进日志、诊断信息或别的文件。
- 安装包完整性：Release 上的 pkg 与 `SHA256SUMS`、`releases.json` 里的哈希对不上，或签名有问题。
- 解析崩溃：恶意构造的词库文件（TSV、Rime yaml、`.qj`）、配置文件或云端返回让输入法崩溃或越界。

不在范围内：需要本机管理员权限或物理接触才能利用的问题；你自己填写的 AI 服务商那一侧的问题。

---

**English.** Mengren is based on Qingjian and maintains Windows/Android. Only the latest Mengren stable release (currently 0.3.7) receives fixes; recent private-transfer device feedback remains unresolved. Please report vulnerabilities privately via
GitHub's "Report a vulnerability" (Security tab), not in public issues. Expect a reply within 3 days; confirmed issues are fixed
in the next release and credited in the changelog. An input method sees every keystroke, so we care most about keystroke leakage
(logs, diagnostics, cloud requests, Secure Input), API key exposure, installer integrity (pkg vs. `SHA256SUMS`), and crashes from
malformed dictionaries, config or cloud responses.
