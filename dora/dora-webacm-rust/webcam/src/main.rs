use dora_node_api::{DoraNode, Event, dora_core::config::DataId, arrow::array::UInt8Array};
use std::error::Error;
use std::thread;
use std::time::Duration;
use anyhow::Context;
use opencv::{
    core::{Vector}, imgcodecs, prelude::*, videoio::{self, VideoCapture}
};

const CAMERA_INDEX: i32 = 0; // 默认使用第一个摄像头

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let mut camera = VideoCapture::new(CAMERA_INDEX, videoio::CAP_ANY)
        .context("Failed to create video capture")?;
    let output = DataId::from("frame".to_owned());
    // 尝试打开摄像头
    if !VideoCapture::is_opened(&camera).context("Failed to check if camera is open")? {
        // 在 Mac M1 上，有时需要延迟以确保摄像头初始化完成
        thread::sleep(Duration::from_millis(500));
        if !VideoCapture::is_opened(&camera).context("Camera still not open after delay")? {
            return Err("Could not open camera 0 or check its status.".into());
        }
    }

    // 尝试设置分辨率 (可选，可以提高性能或稳定性)
    // camera.set(videoio::CAP_PROP_FRAME_WIDTH, 640.0)?;
    // camera.set(videoio::CAP_PROP_FRAME_HEIGHT, 480.0)?;

    while let Some(event) = events.recv() {
        // println!("Received event: {:?}", event);
        match event {
            Event::Input {
                id,
                metadata,
                data: _,
            } => match id.as_str() {
                "tick" => {
                    let mut frame = Mat::default();
                    
                    // 读取帧
                    if camera
                        .read(&mut frame)
                        .context("Failed to read frame from camera")?
                    {
                        if frame.size().context("Failed to get frame size")?.width > 0 {
                            // 将帧编码为 JPEG 格式的字节向量
                            let mut buffer = Vector::new();
                            imgcodecs::imencode(".jpg", &frame, &mut buffer, &Vector::new())
                                .context("Failed to encode frame to JPEG")?;

                            // 发送原始帧数据
                            let std_buffer: Vec<u8> = buffer.into_iter().collect();

                            // 3. 再转为 Arrow 数组
                            let arrow_array = UInt8Array::from(std_buffer);
                            node.send_output(
                                output.clone(),
                                metadata.parameters,
                                arrow_array,
                            )?;
                        }
                    }
                }
                other => eprintln!("Received input `{other}`"),
            },
            _ => {}
        }
    }

    Ok(())
}
