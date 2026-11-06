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

探测工具创建两台合成设备，验证真实 WSS、PAKE 配对、Noise 身份、双向文字和解绑；结束清理它自己的临时目录。不使用真实剪贴板，不打印令牌。

服务端不保存消息、剪贴板或传输临时文件；等待连接超时即释放。Caddy 证书保存在专用 systemd 状态目录，由 Caddy 维护。日志只用 journald，沿用机器已有容量限制；Caddy 过滤请求字段，不记录鉴权头或正文。构建目录属于开发缓存，与线上用户数据分开管理。

仅同步不超过 1 MiB 的文字和链接；图片与大文件继续使用快传。设备离线时，中转不排队，恢复后由设备仍保留的历史补传。安卓进程被系统结束时，下一次启动输入法或设置才恢复。
