use anyhow::Context;
use dora_node_api::{
    arrow::array::UInt8Array, dora_core::config::DataId, DoraNode, Event, Parameter,
};
use opencv::{
    core::{copy_make_border, AlgorithmHint, Rect, Scalar, Vec4b},
    imgproc,
    prelude::*,
};
use std::error::Error;

use candle_core::{DType, Device, Module, Tensor};
use candle_nn::VarBuilder;
// use hf_hub::api::sync::Api;

use std::env;
use std::path::Path;

mod model;

use model::{Multiples, YoloV8};

mod utils;

use utils::bboxes_to_arrow;

// --- 常量定义 ---
const CONFIDENCE_THRESHOLD: f32 = 0.25;
const IOU_THRESHOLD: f32 = 0.45;
const MODEL_SIZE: usize = 640; // YOLOv8 标准输入大小
const LABELS: [&str; 80] = [
    "person",
    "bicycle",
    "car",
    "motorcycle",
    "airplane",
    "bus",
    "train",
    "truck",
    "boat",
    "traffic light",
    "fire hydrant",
    "stop sign",
    "parking meter",
    "bench",
    "bird",
    "cat",
    "dog",
    "horse",
    "sheep",
    "cow",
    "elephant",
    "bear",
    "zebra",
    "giraffe",
    "backpack",
    "umbrella",
    "handbag",
    "tie",
    "suitcase",
    "frisbee",
    "skis",
    "snowboard",
    "sports ball",
    "kite",
    "baseball bat",
    "baseball glove",
    "skateboard",
    "surfboard",
    "tennis racket",
    "bottle",
    "wine glass",
    "cup",
    "fork",
    "knife",
    "spoon",
    "bowl",
    "banana",
    "apple",
    "sandwich",
    "orange",
    "broccoli",
    "carrot",
    "hot dog",
    "pizza",
    "donut",
    "cake",
    "chair",
    "couch",
    "potted plant",
    "bed",
    "dining table",
    "toilet",
    "tv",
    "laptop",
    "mouse",
    "remote",
    "keyboard",
    "cell phone",
    "microwave",
    "oven",
    "toaster",
    "sink",
    "refrigerator",
    "book",
    "clock",
    "vase",
    "scissors",
    "teddy bear",
    "hair drier",
    "toothbrush",
];

