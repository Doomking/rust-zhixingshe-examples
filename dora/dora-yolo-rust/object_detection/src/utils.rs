use std::sync::Arc;

use anyhow::Context;
use dora_node_api::arrow::array::{Array, ArrayRef, Float32Array, Int32Array, StringArray, StructArray};
use dora_node_api::arrow::datatypes::{DataType, Field, Fields, Schema};
use opencv::core::Rect;

/// 将 Vec<(&str, Rect, f32)> 转换为 Arrow StructArray
pub fn bboxes_to_arrow(
    bboxes: Vec<(&'static str, Rect, f32)>,
) -> Result<StructArray, Box<dyn std::error::Error>> {
    // 字段列表
    let num_detections = bboxes.len();

    // 1. 准备各个字段的底层数据 (Builder)
    let mut class_names = Vec::with_capacity(num_detections);
    let mut confidences = Vec::with_capacity(num_detections);
    let mut bboxes_x = Vec::with_capacity(num_detections);
    let mut bboxes_y = Vec::with_capacity(num_detections);
    let mut bboxes_w = Vec::with_capacity(num_detections);
    let mut bboxes_h = Vec::with_capacity(num_detections);

    for (name, rect, conf) in bboxes {
        class_names.push(name);
        confidences.push(conf);
        bboxes_x.push(rect.x);
        bboxes_y.push(rect.y);
        bboxes_w.push(rect.width);
        bboxes_h.push(rect.height);
    }

    // 2. 创建 Arrow 数组 (Arrays)
    let class_array = StringArray::from(class_names);
    let conf_array = Float32Array::from(confidences);
    let x_array = Int32Array::from(bboxes_x);
    let y_array = Int32Array::from(bboxes_y);
    let w_array = Int32Array::from(bboxes_w);
    let h_array = Int32Array::from(bboxes_h);

    // 3. 定义 Schema (Struct)
    let fields = Fields::from(vec![
        Field::new("class_name", DataType::Utf8, false),
        Field::new("confidence", DataType::Float32, false),
        Field::new("bbox_x", DataType::Int32, false),
        Field::new("bbox_y", DataType::Int32, false),
        Field::new("bbox_w", DataType::Int32, false),
        Field::new("bbox_h", DataType::Int32, false),
    ]);
    let schema = Arc::new(Schema::new(fields));

    // 4. 创建 StructArray
    let arrays: Vec<ArrayRef> = vec![
        Arc::new(class_array),
        Arc::new(conf_array),
        Arc::new(x_array),
        Arc::new(y_array),
        Arc::new(w_array),
        Arc::new(h_array),
    ];

    let struct_array = StructArray::new(schema.fields.clone(), arrays, None);

    Ok(struct_array)
}

/// 将 Arrow StructArray 转换为 Vec<(&str, Rect, f32)>
pub fn arrow_to_bboxes(
    struct_array: &StructArray,
) -> Result<Vec<(String, Rect, f32)>, Box<dyn std::error::Error>> {
    // ⚠️ 注意：反序列化后，类别名称将是 String，而不是 &'static str。
    // 这在 Rust 中是更安全、更通用的做法。

    let len = struct_array.len();

    // 1. 获取子数组 (Arrays)
    let class_array = struct_array
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .context("Missing or incorrect class_name array")?;
    let conf_array = struct_array
        .column(1)
        .as_any()
        .downcast_ref::<Float32Array>()
        .context("Missing or incorrect confidence array")?;
    let x_array = struct_array
        .column(2)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("Missing or incorrect bbox_x array")?;
    // ... 依此类推获取 y, w, h 数组 ...
    let y_array = struct_array
        .column(3)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("Missing or incorrect bbox_y array")?;
    let w_array = struct_array
        .column(4)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("Missing or incorrect bbox_w array")?;
    let h_array = struct_array
        .column(5)
        .as_any()
        .downcast_ref::<Int32Array>()
        .context("Missing or incorrect bbox_h array")?;

    let mut bboxes = Vec::with_capacity(len);

    // 2. 遍历并重组数据
    for i in 0..len {
        let name = class_array.value(i).to_owned();
        let conf = conf_array.value(i);
        let x = x_array.value(i);
        let y = y_array.value(i);
        let w = w_array.value(i);
        let h = h_array.value(i);

        let rect = Rect::new(x, y, w, h);

        bboxes.push((name, rect, conf));
    }

    Ok(bboxes)
}
