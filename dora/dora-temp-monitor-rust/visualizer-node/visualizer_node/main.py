"""TODO: Add docstring."""

#!/usr/bin/env python3
# visualizer.py - 新增节点：实时温度曲线图
import threading
from collections import deque

import matplotlib.animation as animation
import matplotlib.pyplot as plt
import pyarrow as pa
from dora import Node


class TempVisualizer:
    def __init__(self, max_points=100):
        self.max_points = max_points
        self.timestamps = deque(maxlen=max_points)
        self.temperatures = deque(maxlen=max_points)
        self.fig, self.ax = plt.subplots(figsize=(10, 6))
        (self.line,) = self.ax.plot([], [], "b-", label="Temperature Curve")
        self.ax.set_ylim(15, 40)
        self.ax.set_xlim(0, max_points)
        self.ax.set_xlabel("Timestamp")
        self.ax.set_ylabel("Temperature (°C)")
        self.ax.set_title("Real Time Temperature Monitoring (M1 Pro)")
        self.ax.legend()
        self.ax.grid(True)

    def update_plot(self, frame):
        self.line.set_data(range(len(self.temperatures)), self.temperatures)
        self.ax.set_xlim(0, max(len(self.temperatures), self.max_points))
        return (self.line,)


def data_receiver(visualizer):
    """后台线程接收dora数据"""

    node = Node()
    for event in node:
        if event["type"] == "INPUT" and event["id"] == "data":
            array = event["value"]
            temp = array[0].as_py()
            visualizer.temperatures.append(temp)
            visualizer.timestamps.append(len(visualizer.timestamps))


def main():
    visualizer = TempVisualizer()
    # 启动数据接收线程（非阻塞）
    data_thread = threading.Thread(target=data_receiver, args=(visualizer,))
    data_thread.daemon = True
    data_thread.start()

    # 启动matplotlib动画
    ani = animation.FuncAnimation(
        visualizer.fig,
        visualizer.update_plot,
        interval=100,  # 100ms刷新一次
        blit=True,
    )
    plt.show()


if __name__ == "__main__":
    main()