pub fn select_device() -> Result<Device, Box<dyn Error>> {
    // 尝试 Metal 设备 (如果 'metal' 特性已启用)
    if let Ok(device) = Device::new_metal(0) {
        println!("🚀 Using Metal device.");
        return Ok(device);
    }

    // 回退到 CPU
    println!("🐢 Using CPU device.");
    Ok(Device::Cpu)
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let output = DataId::from("detections".to_owned());
    // 加载 YOLOv8 模型 (使用 HuggingFace 自动下载)
    println!("Loading YOLOv8 model...");
    // 优化后 (如果支持 CUDA):
    let device = select_device().unwrap();
    // let api = Api::new()?;
    // let repo = api.model("/lmz/candle-yolo-v8".to_string());
    // let model_file = repo.get("yolov8n.safetensors")?;

    // https://hf-mirror.com/lmz/candle-yolo-v8/tree/main
    // 定义本地模型文件的路径
    let current_dir = env::current_dir().context("Failed to get current working directory")?;

    // 定义相对路径
    let relative_path = Path::new("object-detection/models/yolov8n.safetensors");

    //  尝试合并路径并检查
    let local_model_path = current_dir.join(relative_path);
    // 验证文件是否存在（可选，但推荐）
    if !local_model_path.exists() {
        return Err(format!("Model file not found at: {}", local_model_path.display()).into());
    }
    let model_file = local_model_path;
    // 加载权重
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[model_file], DType::F32, &device)? };
    let model = YoloV8::load(vb, Multiples::n(), 80)?;

    println!("Model loaded successfully.");

    while let Some(event) = events.recv() {
        // println!("Received event: {:?}", event);
        match event {
            Event::Input { id, metadata, data } => match id.as_str() {
                "frame" => {
                    // 尝试获取 width
                    let cols = match metadata.parameters.get("width") {
                        Some(Parameter::Integer(v)) => *v as i32,
                        _ => 640, // 默认值
                    };

                    // 尝试获取 height
                    let rows = match metadata.parameters.get("height") {
                        Some(Parameter::Integer(v)) => *v as i32,
                        _ => 480, // 默认值
                    };
                    // 将接收到的字节数据转换为 OpenCV Vector
                    // 1. 将 Arrow trait 对象强转为具体的 UInt8Array
                    let uint8_array = data
                        .as_any()
                        .downcast_ref::<UInt8Array>()
                        .context("Arrow data is not UInt8Array (expected byte array)")?;

                    // 2. 提取 UInt8Array 的字节切片
                    let byte_slice = uint8_array.values(); // 返回 &[u8]
                    if byte_slice.len() != (rows * cols * 4) as usize {
                        eprintln!(
                            "Data size mismatch! Expected {}, got {}",
                            rows * cols * 4,
                            byte_slice.len()
                        );
                        continue;
                    }

                    // 关键点：将 &[u8] 转换为 &[Vec4b]
                    // Vec4b 代表一个由 4 个 u8 组成的像素点
                    let (head, vec4_slice, tail) = unsafe { byte_slice.align_to::<Vec4b>() };

                    if !head.is_empty() || !tail.is_empty() {
                        // 如果数据不是 4 的倍数，可能会进到这里
                        eprintln!("Warning: Byte slice alignment issue");
                    }

                    // 现在传入只有 3 个参数的函数
                    // 因为 vec4_slice 的长度刚好是 byte_slice.len() / 4
                    // 128 * 64 = 8192，这与 vec4_slice.len() 完美匹配！
                    let frame_raw = Mat::new_rows_cols_with_data(
                        rows,       // 128 (行)
                        cols,       // 64 (列)
                        vec4_slice, // 32768字节的 u8 切片现在变成了 8192长度的 Vec4b 切片
                    )?;

                    // let frame_raw = Mat::new_rows_cols_with_data(
                    //     rows,       // 第1个参数: i32
                    //     cols,       // 第2个参数: i32
                    //     byte_slice, // 只需要传入切片本身，不要传指针和长度
                    // )?;
                    let mut frame = Mat::default();
                    // 注意：Webots 的颜色顺序可能是 BGRA，如果颜色不对，请尝试 COLOR_BGRA2RGB
                    imgproc::cvt_color(
                        &frame_raw,
                        &mut frame,
                        imgproc::COLOR_BGRA2BGR,
                        0,
                        AlgorithmHint::ALGO_HINT_DEFAULT,
                    )?;

                    if frame.empty() {
                        eprintln!("Warning: Decoded frame is empty. Skipping this iteration.");
                        continue; // 跳过当前循环，不进入 preprocess_image
                    }
                    // --- 步骤 A: 图像预处理 (OpenCV -> Candle Tensor) ---
                    let (processed_tensor, ratio, pad_w, pad_h) =
                        preprocess_image(&frame, &device)?;

                    // --- 步骤 B: 模型推理 ---
                    let predictions = model.forward(&processed_tensor)?;

                    // --- 步骤 C: 后处理 (NMS) ---
                    // predictions 维度通常是 (1, 84, 8400) -> (Batch, Classes+Coords, Anchors)
                    let preds = predictions.squeeze(0)?;
                    let (bboxes, keypoints) = report_detect(&preds, &frame, ratio, pad_w, pad_h)?;

                    let arrow_array = bboxes_to_arrow(bboxes)?;

                    node.send_output(output.clone(), metadata.parameters, arrow_array)?;
                }
                other => eprintln!("Received input `{other}`"),
            },
            _ => {}
        }
    }

    Ok(())
}

