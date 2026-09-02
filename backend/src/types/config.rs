use std::net::Ipv4Addr;

pub struct AppConfig {
    host: Ipv4Addr,
    port: u16,
}

impl AppConfig {
    fn new() {
        match dotenvy::dotenv() {
            Ok(p) => println!("{}", p.to_str().unwrap()),
            Err(_) => {
                println!(".env file was not found in this directory or at parents");
                std::process::exit(1);
            }
        }
    }
}
