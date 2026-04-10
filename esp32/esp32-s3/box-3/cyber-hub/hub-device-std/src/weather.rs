use log::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use crate::get_status;

const WTTR_HOST: &str = "wttr.in";
const WTTR_PORT: u16 = 80;

pub fn weather_thread() {
    let city = env!("WEATHER_CITY");
    info!("[WEATHER] Thread started for city: {}", city);

    loop {
        match fetch_weather(city) {
            Ok(()) => {
                info!("[WEATHER] Sync complete. Next update in 120s.");
                thread::sleep(Duration::from_secs(120));
            }
            Err(e) => {
                warn!("[WEATHER] Fetch failed: {}. Retrying in 30s...", e);
                thread::sleep(Duration::from_secs(30));
            }
        }
    }
}

fn fetch_weather(city: &str) -> Result<(), String> {
    // 1. TCP connect (TcpStream::connect handles DNS via ToSocketAddrs)
    let addr = format!("{}:{}", WTTR_HOST, WTTR_PORT);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("Connect: {}", e))?;

    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

    // 2. Send HTTP request (%l@%C@%t@%w)
    let request = format!(
        "GET /{}?format=%l@%C@%t@%w HTTP/1.1\r\nHost: wttr.in\r\nUser-Agent: curl/7.81.0\r\nConnection: close\r\n\r\n",
        city
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("Write: {}", e))?;

    // 3. Read response
    let mut response = vec![0u8; 2048];
    let mut total = 0;
    loop {
        match stream.read(&mut response[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total >= response.len() { break; }
            }
            Err(_) => break,
        }
    }

    if total == 0 {
        return Err("Empty response".into());
    }

    // 4. Parse HTTP response body
    let resp_str = core::str::from_utf8(&response[..total])
        .map_err(|_| "Invalid UTF-8".to_string())?;

    let body = resp_str
        .find("\r\n\r\n")
        .map(|i| resp_str[i + 4..].trim())
        .ok_or("No HTTP body")?;

    info!("[WEATHER] Raw: '{}'", body);

    // 5. Parse fields: location@condition@temp@wind
    let mut parts = body.split('@');
    let mut city_name = [0u8; 16];
    let mut desc_en = [0u8; 16];
    let mut t_val: i8 = 0;
    let mut w_val: u8 = 0;

    // A. Location
    if let Some(loc) = parts.next() {
        let loc_clean = loc.split(',').next().unwrap_or(loc).trim();
        let lb = loc_clean.as_bytes();
        let len = core::cmp::min(16, lb.len());
        city_name[..len].copy_from_slice(&lb[..len]);
    }

    // B. English Description
    if let Some(desc_raw) = parts.next() {
        let dr = desc_raw.trim();
        let len = core::cmp::min(16, dr.len());
        desc_en[..len].copy_from_slice(&dr.as_bytes()[..len]);
    }

    // C. Temperature
    if let Some(temp_str) = parts.next() {
        let mut t_acc = String::new();
        for c in temp_str.chars() {
            if c.is_ascii_digit() || c == '-' { t_acc.push(c); }
            else if c != '+' && !t_acc.is_empty() { break; }
        }
        t_val = t_acc.parse().unwrap_or(0);
    }

    // D. Wind speed
    if let Some(wind_str) = parts.next() {
        let mut w_acc = String::new();
        for c in wind_str.chars() {
            if c.is_ascii_digit() { w_acc.push(c); }
            else if !w_acc.is_empty() { break; }
        }
        w_val = w_acc.parse().unwrap_or(0);
    }

    // 6. Update global status
    if let Ok(mut status) = get_status().write() {
        status.city_name = city_name;
        status.weather_desc_en = desc_en;
        status.temperature = t_val;
        status.wind_speed = w_val;
    }

    let city_str = core::str::from_utf8(&city_name).unwrap_or("").trim_matches('\0');
    let desc_str = core::str::from_utf8(&desc_en).unwrap_or("").trim_matches('\0');
    info!("[WEATHER] Synced: {} | {}, Temp={}°C, Wind={}km/h", city_str, desc_str, t_val, w_val);

    Ok(())
}
