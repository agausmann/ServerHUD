pub mod config;

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use anyhow::Context;
use cfa635::{Key, Report, NUM_COLUMNS, NUM_ROWS};
use config::Config;
use sysinfo::{Disk, DiskExt, System, SystemExt};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const SCREEN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy)]
enum Page {
    System,
    Disk,
    Network,
}

impl Page {
    fn next(&self) -> Self {
        match self {
            Self::System => Self::Disk,
            Self::Disk => Self::Network,
            Self::Network => Self::System,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Self::Disk => Self::System,
            Self::Network => Self::Disk,
            Self::System => Self::Network,
        }
    }
}

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
    let mut current_page = Page::System;
    let mut scroll: usize = 0;
    let mut max_scroll: Option<usize> = None;

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
                                    scroll = 0;
                                    max_scroll = None;
                                    next_refresh = now;
                                    lcd.clear_screen()?;
                                }
                                Key::Right => {
                                    current_page = current_page.next();
                                    scroll = 0;
                                    max_scroll = None;
                                    next_refresh = now;
                                    lcd.clear_screen()?;
                                }
                                Key::Up => {
                                    if scroll > 0 {
                                        scroll -= 1;
                                        next_refresh = now;
                                    }
                                }
                                Key::Down => {
                                    if let Some(max_scroll) = max_scroll {
                                        scroll = (scroll + 1).min(max_scroll);
                                        next_refresh = now;
                                    }
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
                if let Some(name) = system.host_name() {
                    lcd.set_text(0, 0, &BLANK_LINE)?;
                    lcd.set_text(0, 0, name.as_bytes())?;
                }

                match current_page {
                    Page::System => {
                        system.refresh_cpu();
                        system.refresh_memory();

                        let load_avg = system.load_average();
                        let load_avg_str = format!(
                            "CPU: {:.2} {:.2} {:.2}",
                            load_avg.one, load_avg.five, load_avg.fifteen,
                        );
                        lcd.set_text(1, 0, &BLANK_LINE)?;
                        lcd.set_text(1, 0, load_avg_str.as_bytes())?;

                        let total = kb_to_mib(system.total_memory());
                        let unavailable = total - kb_to_mib(system.available_memory());
                        let memory_str = format!("Mem: {}/{} M", unavailable, total);
                        lcd.set_text(2, 0, &BLANK_LINE)?;
                        lcd.set_text(2, 0, memory_str.as_bytes())?;
                    }
                    Page::Disk => {
                        system.refresh_disks_list();
                        max_scroll = Some(system.disks().len() - (NUM_ROWS as usize - 1));
                        scroll = scroll.min(max_scroll.unwrap());

                        let disks = system.disks();
                        let sorted_disks = disks
                            .iter()
                            .map(|disk| (disk.mount_point().to_string_lossy().into_owned(), disk))
                            .collect::<BTreeMap<_, _>>();
                        println!("{:?}", sorted_disks);

                        let display_disks: Vec<&Disk> = if config.disk.paths.is_empty() {
                            sorted_disks.values().copied().collect()
                        } else {
                            config
                                .disk
                                .paths
                                .iter()
                                .flat_map(|key| sorted_disks.get(key).copied())
                                .collect()
                        };

                        for (i, disk) in display_disks.into_iter().skip(scroll).take(3).enumerate()
                        {
                            let total = disk.total_space() >> 30;
                            let unavailable = total.saturating_sub(disk.available_space() >> 30);
                            let disk_str = format!(
                                "{} {}/{} G",
                                disk.mount_point().to_string_lossy(),
                                unavailable,
                                total
                            );
                            lcd.set_text(i as u8 + 1, 0, &BLANK_LINE)?;
                            lcd.set_text(i as u8 + 1, 0, disk_str.as_bytes())?;
                        }
                    }
                    Page::Network => {
                        system.refresh_networks_list();
                        // max_scroll = Some(system.networks().len());
                        // scroll = scroll.min(max_scroll.unwrap());
                    }
                }
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

const BLANK_LINE: [u8; NUM_COLUMNS as usize] = [b' '; NUM_COLUMNS as usize];

fn kb_to_mib(x: u64) -> u64 {
    x * 1024 / 1000 / 1024
}
