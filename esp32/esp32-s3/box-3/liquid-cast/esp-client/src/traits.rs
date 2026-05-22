pub trait VideoOutput {
    /// Draw a frame into the output device.
    /// Returns the number of pixels drawn.
    fn draw_frame(&mut self, width: u32, height: u32, pixels: &[u16]) -> anyhow::Result<usize>;

    /// Get the preferred buffer for rendering to avoid extra copies.
    fn get_backbuffer(&mut self) -> &mut [u16];
    /// Draw the content already present in the backbuffer.
    fn draw_from_backbuffer(&mut self, width: u32, height: u32) -> anyhow::Result<usize>;
}
