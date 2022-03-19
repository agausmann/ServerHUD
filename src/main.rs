pub mod config;

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use anyhow::Context;
use cfa635::{Key, Report, NUM_COLUMNS, NUM_ROWS};
use config::Config;
use sysinfo::{Disk, DiskExt, NetworkData, NetworkExt, NetworksExt, System, SystemExt};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const SCREEN_TIMEOUT: Duration = Duration::from_secs(15);

struct App {
    config: Config,
    system: System,
    next_poll: Instant,
    next_refresh: Instant,
    screen_timeout: Option<Instant>,
    lcd: cfa635::Device,
    should_redraw: bool,
    current_page: Page,
    scroll: usize,
    max_scroll: Option<usize>,
    buffer: [[u8; NUM_COLUMNS as usize]; NUM_ROWS as usize],
}

impl App {
    fn new(config: Config) -> anyhow::Result<Self> {
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
        let system = System::new();
        let now = Instant::now();
        Ok(Self {
            config,
            system,
            next_poll: now,
            next_refresh: now,
            screen_timeout: Some(now),
            lcd,
            should_redraw: false,
            current_page: Page::System,
            scroll: 0,
            max_scroll: None,
            buffer: [[b' '; NUM_COLUMNS as usize]; NUM_ROWS as usize],
        })
    }

