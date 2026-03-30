#!/usr/bin/env python3
import socket
import subprocess

def handle_client(conn, addr):
    print(f"[+] CyberHub connected from {addr}")
    try:
        buf = ""
        while True:
            data = conn.recv(1024)
            if not data:
                print(f"[-] CyberHub disconnected from {addr}")
                break
            buf += data.decode('utf-8', errors='ignore')
            # TCP 是流协议，按换行符分割消息
            while '\n' in buf:
                line, buf = buf.split('\n', 1)
                line = line.strip()
                if not line:
                    continue
                print(f"[>] Received: {line}")
                if line == "lock_screen":
                    print("[!] Triggering macOS Lock Screen...")
                    # 现代 macOS 锁屏的几种方法：
                    # 方法 1: 使用 AppleScript 模拟快捷键 (Control+Command+Q) - 需要辅助功能权限
                    # subprocess.run(["osascript", "-e", 'tell application "System Events" to keystroke "q" using {control down, command down}'])
                    
                    # 方法 2: 使用私有 API (SACLockScreenImmediate) - 最直接，通常不需要额外权限
                    try:
                        import ctypes
                        login_pf = ctypes.CDLL("/System/Library/PrivateFrameworks/login.framework/Versions/Current/login")
                        login_pf.SACLockScreenImmediate()
                    except Exception as e:
                        print(f"[!] Private API Lock failed: {e}, trying fallback...")
                        # 方法 3: 强制显示器进入睡眠（如果设置了唤醒需密码，则等同于锁屏）
                        subprocess.run(["pmset", "display_sleep_now"])
                elif line == "ping":
                    pass  # 忽略心跳包
    except ConnectionResetError:
        print(f"[-] Connection reset by CyberHub (Mac locked/network change)")
    except Exception as e:
        print(f"[!] Error: {e}")

def run_server():
    host = '0.0.0.0'
    port = 8080
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((host, port))
        s.listen()
        print(f"[*] CyberHub server listening on {host}:{port}...")
        while True:
            conn, addr = s.accept()
            with conn:
                handle_client(conn, addr)

if __name__ == '__main__':
    run_server()
