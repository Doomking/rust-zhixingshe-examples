use dora_node_api::{
    arrow::array::{Float32Array, StringArray},
    DoraNode, Event,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let (mut _node, mut events) = DoraNode::init_from_env()?;
    println!("日志节点启动");
    while let Some(event) = events.recv() {
        match event {
            Event::Input {
                id,
                metadata: _,
                data,
            } => match id.as_str() {
                "smoothed" => {
                    let array = data
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .ok_or("转换失败")?;
                    let temp = array.value(0);

                    // 终端柱状图（M1终端性能强劲）
                    let bar = "█".repeat((temp * 2.0) as usize);
                    println!("\r[{:4.1}°C] {}", temp, bar);
                }
                "alert" => {
                    let array = data
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .ok_or("转换失败")?;
                    println!("\n🚨 {}", array.value(0));
                }
                other => eprintln!("Logger： Received input `{}`", other),
            },
            _ => {}
        }
    }

    Ok(())
}
