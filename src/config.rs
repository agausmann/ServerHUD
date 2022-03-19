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
}

#[derive(Deserialize)]
pub struct Disk {
    pub paths: Vec<String>,
}

#[derive(Deserialize)]
pub struct Network {
    pub interfaces: Vec<String>,
}
