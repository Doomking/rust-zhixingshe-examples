#!/bin/bash

# LiquidCast 一键管理脚本 - 用于快速启动和关闭项目

COMMAND=$1
ROOT_DIR=$(pwd)

# 颜色定义，用于输出格式化
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # 无颜色

# 启动服务端函数
function start_server() {
    echo -e "${GREEN}[1/2] 正在启动 mac-server...${NC}"
    cd "$ROOT_DIR/mac-server"
    
    # 使用 nohup 在后台启动 cargo run --release
    # 并将所有输出重定向到 server.log 文件中
    nohup cargo run --release > server.log 2>&1 &
    
    # 记录进程 ID，方便后续关闭
    echo $! > .server.pid
    echo -e "${GREEN}mac-server 已在后台启动 (PID: $(cat .server.pid))，日志见 mac-server/server.log${NC}"
}

# 启动客户端函数
function start_client() {
    echo -e "${GREEN}[2/2] 正在编译并烧录 esp-client...${NC}"
    cd "$ROOT_DIR/esp-client"
    
    # 编译并烧录到 ESP32，完成后自动进入监控模式查看串口输出
    cargo run --release
}

# 关闭所有相关进程
function stop_all() {
    echo -e "${RED}正在关闭所有项目...${NC}"
    
    # 1. 尝试通过 PID 文件关闭 mac-server
    if [ -f "$ROOT_DIR/mac-server/.server.pid" ]; then
        PID=$(cat "$ROOT_DIR/mac-server/.server.pid")
        kill $PID 2>/dev/null
        rm "$ROOT_DIR/mac-server/.server.pid"
        echo -e "${RED}mac-server (PID: $PID) 已关闭${NC}"
    else
        # 兜底清理：如果 PID 文件不在，按名称查找并关闭
        pkill -f "target/release/mac-server"
        echo -e "${RED}已尝试清理所有 mac-server 进程${NC}"
    fi

    # 2. 清理可能残留的 espflash 串口监控进程
    pkill -f "espflash"
    echo -e "${RED}已清理串口监控进程${NC}"
}

# 脚本逻辑分发
case $COMMAND in
    "run")
        # 一键启动服务端和客户端
        start_server
        start_client
        ;;
    "stop")
        # 一键关闭
        stop_all
        ;;
    "server")
        # 仅启动服务端，并实时跟踪日志
        start_server
        tail -f "$ROOT_DIR/mac-server/server.log"
        ;;
    *)
        # 帮助信息
        echo "用法: ./liquid.sh [run|stop|server]"
        echo "  run    - 同时启动服务端(后台)和客户端(编译、烧录并监控)"
        echo "  stop   - 关闭服务端及清理监控进程"
        echo "  server - 仅启动服务端并查看实时日志"
        ;;
esac