// 图像预处理：调整大小、填充、归一化、转 Tensor
fn preprocess_image(
    frame: &Mat,
    device: &Device,
) -> Result<(Tensor, f32, f32, f32), Box<dyn Error>> {
    let width = frame.cols();
    let height = frame.rows();

    // 计算缩放比例，保持长宽比
    let ratio = (MODEL_SIZE as f32 / width.max(height) as f32).min(1.0);
    let new_w = (width as f32 * ratio) as i32;
    let new_h = (height as f32 * ratio) as i32;

    // Resize
    let mut resized = Mat::default();
    imgproc::resize(
        frame,
        &mut resized,
        opencv::core::Size::new(new_w, new_h),
        0.0,
        0.0,
        imgproc::INTER_LINEAR,
    )?;

    // Letterbox padding (填充灰色背景到 640x640)
    let dw = (MODEL_SIZE as i32 - new_w) / 2;
    let dh = (MODEL_SIZE as i32 - new_h) / 2;

    let mut padded = Mat::default();
    copy_make_border(
        &resized,
        &mut padded,
        dh,
        MODEL_SIZE as i32 - new_h - dh, // top, bottom
        dw,
        MODEL_SIZE as i32 - new_w - dw, // left, right
        opencv::core::BORDER_CONSTANT,
        Scalar::new(114.0, 114.0, 114.0, 0.0), // YOLO 灰色背景
    )?;

    // BGR -> RGB
    let mut rgb = Mat::default();
    imgproc::cvt_color(
        &padded,
        &mut rgb,
        imgproc::COLOR_BGR2RGB,
        0,
        AlgorithmHint::ALGO_HINT_DEFAULT,
    )?;

    // 转为 Vec<u8>
    let data_vec: Vec<u8> = rgb.data_bytes()?.to_vec();

    // 转为 Candle Tensor: (Batch, Channel, Height, Width)
    // 原始数据是 HWC (640, 640, 3)，需要转为 CHW 并归一化 0-1
    let tensor = Tensor::from_vec(data_vec, (MODEL_SIZE, MODEL_SIZE, 3), device)?
        .permute((2, 0, 1))? // HWC -> CHW
        .to_dtype(DType::F32)?
        .affine(1. / 255., 0.)? // 归一化
        .unsqueeze(0)?; // 添加 Batch 维度 -> (1, 3, 640, 640)

    Ok((tensor, ratio, dw as f32, dh as f32))
}

/// 解析推理结果
/// YOLOv8 Output: [batch, 84, 8400] (xc, yc, w, h, class0...class79)
fn report_detect(
    pred: &Tensor,
    original_frame: &Mat,
    ratio: f32,
    pad_w: f32,
    pad_h: f32,
) -> Result<(Vec<(&'static str, Rect, f32)>, Option<Vec<()>>), Box<dyn Error>> {
    // 1. 转置为 [8400, 84] 便于处理
    let pred = pred.t()?;
    let (n_preds, _n_coords) = pred.dims2()?;
    let pred_vec: Vec<Vec<f32>> = pred.to_vec2()?; // 获取数据到 CPU

    let mut results = Vec::new();

    for i in 0..n_preds {
        let row = &pred_vec[i];

        // 找出最高分的类别 (前4个是坐标，后面是类别)
        let scores = &row[4..];
        let (max_score_idx, max_score) =
            scores
                .iter()
                .enumerate()
                .fold(
                    (0, 0.0_f32),
                    |(idx, max), (i, &val)| {
                        if val > max {
                            (i, val)
                        } else {
                            (idx, max)
                        }
                    },
                );

        if max_score > CONFIDENCE_THRESHOLD {
            // 解析坐标 (cx, cy, w, h) -> 模型输入坐标系
            let cx = row[0];
            let cy = row[1];
            let w = row[2];
            let h = row[3];

            // 转换回原图坐标 (去除 padding 并除以缩放比例)
            let x = ((cx - w / 2.0 - pad_w) / ratio).max(0.0);
            let y = ((cy - h / 2.0 - pad_h) / ratio).max(0.0);
            let width = (w / ratio).min(original_frame.cols() as f32 - x);
            let height = (h / ratio).min(original_frame.rows() as f32 - y);

            results.push((
                LABELS[max_score_idx],
                Rect::new(x as i32, y as i32, width as i32, height as i32),
                max_score,
            ));
        }
    }

    // 简单 NMS (非极大值抑制)
    // 注意：生产环境建议使用 torchvision 或 opencv 自带的 NMSBoxes
    let mut kept_results = Vec::new();
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap()); // 按置信度降序

    while let Some(current) = results.pop() {
        kept_results.push(current.clone());
        // 移除 IOU 大于阈值的框
        results.retain(|item| iou(&current.1, &item.1) < IOU_THRESHOLD);
    }

    Ok((kept_results, None))
}

// 计算两个 Rect 的 IOU
fn iou(box_a: &Rect, box_b: &Rect) -> f32 {
    let x_a = box_a.x.max(box_b.x);
    let y_a = box_a.y.max(box_b.y);
    let x_b = (box_a.x + box_a.width).min(box_b.x + box_b.width);
    let y_b = (box_a.y + box_a.height).min(box_b.y + box_b.height);

    let inter_area = (x_b - x_a).max(0) as f32 * (y_b - y_a).max(0) as f32;
    let box_a_area = (box_a.width * box_a.height) as f32;
    let box_b_area = (box_b.width * box_b.height) as f32;

    inter_area / (box_a_area + box_b_area - inter_area)
}
