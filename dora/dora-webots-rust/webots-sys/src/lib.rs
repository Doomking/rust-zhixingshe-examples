#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// 引入 build.rs 生成的代码
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

mod robot;

pub use robot::WebotsRobot;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        // 仅测试链接是否成功
        // 注意：在没有 Webots 环境下运行此测试会报错
        unsafe {
            std::env::set_var("WEBOTS_CONTROLLER_URL", "tcp://127.0.0.1:1234");
            WebotsRobot::new();
        }
    }
}