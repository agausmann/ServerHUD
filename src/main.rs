pub mod config;

use std::time::{Duration, Instant};

use anyhow::Context;
use cfa635::{Key, Report};
use config::Config;
use sysinfo::{System, SystemExt};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const SCREEN_TIMEOUT: Duration = Duration::from_secs(15);

fn main() -> anyhow::Result<()> {
    let config_raw =
        std::fs::read("ServerHUD.toml").context("cannot read config file ServerHUD.toml")?;
    let config: Config = toml::from_slice(&config_raw).context("cannot parse config file")?;

    let mut lcd =
        cfa635::Device::new(&config.lcd.path).context("failed to open LCD serial port")?;
    lcd.configure_key_reporting(
        &[
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Enter,
            Key::Exit,
        ],
        &[],
    )?;

    let mut system = System::new();

    let mut next_poll = Instant::now();
    let mut next_refresh = Instant::now();
    let mut screen_timeout = Some(Instant::now());

    loop {
        let now = Instant::now();

        if now > next_poll {
            while now > next_poll {
                next_poll += POLL_INTERVAL;
            }

            while let Some(report) = lcd.poll_report()? {
                match report {
                    Report::KeyActivity { key, pressed } if pressed => {
                        if screen_timeout.is_none() {
                            lcd.set_backlight(100, 100)?;
                            // Force refresh
                            next_refresh = now;
                        } else {
                            match key {
                                Key::Left => {
                                    current_page = current_page.prev();
                                    next_refresh = now;
                                    lcd.clear_screen()?;
                                }
                                Key::Right => {
                                    current_page = current_page.next();
                                    next_refresh = now;
                                    lcd.clear_screen()?;
                                }
                                _ => {}
                            }
                        }
                        screen_timeout = Some(now + SCREEN_TIMEOUT);
                    }
                    _ => {}
                }
            }
        }

        if now >= next_refresh {
            while now > next_refresh {
                next_refresh += REFRESH_INTERVAL;
            }
            if screen_timeout.is_some() {
                system.refresh_cpu();

                lcd.clear_screen()?;
                if let Some(name) = system.host_name() {
                    lcd.set_text(0, 0, name.as_bytes())?;
                }
                let load_avg = system.load_average();
                let load_avg_str = format!(
                    "{:.2} {:.2} {:.2}",
                    load_avg.one, load_avg.five, load_avg.fifteen,
                );
                lcd.set_text(1, 0, load_avg_str.as_bytes())?;
            }
        }

        if let Some(timeout) = screen_timeout {
            if now > timeout {
                lcd.set_backlight(0, 0)?;
                screen_timeout = None;
            }
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
