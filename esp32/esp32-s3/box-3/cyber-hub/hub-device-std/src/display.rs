use crate::SystemStatus;
use crate::fonts::draw_icon;
use embedded_graphics::mono_font::{
    MonoTextStyle,
    ascii::FONT_6X13,
    iso_8859_1::{FONT_9X15, FONT_10X20},
};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Text, Baseline};

// --- PSRAM FrameBuffer for flicker-free rendering ---
pub struct FrameBuffer {
    pub buf: Vec<u16>,
    width: u32,
    height: u32,
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            buf: vec![0u16; (width * height) as usize],
            width,
            height,
        }
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(pos, color) in pixels {
            if pos.x >= 0 && pos.y >= 0 && (pos.x as u32) < self.width && (pos.y as u32) < self.height {
                self.buf[(pos.y as u32 * self.width + pos.x as u32) as usize] =
                    RawU16::from(color).into_inner();
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
        let raw = RawU16::from(color).into_inner();
        let clipped = area.intersection(&Rectangle::new(Point::zero(), self.size()));
        for pos in clipped.points() {
            self.buf[(pos.y as u32 * self.width + pos.x as u32) as usize] = raw;
        }
        Ok(())
    }
}

/// Flush the entire framebuffer to a real display in one contiguous transfer.
pub fn flush_framebuffer<D>(display: &mut D, fb: &FrameBuffer) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    display.fill_contiguous(
        &Rectangle::new(Point::zero(), Size::new(fb.width, fb.height)),
        fb.buf.iter().map(|&raw| Rgb565::from(RawU16::new(raw))),
    )
}

// --- 高效渲染包装器 (ScaledDisplay v2.3) ---
pub struct ScaledDisplay<'a, D> {
    base: &'a mut D,
    scale: u32,
    offset: Point,
    current_row: i32,
    start_x: i32,
    last_x: i32,
    last_color: Option<Rgb565>,
}

impl<'a, D> ScaledDisplay<'a, D>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    pub fn new(base: &'a mut D, scale: u32, offset: Point) -> Self {
        Self {
            base,
            scale,
            offset,
            current_row: -1,
            start_x: -1,
            last_x: -1,
            last_color: None,
        }
    }

    pub fn flush(&mut self) -> Result<(), D::Error> {
        if let Some(color) = self.last_color {
            if self.last_x >= self.start_x {
                let width = (self.last_x - self.start_x + 1) as u32 * self.scale;
                let p = self.offset
                    + Point::new(
                        self.start_x * self.scale as i32,
                        self.current_row * self.scale as i32,
                    );
                if width > 0 && width < 1000 {
                    Rectangle::new(p, Size::new(width, self.scale))
                        .into_styled(PrimitiveStyle::with_fill(color))
                        .draw(self.base)?;
                }
            }
        }
        self.last_color = None;
        Ok(())
    }
}

impl<D> DrawTarget for ScaledDisplay<'_, D>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(pos, color) in pixels {
            if pos.y != self.current_row
                || pos.x != self.last_x + 1
                || Some(color) != self.last_color
            {
                self.flush()?;
                self.current_row = pos.y;
                self.start_x = pos.x;
                self.last_color = Some(color);
            }
            self.last_x = pos.x;
        }
        self.flush()?;
        Ok(())
    }
}

impl<D> OriginDimensions for ScaledDisplay<'_, D>
where
    D: OriginDimensions,
{
    fn size(&self) -> Size {
        self.base.size()
    }
}

pub fn draw_static_ui<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    display.clear(Rgb565::BLACK)?;
    Ok(())
}

pub fn clear_area<D>(display: &mut D, rect: Rectangle) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    rect.into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(display)
}

