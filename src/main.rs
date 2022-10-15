use crate::config::{ServiceConfig, ServiceLevel};
use anyhow::Result;
use clap::Parser;
use regex::Regex;
use std::env;
use std::process::exit;

mod config;
mod provider;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap(value_parser = validate_name)]
    name: String,
    command: Vec<String>,
    #[clap(short, long)]
    env: Vec<String>,
    #[clap(long, value_enum, default_value = "system")]
    level: ServiceLevel,
    #[clap(long)]
    start: bool,
}

pub trait ServiceOperator {
    fn install(&self) -> Result<()>;
    fn start(&self) -> Result<()>;
}

fn str_partition(string: &str, delimiter: &str) -> (String, String) {
    let mut splitter = string.splitn(2, delimiter);
    let first = splitter.next().unwrap_or("").into();
    let second = splitter.next().unwrap_or("").into();
    (first, second)
}

fn validate_name(v: &str) -> Result<String, String> {
    let re_valid_name = Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$").expect("Bad regex");
    if !re_valid_name.is_match(v) {
        return Err(format!(
            "Name includes invalid characters. Pattern: {:?}",
            re_valid_name
        ));
    }
    if v.len() > 256 {
        return Err("Name must not exceed 256 characters.".into());
    }
    Ok(v.to_string())
}

fn main() {
    if env::var_os("RUST_LOG").is_none() {
        env::set_var("RUST_LOG", "mkservice=info");
    }
    env_logger::init();

    let args = Args::parse();

    let service = ServiceConfig {
        name: args.name,
        command: args.command,
        level: args.level,
        env: args
            .env
            .into_iter()
            .map(|v| str_partition(&v, "="))
            .collect(),
    };

    log::debug!("Service: {:#?}", service);

    match provider::get_provider(service.clone()) {
        Some(p) => {
            if let Err(e) = p.install() {
                log::error!("Failed creating service: {:?}", e);
                exit(1);
            }
            if args.start {
                if let Err(e) = p.start() {
                    log::error!("Error starting service: {:?}", e);
                    exit(1);
                }
            }
        }
        None => {
            log::error!("Unknown service runtime, cannot add service.");
            exit(1);
        }
    }
    log::info!("Service {:?} installed.", service.name);
}
