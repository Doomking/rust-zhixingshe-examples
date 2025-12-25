use anyhow::Context;
use dora_node_api::arrow::array::{Array, Float32Array, Int32Array, StringArray, StructArray};
use nalgebra::{Matrix3, Matrix4, Vector3};

pub fn get_intrinsic_matrix(width: f32, height: f32, fov: f32) -> Matrix3<f32> {
    let f = width / (2.0 * (fov.to_radians() / 2.0).tan());
    Matrix3::new(f, 0.0, width / 2.0, 0.0, f, height / 2.0, 0.0, 0.0, 1.0)
}

pub fn get_projection_matrix(values: &[f32]) -> Matrix4<f32> {
    if values.len() < 6 {
        return Matrix4::identity();
    }

    let translation = nalgebra::Translation3::new(values[0], values[1], values[2]);
    // Webots 的 Yaw 对应 Z 轴旋转，但在全局坐标系中可能对应 Y 轴，需根据 .wbt 确定
    let rotation = nalgebra::Rotation3::from_euler_angles(values[3], values[4], values[5]);

    nalgebra::Isometry3::from_parts(translation, rotation.into()).to_homogeneous()
}

// 将点云从局部坐标转换到相机 2D 视图
pub fn project_to_camera(points: &[[f32; 3]], intrinsic: &Matrix3<f32>) -> Vec<[f32; 3]> {
    points
        .iter()
        .map(|p| {
            let point = Vector3::new(p[0], p[1], p[2]);
            let projected = intrinsic * point;
            let z = projected[2];
            [projected[0] / z, projected[1] / z, z] // 返回 [u, v, depth]
        })
        .collect()
}

// 你的辅助转换函数
pub fn arrow_to_bboxes(
    struct_array: &StructArray,
) -> Result<Vec<(String, Rect, f32)>, Box<dyn std::error::Error>> {
    let len = struct_array.len();

    let class_array = struct_array
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .context("class_name")?;
    let conf_array = struct_array
        .column(1)
        .as_any()
        .downcast_ref::<Float32Array>()
        .context("confidence")?;
    let x_array = struct_array
        .column(2)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("bbox_x")?;
    let y_array = struct_array
        .column(3)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("bbox_y")?;
    let w_array = struct_array
        .column(4)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("bbox_w")?;
    let h_array = struct_array
        .column(5)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("bbox_h")?;

    let mut bboxes = Vec::with_capacity(len);
    for i in 0..len {
        let name = class_array.value(i).to_owned();
        let conf = conf_array.value(i);
        let rect = Rect::new(
            x_array.value(i),
            y_array.value(i),
            w_array.value(i),
            h_array.value(i),
        );
        bboxes.push((name, rect, conf));
    }
    Ok(bboxes)
}

// 假设你的 Rect 定义如下
#[derive(Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
}