fn get_date_weekday(unix_time: u64) -> (u16, u8, u8, &'static str, &'static str) {
    let days = (unix_time / 86400) as i32;
    let weekday = (days + 4) % 7;
    let weekdays = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let mut y = 1970;
    let mut d = days;
    loop {
        let leap = (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) as i32;
        let y_days = 365 + leap;
        if d < y_days {
            break;
        }
        d -= y_days;
        y += 1;
    }

    let leap = (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) as i32;
    let mut m = 1;
    let month_days = [31, 28 + leap, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for &md in &month_days {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    (
        y as u16,
        m as u8,
        (d + 1) as u8,
        weekdays[weekday as usize],
        months[(m - 1) as usize],
    )
}

pub fn draw_time<D>(display: &mut D, unix_time: u64) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    let hours = (unix_time / 3600) % 24;
    let mins = (unix_time / 60) % 60;
    let mut time_str = [0u8; 5];
    time_str[0] = b'0' + (hours / 10) as u8;
    time_str[1] = b'0' + (hours % 10) as u8;
    time_str[2] = b':';
    time_str[3] = b'0' + (mins / 10) as u8;
    time_str[4] = b'0' + (mins % 10) as u8;
    let time_text = unsafe { core::str::from_utf8_unchecked(&time_str) };

    // --- 紧凑式布局 (v27 微调上移) ---
    // 清理 110px，让整体布局更紧凑一点
    clear_area(
        display,
        Rectangle::new(Point::new(0, 0), Size::new(320, 110)),
    )?;
    let mut clock_display = ScaledDisplay::new(display, 3, Point::new(85, 10));
    Text::with_baseline(
        time_text,
        Point::new(0, 0),
        MonoTextStyle::new(&FONT_10X20, Rgb565::new(31, 61, 28)), // #FFF4E0 Warm White
        Baseline::Top
    )
    .draw(&mut clock_display)?;

    let (y, _m, d, week, month) = get_date_weekday(unix_time);
    let mut date_full = [0u8; 20];
    let mut ptr = 0;
    date_full[ptr..ptr + 3].copy_from_slice(week.as_bytes());
    ptr += 3;
    date_full[ptr] = b' ';
    ptr += 1;
    if d >= 10 {
        date_full[ptr] = b'0' + (d / 10);
        ptr += 1;
    }
    date_full[ptr] = b'0' + (d % 10);
    ptr += 1;
    date_full[ptr] = b' ';
    ptr += 1;
    date_full[ptr..ptr + 3].copy_from_slice(month.as_bytes());
    ptr += 3;
    date_full[ptr] = b' ';
    ptr += 1;
    date_full[ptr] = b'0' + (y / 1000) as u8;
    date_full[ptr + 1] = b'0' + ((y / 100) % 10) as u8;
    date_full[ptr + 2] = b'0' + ((y / 10) % 10) as u8;
    date_full[ptr + 3] = b'0' + (y % 10) as u8;
    ptr += 4;
    let date_str = unsafe { core::str::from_utf8_unchecked(&date_full[..ptr]) };

    let date_style = MonoTextStyle::new(&FONT_10X20, Rgb565::new(26, 51, 25)); // #D7CCC8 Warm Beige
    let date_width = ptr * 10;
    let date_x = (320 - date_width as i32) / 2;
    // 使用 Top 对齐，设为 80
    Text::with_baseline(date_str, Point::new(date_x, 80), date_style, Baseline::Top).draw(display)?;
    Ok(())
}

pub fn draw_metrics<D>(display: &mut D, status: &SystemStatus) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    let hw_color = Rgb565::new(16, 53, 31); // #81D4FA Light Blue 200
    let env_color = Rgb565::new(16, 57, 15); // #86E57F Mint Green
    let hw_label_style = MonoTextStyle::new(&FONT_9X15, hw_color);
    let hw_val_style = MonoTextStyle::new(&FONT_10X20, hw_color);
    let env_label_style = MonoTextStyle::new(&FONT_9X15, env_color);
    let env_val_style = MonoTextStyle::new(&FONT_10X20, env_color);

    // --- 居中对齐与微调上抬 (v27) ---
    let y1 = 142; // Row 1 (CPU/TEMP)
    let y2 = 187; // Row 2 (RAM/HUM)

    // CPU / RAM
    clear_area(
        display,
        Rectangle::new(Point::new(0, y1), Size::new(160, 45)),
    )?;
    // 标签 y+12 垂直居中于 40px 的数值 (整体右移至 35)
    Text::with_baseline("CPU:", Point::new(35, y1 + 12), hw_label_style, Baseline::Top).draw(display)?;
    let mut cpu_buf = [0u8; 2];
    let cpu_val = format_num(&mut cpu_buf, status.cpu_usage);
    let mut cpu_display = ScaledDisplay::new(display, 2, Point::new(75, y1));
    Text::with_baseline(cpu_val, Point::new(0, 0), hw_val_style, Baseline::Top).draw(&mut cpu_display)?;
    // "%" 脚对齐修正 (相对左移一点，设置在 118)
    Text::with_baseline("%", Point::new(118, y1 + 35), hw_label_style, Baseline::Bottom).draw(display)?;

    clear_area(
        display,
        Rectangle::new(Point::new(0, y2), Size::new(160, 45)),
    )?;
    Text::with_baseline("RAM:", Point::new(35, y2 + 12), hw_label_style, Baseline::Top).draw(display)?;
    let mut ram_buf = [0u8; 2];
    let ram_val = format_num(&mut ram_buf, status.mem_usage);
    let mut ram_display = ScaledDisplay::new(display, 2, Point::new(75, y2));
    Text::with_baseline(ram_val, Point::new(0, 0), hw_val_style, Baseline::Top).draw(&mut ram_display)?;
    Text::with_baseline("%", Point::new(118, y2 + 35), hw_label_style, Baseline::Bottom).draw(display)?;

    // Temp / Hum (整体右移至 180)
    let env_offset = 180;
    clear_area(
        display,
        Rectangle::new(Point::new(env_offset, y1), Size::new(140, 45)),
    )?;
    draw_icon(
        display,
        "temp_icon",
        Point::new(env_offset + 5, y1 + 15),
        env_color,
    )?;
    let mut temp_buf = [0u8; 2];
    let temp_text = format_num(&mut temp_buf, status.local_temp as u8);
    let mut temp_display = ScaledDisplay::new(display, 2, Point::new(env_offset + 35, y1));
    Text::with_baseline(temp_text, Point::new(0, 0), env_val_style, Baseline::Top).draw(&mut temp_display)?;
    Text::with_baseline("°C", Point::new(env_offset + 75, y1 + 35), env_label_style, Baseline::Bottom).draw(display)?;

    clear_area(
        display,
        Rectangle::new(Point::new(env_offset, y2), Size::new(140, 45)),
    )?;
    draw_icon(
        display,
        "hum_icon",
        Point::new(env_offset + 5, y2 + 15),
        env_color,
    )?;
    let mut hum_buf = [0u8; 2];
    let hum_text = format_num(&mut hum_buf, status.local_hum);
    let mut hum_display = ScaledDisplay::new(display, 2, Point::new(env_offset + 35, y2));
    Text::with_baseline(hum_text, Point::new(0, 0), env_val_style, Baseline::Top).draw(&mut hum_display)?;
    Text::with_baseline("%", Point::new(env_offset + 80, y2 + 35), env_label_style, Baseline::Bottom).draw(display)?;
    Ok(())
}

