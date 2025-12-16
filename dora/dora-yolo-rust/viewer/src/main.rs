use anyhow::Context;
use dora_node_api::{
    arrow::array::{StructArray, UInt8Array},
    DoraNode, Event,
};
use opencv::{
    core::{Point, Scalar, Vector},
    highgui, imgcodecs, imgproc,
    prelude::*,
};
use std::error::Error;

mod utils;

use utils::arrow_to_bboxes;

fn main() -> Result<(), Box<dyn Error>> {
    let (mut _node, mut events) = DoraNode::init_from_env()?;
    let mut bboxes = Vec::new();
    // 创建一个用于显示的窗口
    highgui::named_window("Dora Webcam Viewer (Rust)", highgui::WINDOW_NORMAL)
        .context("Failed to create highgui window")?;
    println!("Viewer operator initialized.");
    while let Some(event) = events.recv() {
        match event {
            Event::Input {
                id,
                metadata: _,
                data,
            } => match id.as_str() {
                "detections" => {
                    let struct_array = data
                        .as_any()
                        .downcast_ref::<StructArray>()
                        .context("Input is not a StructArray (expected bboxes)")?;

                    let received_bboxes = arrow_to_bboxes(struct_array)?;
                    bboxes = received_bboxes;
                }
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
                        // --- 步骤 D: 在原图上绘制结果 ---
                        let mut display_frame = frame.clone();
                        let bboxes_clone = bboxes.clone();
                        for (classname, bbox, conf) in bboxes_clone {
                            // 画框
                            imgproc::rectangle(
                                &mut display_frame,
                                bbox,
                                Scalar::new(0.0, 255.0, 0.0, 0.0), // 绿色
                                2,
                                imgproc::LINE_8,
                                0,
                            )?;
                            // 写标签
                            let label = format!("{}: {:.2}", classname, conf);
                            imgproc::put_text(
                                &mut display_frame,
                                &label,
                                Point::new(bbox.x, bbox.y - 5),
                                imgproc::FONT_HERSHEY_SIMPLEX,
                                1.0,
                                Scalar::new(0.0, 255.0, 0.0, 0.0),
                                1,
                                imgproc::LINE_8,
                                false,
                            )?;
                        }
                        // 显示图像
                        highgui::imshow("Dora Webcam Viewer (Rust)", &display_frame)
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
