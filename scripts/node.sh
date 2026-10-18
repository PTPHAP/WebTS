#!/bin/sh
set -eu
umask 077
node_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$node_root"
case "${1:-help}" in
    init)
        if [ ! -e node.local.toml ]; then
            # noclobber also protects against a second concurrent initializer.
            (set -C; cat node.example.toml > node.local.toml)
        fi
        mkdir -p secrets
        if [ ! -e secrets/node-pair.key ]; then
            ./web-ts init-key secrets/node-pair.key
        fi
        printf '%s\n' '初始化完成；已有配置和密钥不替换。核对本机TS端口后运行：sh scripts/node.sh configure'
        ;;
    configure)
        printf '%s' 'WebTS主站HTTPS地址：'; IFS= read -r node_main
        printf '%s' '语音公网IPv4（回车保留原配置）：'; IFS= read -r node_public
        case "$node_main" in *[!a-zA-Z0-9:/._-]*|'') printf '%s\n' '地址请使用ASCII域名，不含凭据、查询或特殊字符。' >&2; exit 1;; esac
        case "$node_public" in *[!0-9.]*) printf '%s\n' '公网IPv4格式错误。' >&2; exit 1;; esac
        printf '%s' '5分钟一次性配对码（回车保留已有密钥）：'
        node_tty=''
        if [ -t 0 ]; then node_tty=$(stty -g); stty -echo; trap 'stty "$node_tty"' EXIT HUP INT TERM; fi
        IFS= read -r node_code
        if [ -n "$node_tty" ]; then stty "$node_tty"; trap - EXIT HUP INT TERM; fi
        printf '\n'
        case "$node_code" in *[!a-fA-F0-9]*) printf '%s\n' '配对码格式错误。' >&2; exit 1;; esac
        if [ -n "$node_public" ]; then
            printf '{"main_url":"%s","public_ip":"%s","code":"%s"}\n' "$node_main" "$node_public" "$node_code" | ./web-ts configure-node node.local.toml
        else
            printf '{"main_url":"%s","code":"%s"}\n' "$node_main" "$node_code" | ./web-ts configure-node node.local.toml
        fi
        unset node_code
        ;;
    check) exec ./web-ts check-node node.local.toml ;;
    start)
        ./web-ts check-node node.local.toml
        exec ./web-ts serve-node node.local.toml
        ;;
    *) printf '%s\n' 'WebTS 反向节点：sh scripts/node.sh init | configure | check | start；完整教程见 README.md。' ;;
esac
