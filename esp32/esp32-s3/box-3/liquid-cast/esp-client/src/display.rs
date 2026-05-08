use esp_idf_svc::hal::spi::SpiDeviceDriver;
use esp_idf_svc::hal::gpio;
use mipidsi::options::{Orientation, Rotation, ColorOrder};
use mipidsi::Builder;

pub type AppSpi<'d> = SpiDeviceDriver<'d, esp_idf_svc::hal::spi::SpiDriver<'d>>;
pub type AppDc<'d> = gpio::PinDriver<'d, gpio::Output>;
pub type AppRst<'d> = gpio::PinDriver<'d, gpio::Output>;
pub type AppBl<'d> = gpio::PinDriver<'d, gpio::Output>;

pub struct DisplayManager<'d> {
    spi: AppSpi<'d>,
    dc: AppDc<'d>,
    // Keep control pins alive for the whole runtime (same pattern as liquid-stream).
    _rst: AppRst<'d>,
    _backlight: AppBl<'d>,
    frame_buf: Vec<u16>,
}

impl<'d> DisplayManager<'d> {
    pub fn new(
        spi_device: AppSpi<'d>,
        dc: AppDc<'d>,
        mut rst: AppRst<'d>,
        mut backlight: AppBl<'d>,
    ) -> anyhow::Result<Self> {
        let di = display_interface_spi::SPIInterface::new(spi_device, dc);

        // Match liquid-stream reset cadence exactly.
        rst.set_high()?;
        esp_idf_svc::hal::delay::Ets::delay_ms(20);
        rst.set_low()?;
        esp_idf_svc::hal::delay::Ets::delay_ms(150);

        let display = Builder::new(mipidsi::models::ILI9342CRgb565, di)
            .orientation(Orientation::new().rotate(Rotation::Deg180))
            .color_order(ColorOrder::Bgr)
            .init(&mut esp_idf_svc::hal::delay::Ets)
            .map_err(|_| anyhow::anyhow!("Display init failed"))?;

        // Extract raw interfaces for fast DMA blit
        let (di, _, _) = display.release();
        let (spi, dc) = di.release();

        backlight.set_high()?;

        Ok(Self {
            spi,
            dc,
            _rst: rst,
            _backlight: backlight,
            frame_buf: vec![0u16; 320 * 240], // Will allocate in PSRAM if enabled
        })
    }

    pub fn draw_rgb565_be_pixels(&mut self, width: u32, height: u32, rgb565_be_pixels: &[u16]) -> anyhow::Result<()> {
        let needed = (width as usize).saturating_mul(height as usize);
        let count = needed.min(self.frame_buf.len()).min(rgb565_be_pixels.len());
        self.frame_buf[..count].copy_from_slice(&rgb565_be_pixels[..count]);

        // Same raw path as verified `liquid-stream`: RAMWR then contiguous DMA push.
        self.dc.set_low()?;
        self.spi.write(&[0x2C])?;
        self.dc.set_high()?;

        let bytes = unsafe {
            std::slice::from_raw_parts(self.frame_buf.as_ptr() as *const u8, self.frame_buf.len() * 2)
        };

        // 32KB chunks for DMA
        let chunk_size = 32768;
        for chunk in bytes.chunks(chunk_size) {
            self.spi.write(chunk)?;
        }
        Ok(())
    }
}
