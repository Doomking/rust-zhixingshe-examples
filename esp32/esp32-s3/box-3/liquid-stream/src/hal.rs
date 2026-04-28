use esp_idf_sys as _;
use esp_idf_svc::hal::{gpio, spi, i2c, delay::Ets, peripherals::Peripherals, units::FromValueType};
use mipidsi::{models::ILI9342CRgb565, options::{Orientation, Rotation, ColorOrder}, Display};
use display_interface_spi::SPIInterface;

pub type AppDisplay = Display<SPIInterface<spi::SpiDeviceDriver<'static, spi::SpiDriver<'static>>, gpio::PinDriver<'static, gpio::Output>>, ILI9342CRgb565, mipidsi::NoResetPin>;

pub struct Hardware {
    pub display: AppDisplay,
    pub i2c0: i2c::I2cDriver<'static>,
    pub rst: gpio::PinDriver<'static, gpio::Output>,
    pub backlight: gpio::PinDriver<'static, gpio::Output>,
}

pub fn init_hardware(peripherals: Peripherals) -> anyhow::Result<Hardware> {
    let pins = peripherals.pins;

    // --- 1. Display Initialization ---
    let spi_config = spi::config::Config::new().baudrate(40_u32.MHz().into());
    let spi_driver = spi::SpiDriver::new(
        peripherals.spi2,
        pins.gpio7, // SCK 
        pins.gpio6, // MOSI
        Option::<gpio::AnyIOPin>::None, // MISO
        &spi::SpiDriverConfig::new(),
    )?;

    let spi_device = spi::SpiDeviceDriver::new(spi_driver, Some(pins.gpio5), &spi_config)?;
    let dc = gpio::PinDriver::output(pins.gpio4)?;
    let di = SPIInterface::new(spi_device, dc);

    let mut rst = gpio::PinDriver::output(pins.gpio48)?;
    let mut delay = Ets;

    // BOX-3 特殊复位链路：需要手动发出高脉冲，然后长期拉低 (Active High Reset)
    // 如果交由 mipidsi::Builder 的 reset_pin 处理，它会按照 Active Low 的默认习惯将引脚永久拉高，导致屏幕一直被锁死在复位状态（白屏）。
    rst.set_high()?;
    esp_idf_svc::hal::delay::FreeRtos::delay_ms(20);
    rst.set_low()?;
    esp_idf_svc::hal::delay::FreeRtos::delay_ms(150);

    let display = mipidsi::Builder::new(ILI9342CRgb565, di)
        .color_order(ColorOrder::Bgr)
        .orientation(Orientation::default().rotate(Rotation::Deg180))
        .init(&mut delay)
        .map_err(|_| anyhow::anyhow!("Display init failed"))?;

    let mut backlight = gpio::PinDriver::output(pins.gpio47)?;
    backlight.set_high()?;

    // --- 2. I2C Initialization ---
    let i2c0_config = i2c::I2cConfig::new().baudrate(100_u32.kHz().into());
    let i2c0 = i2c::I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio18, &i2c0_config)?;

    Ok(Hardware { display, i2c0, rst, backlight })
}