pub fn draw_weather<D>(display: &mut D, status: &SystemStatus) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    // --- 紧致化气象行 (v27: Zone 110-140) ---
    clear_area(
        display,
        Rectangle::new(Point::new(0, 110), Size::new(320, 30)),
    )?;

    // Prioritize Voice State Over Weather
    if status.voice_state == 1 {
        let status_style = MonoTextStyle::new(&FONT_10X20, Rgb565::GREEN);
        let status_text = "● LISTENING...";
        let x = (320 - (status_text.len() as i32 * 10)) / 2;
        Text::with_baseline(status_text, Point::new(x, 115), status_style, Baseline::Top).draw(display)?;
        return Ok(());
    } else if status.voice_state == 2 {
        let status_style = MonoTextStyle::new(&FONT_10X20, Rgb565::new(31, 45, 9)); // Amber
        let status_text = "PROCESSING...";
        let x = (320 - (status_text.len() as i32 * 10)) / 2;
        Text::with_baseline(status_text, Point::new(x, 115), status_style, Baseline::Top).draw(display)?;
        return Ok(());
    }

    let desc_raw =
        unsafe { core::str::from_utf8_unchecked(&status.weather_desc_en) }.trim_matches('\0');
    if !desc_raw.is_empty() {
        let mut city_buf = [0u8; 16];
        let mut city_len = 0;
        for (i, &b) in status.city_name.iter().enumerate() {
            if b == 0 {
                break;
            }
            let mut char_b = b;
            if b >= b'a' && b <= b'z' {
                char_b = b - 32;
            }
            city_buf[i] = char_b;
            city_len += 1;
        }
        let city_text = unsafe { core::str::from_utf8_unchecked(&city_buf[..city_len]) };

        let mut desc_up = [0u8; 16];
        let mut desc_len = 0;
        for (i, &b) in desc_raw.as_bytes().iter().enumerate() {
            let mut char_b = b;
            if b >= b'a' && b <= b'z' {
                char_b = b - 32;
            }
            desc_up[i] = char_b;
            desc_len += 1;
        }
        let desc_text = unsafe { core::str::from_utf8_unchecked(&desc_up[..desc_len]) };
        let mut temp_buf = [0u8; 8];
        let t_str = format_temp_simple(&mut temp_buf, status.temperature);
        let mut wind_buf = [0u8; 3];
        let w_val = format_wind(&mut wind_buf, status.wind_speed);

        let full_str = format!("{} {}  {}  {}KPH", city_text, desc_text, t_str, w_val);
        let font_style = MonoTextStyle::new(&FONT_10X20, Rgb565::new(31, 45, 9)); // #FFB74D Amber Orange
        let total_w = full_str.chars().count() as i32 * 10;
        let x = (320 - total_w) / 2;
        // 放置在 Zone 中心 (110-140) -> 115
        Text::with_baseline(&full_str, Point::new(x, 115), font_style, Baseline::Top).draw(display)?;
    } else {
        Text::with_baseline(
            "WEATHER UPDATING...",
            Point::new(95, 110),
            MonoTextStyle::new(&FONT_6X13, Rgb565::new(150, 150, 150)),
            Baseline::Top
        )
        .draw(display)?;
    }
    Ok(())
}

