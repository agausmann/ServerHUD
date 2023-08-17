use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub lcd: Lcd,
    pub disk: Disk,
    pub network: Network,
}

#[derive(Deserialize)]
pub struct Lcd {
    pub path: String,
    pub screen_backlight: u8,
    pub keypad_backlight: u8,
}

#[derive(Deserialize)]
pub struct Disk {
    pub paths: Vec<String>,
    pub md_raid: Vec<String>,
}

#[derive(Deserialize)]
pub struct Network {
    pub interfaces: Vec<String>,
}
