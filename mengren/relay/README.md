# 猛人输入法公网中转

独立 Rust 程序，只在内存对接在线设备的加密字节流。与猛人快传使用相同的自有 VPS 思路，但服务和协议独立；原有六位码配对、Noise 加密与对账规则不变。服务器无法解密剪贴板内容。

## 构建与启动

在仓库根目录执行：

```sh
cargo build --locked --release -p mengren-ime-relay --bin mengren-ime-relay
```

运行前设置 `MENGREN_IME_RELAY_TOKEN`（24–256 个可打印 ASCII 字符，使用随机生成的强令牌），可选 `MENGREN_IME_RELAY_BIND`（默认 `127.0.0.1:18319`）。令牌放在只有 root 能读取的 systemd 环境文件，不写入命令历史、仓库、截图或公开日志。

```sh
/opt/mengren-ime-relay/mengren-ime-relay
curl --fail http://127.0.0.1:18319/healthz
```

`deploy/` 提供两个独立 systemd 服务与 Caddy 示例。先将示例域名改成自己的域名，验证配置，再启用服务；Caddy 二进制和中转二进制放在 `/opt/mengren-ime-relay/`，配置在 `/etc/mengren-ime-relay/Caddyfile`。环境文件为 `/etc/mengren-ime-relay.env`，只供中转进程使用。

## HTTPS 与网络

示例使用 `8443/tcp` 提供 HTTPS/WSS，`80/tcp` 供自动证书的 HTTP 验证。443 可以继续留给已有代理；不开 UDP 8443，后端 18319 不开放到公网。修改防火墙前备份，保留原有 SSH 和服务规则，先检查配置；不要直接覆盖已有网站或代理配置。

客户端地址为 `wss://relay.example.com:8443/v1/clipboard`。域名 A/AAAA 必须指向实际可用的地址；没有 IPv6 时不要添加无效 AAAA。`https://relay.example.com:8443/healthz` 仅返回健康状态和在线数量，不能填作客户端中转地址。其他路径返回 404。

## 验证与清理

```sh
cargo test --locked -p qingjian-clipboard-sync --lib
# 通过安全方式向当前进程注入 MENGREN_IME_RELAY_URL 和 MENGREN_IME_RELAY_TOKEN 后运行：
cargo run --locked -p mengren-ime-relay --bin probe
```

探测工具创建两台合成设备，验证真实 WSS、PAKE 配对、Noise 身份、双向文字、图片、私密单次发送和解绑；结束清理它自己的临时目录。不使用真实剪贴板，不打印令牌。

服务端不保存消息、剪贴板或传输临时文件；等待连接超时即释放。Caddy 证书保存在专用 systemd 状态目录，由 Caddy 维护。日志只用 journald，沿用机器已有容量限制；Caddy 过滤请求字段，不记录鉴权头或正文。构建目录属于开发缓存，与线上用户数据分开管理。

`deploy/report.sh` 可单独查看新增服务占用、健康状态、磁盘与日志大小。它只读，不执行清理或重启；已有运维清理工具可调用它作为报告补充。

v0.3.6 起同步小于 100 MB（100,000,000 字节）的文字、链接和图片；文字另限 100 万字符，Windows 图片另限 4000 万像素。普通文件、文件夹与压缩包继续使用快传。设备离线时，中转不排队，恢复后由设备仍保留的历史补传。安卓进程被系统结束时，下一次启动输入法或设置才恢复。

## Debian / Ubuntu 部署步骤

以下以独立服务、HTTPS 8443 为例。先确认端口未被占用、备份当前防火墙与相关配置，保留现有 SSH、网站和代理服务。不要整套覆盖防火墙规则。

1. 将自己的域名 A 指向 VPS 的 IPv4。IPv6 确实可达才添加 AAAA，使用仅 DNS 模式。为本服务新增 TCP 80（ACME）和 8443（HTTPS）的入站放行；云厂商安全组也要放行。18319 仅回环监听。
2. 构建 Linux 静态或对应发行版二进制。将 `mengren-ime-relay` 和从官方发布、核验官方摘要的 Caddy 放入 `/opt/mengren-ime-relay/`，权限 0755。不要替换系统原有 Caddy 或代理服务。
3. 安全创建令牌环境文件。下面的 Python 在服务器上直接写入，不把值打印到终端或命令行：

```sh
sudo install -d -m 755 /opt/mengren-ime-relay /etc/mengren-ime-relay
sudo python3 - <<'PY'
import os, secrets
path = '/etc/mengren-ime-relay.env'
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as out:
    out.write('MENGREN_IME_RELAY_BIND=127.0.0.1:18319\n')
    out.write('MENGREN_IME_RELAY_TOKEN=' + secrets.token_urlsafe(32) + '\n')
PY
```

文件已存在时命令拒绝覆盖，升级不要重新生成令牌。用自己的私密凭据管理方式把令牌配置到两端，不在公开 issue 中粘贴。

4. 将 `deploy/mengren-ime-relay.service`、`deploy/mengren-ime-caddy.service` 安装到 `/etc/systemd/system/`。把 `deploy/Caddyfile.example` 复制到 `/etc/mengren-ime-relay/Caddyfile`，将 `relay.example.com` 换成自己的域名，保留 HTTP-01 验证和回环反代。
5. 先验证，成功后再启动这两个新服务：

```sh
sudo /opt/mengren-ime-relay/caddy validate --config /etc/mengren-ime-relay/Caddyfile --adapter caddyfile
sudo systemctl daemon-reload
sudo systemctl enable --now mengren-ime-relay.service mengren-ime-caddy.service
curl --fail http://127.0.0.1:18319/healthz
curl --fail https://relay.example.com:8443/healthz
```

Caddy 首次申请证书可能需要等待。检查 `journalctl -u mengren-ime-caddy -n 50` 的证书错误，不公开原始日志中的个人信息。若使用 AAAA，分别检查 IPv4/IPv6 的健康访问。命令中的域名都是示例。

6. 两端按 [客户端设置和排查](../../docs/user/help/mengren-relay.md) 填 `wss://relay.example.com:8443/v1/clipboard` 及同一令牌，开启同步后进行六位码配对。原有局域网配对不需要重做。

## 升级与运行边界

先备份旧中转二进制，校验新文件摘要，确认客户端和服务器都是 v0.3.6 或更新版本，再仅重启本服务。保留环境文件、证书和端口配置，不重启其他代理。回退只换回本服务旧二进制，旧版不支持大图片。

服务最多 16 个在线设备、16 个等待会话、48 个连接；单帧 64 KiB。转发采用非阻塞读写和每方向约 129 KiB 的有界缓冲，慢设备会产生反压，不在服务器积攒整张图片。单会话最多 512 MiB 双向总流量、十分钟，空闲三十秒断开；这些是整轮对账限制，单条内容仍必须小于 100 MB。服务端不保留离线消息。

普通内容可能由设备历史补传，私密内容没有历史与补传：只有 Noise 认证的已配对设备主动声明接收能力才尝试。服务器不解密私密正文。测试 `cargo test --release -p qingjian-clipboard-sync hundred_megabyte_boundary -- --ignored` 明确运行大数据测试；它使用临时身份和合成内容，与真实剪贴板无关。

电脑仍能看历史却不发送新复制内容时，先检查「暂停记录」。暂停只停采集与发送新内容，接收仍继续。安卓收到图片后，只有支持图片剪贴板的目标应用才能粘贴；只复制图片 URL 仍是文字链接。自定义过手机工具栏的用户可自行添加锁图标「私密发送」。
