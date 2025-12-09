use anyhow::Context;
use dora_node_api::{arrow::array::UInt8Array, DoraNode, Event};
use opencv::{core::Vector, highgui, imgcodecs, prelude::*};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let (mut _node, mut events) = DoraNode::init_from_env()?;
    // 创建一个用于显示的窗口
    highgui::named_window("Dora Webcam Viewer (Rust)", highgui::WINDOW_NORMAL)
        .context("Failed to create highgui window")?;
    println!("Viewer operator initialized.");
    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata: _, data } => match id.as_str() {
                "frame" => {
                    // 将接收到的字节数据转换为 OpenCV Vector
                    // 1. 将 Arrow trait 对象强转为具体的 UInt8Array
                    let uint8_array = data
                        .as_any()
                        .downcast_ref::<UInt8Array>()
                        .context("Arrow data is not UInt8Array (expected byte array)")?;

                    // 2. 提取 UInt8Array 的字节切片
                    let byte_slice = uint8_array.values(); // 返回 &[u8]

                    // 3. 转换为 OpenCV Vector<u8>（from_slice 接收 &[u8]）
                    let buffer = Vector::from_slice(byte_slice);

                    // 解码 JPEG 数据成 Mat
                    let frame = imgcodecs::imdecode(&buffer, imgcodecs::IMREAD_COLOR)
                        .context("Failed to decode image from buffer")?;

                    if frame
                        .size()
                        .context("Failed to get decoded frame size")?
                        .width
                        > 0
                    {
                        // 显示图像
                        highgui::imshow("Dora Webcam Viewer (Rust)", &frame)
                            .context("Failed to imshow frame")?;
                        // 必须调用 wait_key 来处理 GUI 事件
                        highgui::wait_key(1).context("Failed to wait_key")?;
                    }
                }
                other => eprintln!("Received input `{other}`"),
            },
            _ => {}
        }
    }

    Ok(())
}
