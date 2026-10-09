#!/usr/bin/env python3
"""Opt-in, digest-pinned workaround for teamspeak-music-bot's SDK 0.2.2.

Run in the bot's installation directory after stopping that bot. Only the exact
published MIT SDK file is changed; future versions require fresh verification.
"""
import argparse
import hashlib
import json
from pathlib import Path

EXPECTED = 'e7a762027f0f07cf86fe8736349000360089727cc51ad279150dda393194733e'
OLD = 'u.set(this.#e.fakeSignature, 0), u.set(l, x), u.set(s, x + b), this.#b(u);'
NEW = 'if (!this.#e.cryptoInitComplete) throw new Error("Voice encryption handshake incomplete");\n\t\tconst [encryptedVoice, voiceTag] = this.#e.encrypt(r.Voice, n, a, l, s, false, false);\n\t\tu.set(voiceTag.slice(0, x), 0), u.set(l, x), u.set(encryptedVoice, x + b), this.#b(u);'

def patched(source):
    if hashlib.sha256(source).hexdigest() != EXPECTED:
        raise ValueError('协议文件摘要不匹配，拒绝修改。请检查 SDK 版本。')
    text = source.decode('utf-8')
    if text.count(OLD) != 1 or text.count('typeFlagged: r.Voice | i.Unencrypted,') != 1:
        raise ValueError('语音发送函数与已验证版本不符，拒绝修改。')
    return text.replace('typeFlagged: r.Voice | i.Unencrypted,', 'typeFlagged: r.Voice,').replace(OLD, NEW).encode('utf-8')

def main():
    parser = argparse.ArgumentParser(description='检查或修复音乐机器人 SDK 0.2.2 语音加密（中文）')
    parser.add_argument('--bot-root', type=Path, required=True, help='音乐机器人安装目录')
    parser.add_argument('--apply', action='store_true', help='确认机器人已停止后应用；默认只检查')
    args = parser.parse_args()
    root = args.bot_root.resolve(strict=True)
    package = (root / 'node_modules/@honeybbq/teamspeak-client').resolve(strict=True)
    if not package.is_relative_to(root):
        raise ValueError('依赖目录不能通过链接指向安装目录之外。')
    if json.loads((package / 'package.json').read_text(encoding='utf-8'))['version'] != '0.2.2':
        raise ValueError('仅兼容已验证的 SDK 0.2.2；其他版本不会修改。')
    file = (package / 'dist/handler-C_JhqGTd.js').resolve(strict=True)
    if not file.is_relative_to(package):
        raise ValueError('协议文件不能指向依赖目录之外。')
    source = file.read_bytes()
    backup = file.with_suffix('.js.webts-backup')
    if backup.exists() and source == patched(backup.read_bytes()):
        print('已经应用已验证的语音加密修复。'); return
    output = patched(source)
    if not args.apply:
        print('SDK 0.2.2 与摘要匹配。先停止音乐机器人，再增加 --apply 应用。'); return
    with backup.open('xb') as target:
        target.write(source)
    temporary = file.with_suffix('.js.webts-new')
    with temporary.open('xb') as target:
        target.write(output)
    temporary.replace(file)
    print('已开启 AES-128-EAX 语音加密，保留原文件备份。请启动机器人并验证音乐。')

if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, KeyError) as error:
        raise SystemExit(str(error))
