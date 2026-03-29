#!/usr/bin/env python3
import socket

def run_server():
    host = '0.0.0.0'
    port = 8080
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((host, port))
        s.listen()
        print(f"[*] Waiting for CyberHub connection on {host}:{port}...")
        while True:
            conn, addr = s.accept()
            with conn:
                print(f"[+] Connected by CyberHub at {addr}")
                data = conn.recv(1024)
                if data:
                    print(f"[>] Received: {data.decode('utf-8', errors='ignore')}")

if __name__ == '__main__':
    run_server()
