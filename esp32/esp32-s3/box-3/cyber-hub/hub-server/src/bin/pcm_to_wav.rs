use std::env;
use std::fs::File;
use std::io::Read;
use hound::{WavWriter, WavSpec, SampleFormat};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run --bin pcm_to_wav <input.pcm>");
        return Ok(());
    }

    let input_path = &args[1];
    let output_path = input_path.replace(".pcm", "_fixed.wav");

    let mut input_file = File::open(input_path)?;
    let mut buffer = Vec::new();
    input_file.read_to_end(&mut buffer)?;

    let spec = WavSpec {
        channels: 2,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(&output_path, spec)?;
    for chunk in buffer.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        writer.write_sample(sample)?;
    }
    writer.finalize()?;

    println!("Converted {} to {}", input_path, output_path);
    Ok(())
}
