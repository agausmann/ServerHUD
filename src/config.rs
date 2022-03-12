use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub lcd: Lcd,
}

#[derive(Deserialize)]
pub struct Lcd {
    pub path: String,
}
