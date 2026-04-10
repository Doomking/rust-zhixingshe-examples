use log::*;
use std::net::TcpStream;
use std::io::{Read, Write};
use std::time::Duration;
use std::thread;
use crate::{get_status, get_flip_channel, get_audio_channel, get_voice_channel};

const MAGIC: u8 = 0x5A;

// Message Types
const TYPE_METRICS: u8 = 0x01;
const TYPE_FLIP: u8 = 0x0F;
const TYPE_AUDIO: u8 = 0x11;

fn send_packet(writer: &mut TcpStream, msg_type: u8, payload: &[u8]) -> std::io::Result<()> {
    let len = payload.len() as u16;
    let header = [
        MAGIC,
        msg_type,
        (len & 0xFF) as u8,
        ((len >> 8) & 0xFF) as u8,
    ];
    writer.write_all(&header)?;
    writer.write_all(payload)?;
    Ok(())
}

pub fn tcp_thread() {
    let mac_ip = env!("SERVER_IP");
    let server_addr = format!("{}:8080", mac_ip);

    loop {
        info!("Connecting to Hub Server at {}...", server_addr);
        
        let stream = match TcpStream::connect_timeout(
            &server_addr.parse().unwrap(), 
            Duration::from_secs(5)
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!("Connection failed: {:?}. Retrying in 2s...", e);
                thread::sleep(Duration::from_secs(2));
                continue;
            }
        };

        info!("Connected to Hub Server!");
        stream.set_nonblocking(true).unwrap();
        stream.set_nodelay(true).unwrap_or(());

        let mut writer = stream;

        // Main loop: Non-blocking read + channel drains
        let (_, flip_rx_mutex) = get_flip_channel();
        let (_, audio_rx_mutex) = get_audio_channel();
        let (_, voice_rx_mutex) = get_voice_channel();

        let mut header_buf = [0u8; 4];
        let mut session_alive = true;

        while session_alive {
            // 1. Non-blocking read from server (metrics)
            match writer.read_exact(&mut header_buf) {
                Ok(_) => {
                    if header_buf[0] == TYPE_METRICS {
                        let cpu = header_buf[1];
                        let mem = header_buf[2];
                        if let Ok(mut status) = get_status().write() {
                            status.cpu_usage = cpu;
                            status.mem_usage = mem;
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, continue
                }
                Err(_) => {
                    warn!("TCP Read Error. Ending session.");
                    session_alive = false;
                    continue;
                }
            }

            // 2. Check for flip event
            if let Ok(rx) = flip_rx_mutex.lock() {
                if let Ok(_) = (*rx).try_recv() {
                    if send_packet(&mut writer, TYPE_FLIP, &[]).is_err() {
                        session_alive = false;
                        continue;
                    }
                }
            }

            // 3. Check for Voice State Events
            if let Ok(rx) = voice_rx_mutex.lock() {
                while let Ok(event_type) = (*rx).try_recv() {
                    info!("TCP: Sending Voice Event 0x{:02X}", event_type);
                    if send_packet(&mut writer, event_type as u8, &[]).is_err() {
                        session_alive = false;
                        break;
                    }
                }
            }
            if !session_alive { continue; }

            // 4. Check for Audio Data Chunks
            if let Ok(rx_guard) = audio_rx_mutex.lock() {
                while let Ok(pcm) = (*rx_guard).try_recv() {
                    if send_packet(&mut writer, TYPE_AUDIO, &pcm).is_err() {
                        error!("TCP Audio Write Error. Breaking session.");
                        session_alive = false;
                        break;
                    }
                }
            }

            thread::sleep(Duration::from_millis(10));
        }

        warn!("TCP Session finished. Reconnecting in 2s...");
        thread::sleep(Duration::from_secs(2));
    }
}