pub fn draw_dashboard<D>(display: &mut D, status: &SystemStatus) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565> + OriginDimensions,
{
    draw_time(display, status.unix_time)?;
    draw_weather(display, status)?;
    draw_metrics(display, status)?;
    Ok(())
}

fn format_num<'a>(buf: &'a mut [u8], val: u8) -> &'a str {
    let v = if val > 99 { 99 } else { val };
    buf[0] = b'0' + (v / 10);
    buf[1] = b'0' + (v % 10);
    unsafe { core::str::from_utf8_unchecked(&buf[..2]) }
}

pub fn format_num_spaced<'a>(buf: &'a mut [u8], val: u8) -> &'a str {
    let v = if val > 99 { 99 } else { val };
    buf[0] = b'0' + (v / 10);
    buf[1] = b'0' + (v % 10);
    buf[2] = b' ';
    unsafe { core::str::from_utf8_unchecked(&buf[..2]) }
}

fn format_temp_simple<'a>(buf: &'a mut [u8], temp: i8) -> &'a str {
    let mut pos = 0;
    let mut t = temp;
    if t < 0 {
        buf[pos] = b'-';
        pos += 1;
        t = -t;
    }
    if t >= 10 {
        buf[pos] = b'0' + (t / 10) as u8;
        pos += 1;
    }
    buf[pos] = b'0' + (t % 10) as u8;
    buf[pos + 1] = 0xc2; // Degree symbol (UTF-8 part 1)
    buf[pos + 2] = 0xb0; // Degree symbol (UTF-8 part 2)
    buf[pos + 3] = b'C';
    unsafe { core::str::from_utf8_unchecked(&buf[..pos + 4]) }
}

fn format_wind<'a>(buf: &'a mut [u8], wind: u8) -> &'a str {
    let mut pos = 0;
    let w = if wind > 99 { 99 } else { wind };
    if w >= 10 {
        buf[pos] = b'0' + (w / 10);
        pos += 1;
    }
    buf[pos] = b'0' + (w % 10);
    unsafe { core::str::from_utf8_unchecked(&buf[..pos + 1]) }
}

pub fn draw_cyber_hub_ui<D>(display: &mut D, status_msg: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Rgb565>,
{
    display.clear(Rgb565::BLACK)?;
    let title_style = MonoTextStyle::new(&FONT_10X20, Rgb565::GREEN);
    Text::new("CYBER-HUB", Point::new(100, 50), title_style).draw(display)?;
    let status_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    Text::new(status_msg, Point::new(30, 120), status_style).draw(display)?;
    Ok(())
}
