use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;
use embedded_graphics::mono_font::{ascii::FONT_10X20, MonoTextStyle};

// 绘制赛博朋克风格的赛博枢纽界面
// 我们接收一个泛型 D，它代表了任意实现了 DrawTarget (可以被画画的画布) 的对象
pub fn draw_cyber_hub_ui<D>(display: &mut D, status_msg: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    // 1. 清空屏幕为纯黑
    display.clear(Rgb565::BLACK)?;

    // 2. 绘制一个炫酷的边框
    let screen_style = PrimitiveStyleBuilder::new()
        .stroke_color(Rgb565::CYAN)
        .stroke_width(2)
        .fill_color(Rgb565::BLACK)
        .build();

    let rect = Rectangle::new(Point::new(10, 10), Size::new(300, 220));
    rect.into_styled(screen_style).draw(display)?;

    // 3. 绘制赛博枢纽的标题
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::GREEN);
    Text::new("CYBER-HUB V1.0", Point::new(80, 50), title_style).draw(display)?;

    // 4. 绘制当前状态信息
    let status_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    Text::new(status_msg, Point::new(30, 120), status_style).draw(display)?;

    Ok(())
}
