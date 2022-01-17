use crate::config::ServiceConfig;
use crate::ServiceOperator;
use std::path::Path;
use systemd::Systemd;

pub mod systemd;

pub fn get_provider(service: ServiceConfig) -> Option<impl ServiceOperator> {
    if Path::new("/run/systemd/system").exists() {
        Some(Systemd { service })
    } else {
        None
    }
}
