// Jackson Coxson

use log::{error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use plist_plus::Plist;

use crate::device::fetch_first_device;

pub static LAST_BEAT_SUCCESSFUL: AtomicBool = AtomicBool::new(false);

// 裝置在 Marco 訊息裡回報的 Interval 單位是秒；還沒讀到回報值之前用的預設值
const DEFAULT_INTERVAL_SECS: f64 = 15.0;
// receive 逾時取 interval 的兩倍，並鉗在 [20, 120] 秒之間：
// 太短會把裝置還沒發出的下一拍誤判成斷線，太長則真斷線時遲遲察覺不到
const MIN_RECEIVE_TIMEOUT_SECS: f64 = 20.0;
const MAX_RECEIVE_TIMEOUT_SECS: f64 = 120.0;
// 失敗重試的退避範圍（秒）：每次失敗加倍，成功一拍後重置回最小值
const MIN_BACKOFF_SECS: u64 = 5;
const MAX_BACKOFF_SECS: u64 = 60;

/// 由裝置回報的 interval（秒）換算 receive 逾時（毫秒）
fn receive_timeout_ms(interval_secs: f64) -> u32 {
    let secs = (interval_secs * 2.0).clamp(MIN_RECEIVE_TIMEOUT_SECS, MAX_RECEIVE_TIMEOUT_SECS);
    (secs * 1000.0) as u32
}

/// 失敗退避：加倍並鉗在最大值內
fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(Duration::from_secs(MAX_BACKOFF_SECS))
}

/// 從裝置的 Marco 訊息讀它回報的 Interval（秒）；讀不到或值不合理時回 None
///
/// 注意：不要用 plist_plus 的 clone/access_path 讀值——access_path 交出的節點
/// 與原樹共用底層指標，兩邊 Drop 會 double free。這裡只借用 to_string 轉出
/// XML，再用純 Rust 的 plist crate 解析，原訊息本身不動、之後照常回送。
fn interval_from_marco(marco: &Plist) -> Option<f64> {
    let xml = marco.to_string();
    let value = plist::Value::from_reader(std::io::Cursor::new(xml.as_bytes())).ok()?;
    let interval = value.as_dictionary()?.get("Interval")?;
    let value = match interval {
        plist::Value::Real(v) => Some(*v),
        plist::Value::Integer(v) => v.as_signed().map(|v| v as f64),
        _ => None,
    }?;
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}

pub fn start_beat() {
    std::thread::Builder::new()
        .name("heartbeat".to_string())
        .spawn(move || {
            // Wait for the listen thread to start
            std::thread::sleep(std::time::Duration::from_millis(100));
            info!("Starting heartbeat thread");

            let mut backoff = Duration::from_secs(MIN_BACKOFF_SECS);
            // 成功一拍後會被裝置回報的 interval 更新；在此之前先用預設值
            let mut timeout_ms = receive_timeout_ms(DEFAULT_INTERVAL_SECS);

            loop {
                let device = match fetch_first_device() {
                    Ok(d) => d,
                    _ => {
                        LAST_BEAT_SUCCESSFUL.store(false, Ordering::Relaxed);
                        warn!("Could not get device from muxer for heartbeat");
                        std::thread::sleep(backoff);
                        backoff = next_backoff(backoff);
                        continue;
                    }
                };

                let hb = match device.new_heartbeat_client("minimuxer") {
                    Ok(h) => h,
                    Err(e) => {
                        LAST_BEAT_SUCCESSFUL.store(false, Ordering::Relaxed);
                        error!("Failed to create heartbeat client: {:?}", e);
                        std::thread::sleep(backoff);
                        backoff = next_backoff(backoff);
                        continue;
                    }
                };

                loop {
                    let plist = match hb.receive(timeout_ms) {
                        Ok(p) => p,
                        Err(e) => {
                            LAST_BEAT_SUCCESSFUL.store(false, Ordering::Relaxed);
                            error!("Heartbeat recv failed: {:?}", e);
                            break;
                        }
                    };

                    // 之後的逾時跟著裝置回報的節奏走，不再寫死
                    if let Some(interval) = interval_from_marco(&plist) {
                        timeout_ms = receive_timeout_ms(interval);
                    }

                    match hb.send(plist) {
                        Ok(_) => {}
                        Err(e) => {
                            LAST_BEAT_SUCCESSFUL.store(false, Ordering::Relaxed);
                            error!("Heartbeat send failed: {:?}", e);
                            break;
                        }
                    }

                    LAST_BEAT_SUCCESSFUL.store(true, Ordering::Relaxed);
                    backoff = Duration::from_secs(MIN_BACKOFF_SECS);
                    info!("Heartbeat success!");
                }

                // 連線斷掉後按退避節奏重連，裝置恢復後自動續跳
                std::thread::sleep(backoff);
                backoff = next_backoff(backoff);
            }
        })
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_timeout_is_twice_interval_and_clamped() {
        assert_eq!(receive_timeout_ms(15.0), 30_000);
        assert_eq!(receive_timeout_ms(1.0), 20_000); // 鉗下限
        assert_eq!(receive_timeout_ms(600.0), 120_000); // 鉗上限
    }

    #[test]
    fn backoff_doubles_and_caps_at_max() {
        let mut backoff = Duration::from_secs(MIN_BACKOFF_SECS);
        for expected in [10, 20, 40, 60, 60] {
            backoff = next_backoff(backoff);
            assert_eq!(backoff, Duration::from_secs(expected));
        }
    }

    #[test]
    fn interval_is_read_from_marco() {
        let xml = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<plist version=\"1.0\"><dict>",
            "<key>Command</key><string>Marco</string>",
            "<key>Interval</key><real>15.0</real>",
            "</dict></plist>"
        );
        let marco = Plist::from_xml(xml.to_string()).expect("test plist should parse");
        assert_eq!(interval_from_marco(&marco), Some(15.0));
    }
}
