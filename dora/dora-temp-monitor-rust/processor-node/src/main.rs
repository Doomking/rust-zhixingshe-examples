use dora_node_api::{
    arrow::array::{Float32Array, StringArray},
    dora_core::config::DataId,
    DoraNode, Event,
};
use std::collections::VecDeque;
use std::error::Error;

struct TemperatureProcessor {
    window: VecDeque<f32>,
    threshold: f32, // 异常阈值
}

impl TemperatureProcessor {
    fn new(window_size: usize, threshold: f32) -> Self {
        Self {
            window: VecDeque::with_capacity(window_size),
            threshold,
        }
    }

    fn process(&mut self, temp: f32) -> (f32, Option<String>) {
        self.window.push_back(temp);
        if self.window.len() > 10 {
            self.window.pop_front();
        }

        let avg = self.window.iter().sum::<f32>() / self.window.len() as f32;

        // 异常检测逻辑
        let alert = if (temp - avg).abs() > self.threshold {
            Some(format!(
                "⚠️ 温度突变: {:.1}°C (偏离均值{:.1}°C)",
                temp,
                (temp - avg).abs()
            ))
        } else {
            None
        };

        (avg, alert)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut node, mut events) = DoraNode::init_from_env()?;
    let output_temp_smoothed = DataId::from("temp_smoothed".to_owned());
    let output_temp_alert = DataId::from("temp_smootemp_alertthed".to_owned());
    println!("🧮 处理器节点启动 (滑动窗口+异常检测)");

    let mut processor = TemperatureProcessor::new(10, 3.0);

    while let Some(event) = events.recv() {
        match event {
            Event::Input { id, metadata, data } => match id.as_str() {
                "temp" => {
                    let array = data
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .ok_or("类型转换失败")?;
                    let temp = array.value(0) as f32;

                    let (avg, alert) = processor.process(temp);

                    // 发送平滑数据
                    let avg_array = Float32Array::from(vec![avg]);
                    node.send_output(
                        output_temp_smoothed.clone(),
                        metadata.parameters.clone(),
                        avg_array,
                    )?;

                    // 如果有异常，发送警报
                    if let Some(alert_msg) = alert {
                        let alert_array = StringArray::from(vec![alert_msg]);
                        node.send_output(
                            output_temp_alert.clone(),
                            metadata.parameters,
                            alert_array,
                        )?;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}
