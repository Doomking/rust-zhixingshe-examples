use std::net::TcpStream;
use std::io::{Read, Write};
use std::thread;
use log::*;
use crate::{get_flip_channel, get_voice_channel, get_audio_channel};

pub fn tcp_thread() {
    info!("Starting TCP Client thread...");
    
    // We need to connect to the hub-server
    // Assuming 192.168.1.100 for now, should be dynamic
    let server_ip = env!("SERVER_IP");
    let server_addr = format!("{}:8080", server_ip);

    loop {
        match TcpStream::connect(&server_addr) {
            Ok(mut stream) => {
                info!("TCP Connected to {}", server_addr);
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(100)));
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(1000)));

                let flip_ch = get_flip_channel();
                let voice_ch = get_voice_channel();
                let audio_ch = get_audio_channel();

                loop {
                    // Helper to send scoped packets
                    let mut write_res = Ok(());

                    // 1. Check if we need to send audio data
                    if let Ok(rx_guard) = audio_ch.1.lock() {
                        while let Ok(pcm_vec) = rx_guard.try_recv() {
                            let mut header = [0x5A, 0x11, 0, 0];
                            let len = (pcm_vec.len() as u16).to_le_bytes();
                            header[2..4].copy_from_slice(&len);
                            write_res = stream.write_all(&header).and_then(|_| stream.write_all(&pcm_vec));
                            if write_res.is_err() { break; }
                        }
                    }

                    // 2. Check for voice events (Start 0x10, End 0x12)
                    if write_res.is_ok() {
                        if let Ok(rx_guard) = voice_ch.1.lock() {
                            while let Ok(e) = rx_guard.try_recv() {
                                let header = [0x5A, e as u8, 0, 0];
                                write_res = stream.write_all(&header);
                                if write_res.is_err() { break; }
                            }
                        }
                    }

                    // 3. Check for flip events (0x0F)
                    if write_res.is_ok() {
                        if let Ok(rx_guard) = flip_ch.1.lock() {
                            while let Ok(_) = rx_guard.try_recv() {
                                let header = [0x5A, 0x0F, 0, 0];
                                write_res = stream.write_all(&header);
                                if write_res.is_err() { break; }
                            }
                        }
                    }

                    if write_res.is_err() {
                        error!("TCP Write Error, reconnecting...");
                        break;
                    }

                    // 4. Try to read heartbeats (Type 0x01)
                    let mut buffer = [0u8; 4];
                    if let Ok(n) = stream.read(&mut buffer) {
                        if n == 4 && buffer[0] == 0x01 {
                            // Update system metrics if needed
                        }
                    }

                    thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            Err(e) => {
                error!("TCP Connect Fail: {:?}. Retrying in 5s...", e);
                thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}
