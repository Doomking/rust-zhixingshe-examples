use defmt::{info, warn, error};
use embassy_net::tcp::TcpSocket;
use embassy_net::dns::DnsQueryType;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write;
use crate::{SYSTEM_STATUS, STATUS_STATE};
use static_cell::StaticCell;

const WTTR_PORT: u16 = 80;

#[embassy_executor::task]
pub async fn weather_task(stack: Stack<'static>) {
    // 静态内存分配
    static RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();
    static TX_BUF: StaticCell<[u8; 512]> = StaticCell::new();
    static HTTP_BUF: StaticCell<[u8; 2048]> = StaticCell::new();
    
    let rx_buffer = RX_BUF.init([0u8; 1024]);
    let tx_buffer = TX_BUF.init([0u8; 512]);
    let http_buffer = HTTP_BUF.init([0u8; 2048]);

    let city = env!("WEATHER_CITY");
    info!("Weather: task started (v19 English-Only) for city: {}", city);

    loop {
        // 1. 网络状态自检
        while !stack.is_link_up() || !stack.is_config_up() {
            Timer::after(Duration::from_millis(500)).await;
        }

        // 2. DNS 解析
        info!("Weather: Resolving wttr.in...");
        let wttr_ip = match stack.dns_query("wttr.in", DnsQueryType::A).await {
            Ok(ips) if !ips.is_empty() => {
                let ip = ips[0];
                info!("Weather: DNS Resolve Success -> {}", ip);
                ip
            },
            _ => {
                warn!("Weather: DNS Resolve Failed! Domain 'wttr.in' unreachable. Waiting 60s...");
                Timer::after(Duration::from_secs(60)).await;
                continue;
            }
        };

        let mut socket = TcpSocket::new(stack, rx_buffer, tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(15)));

        info!("Weather: Connecting to http://{}:{}...", wttr_ip, WTTR_PORT);
        if let Err(e) = socket.connect((wttr_ip, WTTR_PORT)).await {
            warn!("Weather: TCP Connect Error: {:?}.", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(60)).await;
            continue;
        }

        // 3. 构建请求 (%l@%C@%t@%w)
        let mut req_ptr = 0;
        let mut add_req = |data: &[u8]| {
             http_buffer[req_ptr..req_ptr+data.len()].copy_from_slice(data);
             req_ptr += data.len();
        };
        add_req(b"GET /");
        add_req(city.as_bytes());
        add_req(b"?format=%l@%C@%t@%w HTTP/1.1\r\n"); 
        add_req(b"Host: wttr.in\r\n");
        add_req(b"User-Agent: curl/7.81.0\r\n");
        add_req(b"Connection: close\r\n\r\n");

        if let Err(e) = socket.write_all(&http_buffer[..req_ptr]).await {
            warn!("Weather: TCP Write Error: {:?}", defmt::Debug2Format(&e));
            Timer::after(Duration::from_secs(60)).await;
            continue;
        }

        let mut pos = 0;
        loop {
            match socket.read(&mut http_buffer[pos..]).await {
                Ok(0) => break,
                Ok(n) => {
                    pos += n;
                    if pos >= http_buffer.len() { break; }
                }
                Err(e) => {
                    warn!("Weather: Socket Read Interrupted: {:?}", defmt::Debug2Format(&e));
                    break;
                }
            }
        }

        if pos > 0 {
            if let Ok(resp) = core::str::from_utf8(&http_buffer[..pos]) {
                if let Some(body_start) = resp.find("\r\n\r\n") {
                    let body = &resp[body_start + 4..].trim();
                    info!("Weather: Raw Response: '{}'", body);
                    
                    let mut parts = body.split('@');
                    let mut city_name = [0u8; 16];
                    let mut desc_en = [0u8; 16];
                    let mut cond_code = 116;
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
                         cond_code = map_condition_to_code(dr);
                    }

                    // C. Temp
                    if let Some(temp_str) = parts.next() {
                         let mut t_acc = alloc::string::String::new();
                         for c in temp_str.chars() {
                             if c.is_ascii_digit() || c == '-' { t_acc.push(c); }
                             else if c != '+' && !t_acc.is_empty() { break; }
                         }
                         t_val = t_acc.parse().unwrap_or(0);
                    }

                    // D. Wind
                    if let Some(wind_str) = parts.next() {
                         let mut w_acc = alloc::string::String::new();
                         for c in wind_str.chars() {
                             if c.is_ascii_digit() { w_acc.push(c); }
                             else if !w_acc.is_empty() { break; }
                         }
                         w_val = w_acc.parse().unwrap_or(0);
                    }

                    {
                        let state = STATUS_STATE.lock().await;
                        let mut status_ref = state.borrow_mut();
                        status_ref.city_name = city_name;
                        status_ref.weather_desc_en = desc_en;
                        status_ref.weather_code = cond_code;
                        status_ref.temperature = t_val;
                        status_ref.wind_speed = w_val;
                    }
                    SYSTEM_STATUS.signal(());
                    info!("[WEATHER] English Sync: {} | {}, Temp={}C", core::str::from_utf8(&city_name).unwrap_or(""), core::str::from_utf8(&desc_en).unwrap_or(""), t_val);
                }
            }
        }
        
        socket.close();
        Timer::after(Duration::from_secs(120)).await;
    }
}

fn map_condition_to_code(desc: &str) -> u16 {
    let lower = desc.to_ascii_lowercase();
    if lower.contains("sunny") || lower.contains("clear") { 113 }
    else if lower.contains("cloudy") { 116 }
    else if lower.contains("overcast") { 122 }
    else if lower.contains("rain") || lower.contains("shower") { 176 }
    else if lower.contains("snow") { 326 }
    else { 116 } 
}
