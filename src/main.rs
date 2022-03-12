pub mod config;

use anyhow::Context;
use config::Config;

fn main() -> anyhow::Result<()> {
    let config_raw =
        std::fs::read("ServerHUD.toml").context("cannot read config file ServerHUD.toml")?;
    let config: Config = toml::from_slice(&config_raw).context("cannot parse config file")?;

    let mut lcd =
        cfa635::Device::new(&config.lcd.path).context("failed to open LCD serial port")?;

    lcd.clear_screen()?;
    lcd.set_backlight(100, 100)?;
    lcd.set_text(0, 0, b"Hello World")?;

    Ok(())
}