    fn run(mut self) -> anyhow::Result<()> {
        loop {
            let now = Instant::now();
            if now >= self.next_poll {
                self.poll()?;
                while now >= self.next_poll {
                    self.next_poll += POLL_INTERVAL;
                }
            }

            if let Some(screen_timeout) = self.screen_timeout {
                if now >= screen_timeout {
                    self.sleep()?;
                }
            }

            // Make a new "now" that is after the poll,
            // so if queue_refresh was called, we immediately refresh:
            let now = Instant::now();
            if now >= self.next_refresh {
                self.refresh();
                while now >= self.next_refresh {
                    self.next_refresh += REFRESH_INTERVAL;
                }
            }

            if self.should_redraw {
                self.redraw()?;
                self.should_redraw = false;
            }

            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn poll(&mut self) -> anyhow::Result<()> {
        while let Some(report) = self.lcd.poll_report()? {
            match report {
                Report::KeyActivity { key, pressed } if pressed => {
                    if !self.wake() {
                        match key {
                            Key::Left => {
                                self.current_page = self.current_page.prev();
                                self.scroll = 0;
                                self.max_scroll = None;
                                self.queue_redraw();
                            }
                            Key::Right => {
                                self.current_page = self.current_page.next();
                                self.scroll = 0;
                                self.max_scroll = None;
                                self.queue_redraw();
                            }
                            Key::Up => {
                                if self.scroll > 0 {
                                    self.scroll -= 1;
                                    self.queue_redraw();
                                }
                            }
                            Key::Down => {
                                if let Some(max_scroll) = self.max_scroll {
                                    if self.scroll < max_scroll {
                                        self.scroll += 1;
                                        self.queue_redraw();
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn sleep(&mut self) -> anyhow::Result<()> {
        self.screen_timeout = None;
        self.lcd.set_backlight(0, 0)?;
        Ok(())
    }

    fn wake(&mut self) -> bool {
        let was_asleep = self.screen_timeout.is_none();
        self.screen_timeout = Some(Instant::now() + SCREEN_TIMEOUT);
        if was_asleep {
            // Instead of turning on backlight here,
            // it is deferred until the end of redraw after the LCD is updated.
            // Otherwise, the old contents might be briefly displayed.
            self.queue_redraw();
        }
        was_asleep
    }

    fn is_awake(&self) -> bool {
        self.screen_timeout.is_some()
    }

    fn queue_redraw(&mut self) {
        self.should_redraw = true;
    }

    fn refresh(&mut self) {
        self.system.refresh_cpu();
        self.system.refresh_memory();
        self.system.refresh_disks_list();
        self.system.refresh_networks_list();
        if self.is_awake() {
            self.queue_redraw();
        }
    }

    fn redraw(&mut self) -> anyhow::Result<()> {
        self.clear();
        if let Some(name) = self.system.host_name() {
            self.set_text(0, 0, name.as_bytes());
        }

        match self.current_page {
            Page::System => {
                let load_avg = self.system.load_average();
                let load_avg_str = format!(
                    "CPU: {:.2} {:.2} {:.2}",
                    load_avg.one, load_avg.five, load_avg.fifteen,
                );
                self.set_text(1, 0, load_avg_str.as_bytes());

                let total = kb_to_mib(self.system.total_memory());
                let unavailable = total - kb_to_mib(self.system.available_memory());
                let memory_str = format!("Mem: {}/{} M", unavailable, total);
                self.set_text(2, 0, memory_str.as_bytes());
            }
            Page::Disk => {
                let disks = self.system.disks();
                let sorted_disks = disks
                    .iter()
                    .map(|disk| (disk.mount_point().to_string_lossy().into_owned(), disk))
                    .collect::<BTreeMap<_, _>>();

                let display_disks: Vec<&Disk> = if self.config.disk.paths.is_empty() {
                    sorted_disks.into_values().collect()
                } else {
                    self.config
                        .disk
                        .paths
                        .iter()
                        .flat_map(|key| sorted_disks.get(key).copied())
                        .collect()
                };

                let max_scroll = display_disks.len().saturating_sub(NUM_ROWS as usize - 1);
                self.max_scroll = Some(max_scroll);
                self.scroll = self.scroll.min(max_scroll);

                // Creating an intermediate collection; otherwise, we would
                // be calling `set_text` while `system` is still borrowed.
                //
                // Long term fix: create a struct wrapping the buffer that
                // provides the set_text method, so set_text can be a disjoint
                // borrow instead of borrowing all of `self`.
                let lines: Vec<String> = display_disks
                    .into_iter()
                    .skip(self.scroll)
                    .take(NUM_ROWS as usize - 1)
                    .map(|disk| {
                        let total = disk.total_space() >> 30;
                        let unavailable = total.saturating_sub(disk.available_space() >> 30);
                        format!(
                            "{} {}/{} G",
                            disk.mount_point().to_string_lossy(),
                            unavailable,
                            total
                        )
                    })
                    .collect();

                for (i, line) in lines.into_iter().enumerate() {
                    self.set_text(i + 1, 0, line.as_bytes());
                }
            }
            Page::Network => {
                let sorted_networks: BTreeMap<&String, &NetworkData> =
                    self.system.networks().iter().collect();

                let display_networks: Vec<(&String, &NetworkData)> =
                    if self.config.network.interfaces.is_empty() {
                        sorted_networks.into_iter().collect()
                    } else {
                        self.config
                            .network
                            .interfaces
                            .iter()
                            .flat_map(|key| sorted_networks.get(key).copied().map(|net| (key, net)))
                            .collect()
                    };

                let max_scroll = display_networks.len().saturating_sub(NUM_ROWS as usize - 1);
                self.max_scroll = Some(max_scroll);
                self.scroll = self.scroll.min(max_scroll);

                let lines: Vec<String> = display_networks
                    .into_iter()
                    .skip(self.scroll)
                    .take(NUM_ROWS as usize - 1)
                    .map(|(name, net)| {
                        let up = net.transmitted() as f32 / REFRESH_INTERVAL.as_secs_f32() * 8.0e-6;
                        let down = net.received() as f32 / REFRESH_INTERVAL.as_secs_f32() * 8.0e-6;
                        format!("{} {:.1}/{:.1} M", name, up, down)
                    })
                    .collect();

                for (i, line) in lines.into_iter().enumerate() {
                    self.set_text(i + 1, 0, line.as_bytes());
                }
            }
        }
        self.flush()?;
        // Deferred backlight control from wake():
        self.lcd.set_backlight(100, 100)?;
        Ok(())
    }

    fn clear(&mut self) {
        for row in &mut self.buffer {
            row.fill(b' ');
        }
    }

    fn set_text(&mut self, row: usize, col: usize, text: &[u8]) {
        let line = &mut self.buffer[row][col..];
        let clamped_len = text.len().min(line.len());
        line[..clamped_len].copy_from_slice(&text[..clamped_len]);
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        for (row_index, row_text) in self.buffer.iter().enumerate() {
            self.lcd.set_text(row_index as u8, 0, row_text)?;
        }
        Ok(())
    }
}

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

    let app = App::new(config)?;
    app.run()
}

fn kb_to_mib(x: u64) -> u64 {
    x * 1024 / 1000 / 1024
}
