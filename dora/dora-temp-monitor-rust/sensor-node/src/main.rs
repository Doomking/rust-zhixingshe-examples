use dora_node_api::{arrow::array::Float32Array, dora_core::config::DataId, DoraNode, Event};
use rand::Rng;
use std::error::Error;
use std::time::Instant;

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let output = DataId::from("temp_raw".to_owned());
    println!("🌡️ 传感器节点启动 (M1 Pro优化版)");

    let mut rng = rand::rng();
    let start = Instant::now();

    while let Some(event) = events.recv() {
        // println!("Received event: {:?}", event);
        match event {
            Event::Input {
                id,
                metadata,
                data: _,
            } => match id.as_str() {
                "tick" => {
                    // 模拟带噪声的温度数据
                    let temp = 25.0
                        + rng.random_range(-5.0..5.0)
                        + (start.elapsed().as_secs_f32() * 0.01).sin() * 3.0; // 添加正弦趋势

                    let temp_array = Float32Array::from(vec![temp]);

                    node.send_output(output.clone(), metadata.parameters, temp_array)?;
                }
                other => eprintln!("Received input `{other}`"),
            },
            _ => {}
        }
    }

    Ok(())
}
