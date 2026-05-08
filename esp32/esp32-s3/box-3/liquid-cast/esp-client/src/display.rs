use esp_idf_svc::hal::spi::SpiDeviceDriver;
use esp_idf_svc::hal::gpio;
use mipidsi::options::{Orientation, Rotation, ColorOrder};
use mipidsi::Builder;
use crate::traits::VideoOutput;

pub type AppSpi<'d> = SpiDeviceDriver<'d, esp_idf_svc::hal::spi::SpiDriver<'d>>;
pub type AppDc<'d> = gpio::PinDriver<'d, gpio::Output>;
pub type AppRst<'d> = gpio::PinDriver<'d, gpio::Output>;
pub type AppBl<'d> = gpio::PinDriver<'d, gpio::Output>;

pub struct DisplayManager<'d> {
    spi: AppSpi<'d>,
    dc: AppDc<'d>,
    _rst: AppRst<'d>,
    _backlight: AppBl<'d>,
    frame_buf: Vec<u16>,
}

impl<'d> VideoOutput for DisplayManager<'d> {
    fn draw_frame(&mut self, width: u32, height: u32, pixels: &[u16]) -> anyhow::Result<usize> {
        let needed = (width as usize).saturating_mul(height as usize);
        let count = needed.min(self.frame_buf.len()).min(pixels.len());
        
        // If pixels is not our own frame_buf, we copy.
        // In the optimized path, we decode directly into get_backbuffer(), so this copy is skipped if pointers match.
        if !std::ptr::eq(self.frame_buf.as_ptr(), pixels.as_ptr()) {
            self.frame_buf[..count].copy_from_slice(&pixels[..count]);
        }

        self.dc.set_low()?;
        self.spi.write(&[0x2C])?;
        self.dc.set_high()?;

        let bytes = unsafe {
            std::slice::from_raw_parts(self.frame_buf.as_ptr() as *const u8, self.frame_buf.len() * 2)
        };

        let chunk_size = 32768;
        for chunk in bytes.chunks(chunk_size) {
            self.spi.write(chunk)?;
        }
        Ok(count)
    }

    fn get_backbuffer(&mut self) -> &mut [u16] {
        &mut self.frame_buf
    }

    fn draw_from_backbuffer(&mut self, width: u32, height: u32) -> anyhow::Result<usize> {
        let count = (width as usize).saturating_mul(height as usize).min(self.frame_buf.len());
        
        self.dc.set_low()?;
        self.spi.write(&[0x2C])?;
        self.dc.set_high()?;

        let bytes = unsafe {
            std::slice::from_raw_parts(self.frame_buf.as_ptr() as *const u8, self.frame_buf.len() * 2)
        };

        let chunk_size = 32768;
        for chunk in bytes.chunks(chunk_size) {
            self.spi.write(chunk)?;
        }
        Ok(count)
    }
}

impl<'d> DisplayManager<'d> {
    pub fn new(
        spi_device: AppSpi<'d>,
        dc: AppDc<'d>,
        mut rst: AppRst<'d>,
        mut backlight: AppBl<'d>,
    ) -> anyhow::Result<Self> {
        let di = display_interface_spi::SPIInterface::new(spi_device, dc);

        rst.set_high()?;
        esp_idf_svc::hal::delay::Ets::delay_ms(20);
        rst.set_low()?;
        esp_idf_svc::hal::delay::Ets::delay_ms(150);

        let display = Builder::new(mipidsi::models::ILI9342CRgb565, di)
            .orientation(Orientation::new().rotate(Rotation::Deg180))
            .color_order(ColorOrder::Bgr)
            .init(&mut esp_idf_svc::hal::delay::Ets)
            .map_err(|_| anyhow::anyhow!("Display init failed"))?;

        let (di, _, _) = display.release();
        let (spi, dc) = di.release();

        backlight.set_high()?;

        Ok(Self {
            spi,
            dc,
            _rst: rst,
            _backlight: backlight,
            frame_buf: vec![0u16; 320 * 240],
        })
    }
}
