#!/bin/sh
# 只读报告，不执行清理、删除、服务重启或防火墙变更。
set -eu
for unit in mengren-ime-relay.service mengren-ime-caddy.service; do
    printf '\n%s\n' "$unit"
    systemctl show "$unit" -p ActiveState -p MainPID -p MemoryCurrent
done
printf '\nRelay health: '
curl --fail --silent --show-error --max-time 3 http://127.0.0.1:18319/healthz
printf '\n\n'
df -h /
free -m
journalctl --disk-usage
