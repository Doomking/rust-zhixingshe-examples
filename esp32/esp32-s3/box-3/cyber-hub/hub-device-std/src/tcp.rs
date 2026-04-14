use std::net::TcpStream;
use std::io::{Read, Write};
use std::thread;
use log::*;
use crate::voice_prompts::COMMAND_DONE_PCM;
use crate::{enqueue_playback_pcm, get_audio_channel, get_flip_channel, get_voice_channel};

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
                let mut parse_buf: Vec<u8> = Vec::with_capacity(1024);
                let mut read_buf = [0u8; 512];

                loop {
                    // Helper to send scoped packets
                    let mut write_res = Ok(());

                    // 1. Check if we need to send audio data
                    if let Ok(rx_guard) = audio_ch.1.lock() {
                        while let Ok(pcm_vec) = rx_guard.try_recv() {
                            let mut header = [crate::protocol::MAGIC_HEADER, crate::protocol::MSG_VOICE_DATA, 0, 0];
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
                                let header = [crate::protocol::MAGIC_HEADER, e as u8, 0, 0];
                                write_res = stream.write_all(&header);
                                if write_res.is_err() { break; }
                            }
                        }
                    }

                    // 3. Check for flip events
                    if write_res.is_ok() {
                        if let Ok(rx_guard) = flip_ch.1.lock() {
                            while let Ok(_) = rx_guard.try_recv() {
                                let header = [crate::protocol::MAGIC_HEADER, crate::protocol::MSG_FLIP_EVENT, 0, 0];
                                write_res = stream.write_all(&header);
                                if write_res.is_err() { break; }
                            }
                        }
                    }

                    if write_res.is_err() {
                        error!("TCP Write Error, reconnecting...");
                        break;
                    }

                    // 4. Read server packets (metrics, feedback cues, …)
                    if let Ok(n) = stream.read(&mut read_buf) {
                        if n == 0 {
                            error!("TCP EOF, reconnecting...");
                            break;
                        }
                        parse_buf.extend_from_slice(&read_buf[..n]);
                    }
                    while parse_buf.len() >= 4 {
                        if parse_buf[0] != crate::protocol::MAGIC_HEADER {
                            parse_buf.remove(0);
                            continue;
                        }
                        let msg_type = parse_buf[1];
                        let payload_len =
                            u16::from_le_bytes([parse_buf[2], parse_buf[3]]) as usize;
                        if parse_buf.len() < 4 + payload_len {
                            break;
                        }
                        let packet: Vec<u8> = parse_buf.drain(..4 + payload_len).collect();
                        let payload = &packet[4..];

                        match msg_type {
                            crate::protocol::MSG_METRICS if payload.len() >= 2 => {
                                let cpu = payload[0];
                                let mem = payload[1];
                                if let Ok(mut status) = crate::get_status().write() {
                                    status.cpu_usage = cpu;
                                    status.mem_usage = mem;
                                }
                            }
                            crate::protocol::MSG_FEEDBACK if payload.is_empty() => {
                                debug!("[PLAYBACK] received done cue from server");
                                enqueue_playback_pcm(COMMAND_DONE_PCM.to_vec());
                            }
                            crate::protocol::MSG_FEEDBACK => {
                                debug!("[PLAYBACK] received {} bytes downlink PCM", payload.len());
                                enqueue_playback_pcm(payload.to_vec());
                            }
                            _ => {}
                        }
                    }

                    thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            Err(e) => {
                error!("TCP Connect Fail: {:?}. Retrying in 5s...", e);
                thread::sleep(std::time::Duration::from_secs(5));
            }
        }
    }
}
