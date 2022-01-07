use anyhow::Result;
use clap::Parser;
use maplit::{btreemap, convert_args};
use regex::Regex;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{exit, Command};

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap(parse(try_from_str = validate_name))]
    name: String,
    command: Vec<String>,
    #[clap(short, long)]
    env: Vec<String>,
    #[clap(long, arg_enum, default_value = "system")]
    level: ServiceLevel,
    #[clap(long)]
    start: bool,
}

#[derive(clap::ArgEnum, Clone, Debug, PartialEq)]
enum ServiceLevel {
    User,
    System,
}

#[derive(Debug)]
struct Service {
    name: String,
    command: Vec<String>,
    env: BTreeMap<String, String>,
    level: ServiceLevel,
}

#[derive(Debug)]
enum SystemdValue {
    List(Vec<String>),
    Str(String),
}

type SystemdSection = BTreeMap<String, SystemdValue>;

#[derive(Debug, Default, Serialize)]
struct SystemdService {
    #[serde(serialize_with = "serialize_systemd_section", rename = "Unit")]
    unit: SystemdSection,
    #[serde(serialize_with = "serialize_systemd_section", rename = "Install")]
    install: SystemdSection,
    #[serde(serialize_with = "serialize_systemd_section", rename = "Service")]
    service: SystemdSection,
}

fn serialize_systemd_section<S>(section: &SystemdSection, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = s.serialize_map(Some(section.len()))?;
    for (key, option) in section {
        match option {
            SystemdValue::Str(value) => {
                map.serialize_entry(key, value)?;
            }
            SystemdValue::List(values) => {
                for value in values {
                    map.serialize_entry(key, value)?;
                }
            }
        }
    }
    map.end()
}

impl From<String> for SystemdValue {
    fn from(item: String) -> Self {
        SystemdValue::Str(item)
    }
}

impl From<Vec<String>> for SystemdValue {
    fn from(item: Vec<String>) -> Self {
        SystemdValue::List(item)
    }
}

impl From<&str> for SystemdValue {
    fn from(item: &str) -> Self {
        SystemdValue::Str(item.to_string())
    }
}

/// serde_ini serializes with CRLF by default, this just enforces LF.
fn serialize_to_string<T: Serialize>(t: &T) -> Result<String> {
    let mut buf: Vec<u8> = Vec::with_capacity(128);
    let writer = serde_ini::write::Writer::new(&mut buf, serde_ini::write::LineEnding::Linefeed);
    let mut ser = serde_ini::ser::Serializer::new(writer);
    t.serialize(&mut ser).map(|_| &buf)?;
    Ok(unsafe { String::from_utf8_unchecked(buf) })
}

fn systemd_escape(strings: Vec<String>, args: Vec<&str>) -> Result<String> {
    let mut output = Command::new("systemd-escape")
        .args(args)
        .arg("--")
        .args(strings)
        .output()?;
    // Trim the trailing newline.
    output.stdout.truncate(output.stdout.len() - 1);
    let escaped = String::from_utf8(output.stdout)?;
    Ok(escaped)
}

fn systemd_quote(strings: Vec<String>) -> String {
    strings
        .into_iter()
        // Naive...
        .map(|s| format!("\"{}\"", s.replace('"', "\\\"")))
        .collect::<Vec<String>>()
        .join(" ")
}

impl Service {
    fn to_systemd_unit(&self) -> Result<String> {
        let mut service_unit = SystemdService::default();
        service_unit.unit = convert_args!(btreemap!(
            "Description" => self.name.clone(),
        ));
        service_unit.service = convert_args!(btreemap!(
            "Type" => "simple",
            "ExecStart" => systemd_quote(self.command.clone()),
            "Environment" => self.env
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<String>>(),
        ));
        service_unit.install = convert_args!(btreemap!(
            "WantedBy" => "multi-user.target",
        ));

        serialize_to_string(&service_unit)
    }
}

fn create_systemd_service(service: &Service, start: bool) -> Result<()> {
    let safe_unit_name = systemd_escape(vec![service.name.clone()], vec![])?;
    let unit_file_name = format!("{}.service", safe_unit_name);
    use std::path::PathBuf;
    let unit_path = match service.level {
        ServiceLevel::System => PathBuf::from(r"/etc/systemd/system"),
        ServiceLevel::User => {
            let home_dir = env::var("HOME")?;
            let unit_dir = PathBuf::from(format!(r"{}/.config/systemd/user", home_dir));
            fs::create_dir_all(&unit_dir)?;
            unit_dir
        }
    }
    .join(unit_file_name);

    let content = service.to_systemd_unit()?;
    let debug_prefix = "\n>  ";
    log::info!(
        "Writing systemd unit to {:?}:{}{}",
        unit_path,
        debug_prefix,
        content.replace("\n", debug_prefix)
    );
    let mut file = File::create(&unit_path)?;
    file.write_all(content.as_bytes())?;

    let mut base_command = vec!["systemctl"];
    if service.level == ServiceLevel::User {
        base_command.push("--user");
    }

    log::info!("Reloading systemd daemon...");
    Command::new(base_command[0])
        .args(&base_command[1..])
        .arg("daemon-reload")
        .spawn()?
        .wait()?;

    log::info!("Enabling service...");
    Command::new(&base_command[0])
        .args(&base_command[1..])
        .arg("enable")
        .arg(service.name.clone())
        .spawn()?
        .wait()?;

    if start {
        log::info!("Starting service...");
        Command::new(&base_command[0])
            .args(&base_command[1..])
            .arg("start")
            .arg(service.name.clone())
            .spawn()?
            .wait()?;
    }

    Ok(())
}

fn str_partition(string: &str, delimiter: &str) -> (String, String) {
    let mut splitter = string.splitn(2, delimiter);
    let first = splitter.next().or(Some("")).unwrap().into();
    let second = splitter.next().or(Some("")).unwrap().into();
    (first, second)
}

fn validate_name(v: &str) -> Result<String, String> {
    let re_valid_name = Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]*$").expect("Bad regex");
    if !re_valid_name.is_match(v) {
        return Err(format!(
            "Name includes invalid characters. Pattern: {:?}",
            re_valid_name
        )
        .into());
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

    let service = Service {
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

    // systemd
    if Path::new("/run/systemd/system").exists() {
        log::info!("systemd detected, creating service...");
        if let Err(e) = create_systemd_service(&service, args.start) {
            log::error!("Failed creating service: {:?}", e);
            exit(1);
        }
    } else {
        log::error!("Unknown init system, cannot add service.");
        exit(1);
    }
    log::info!("Service {:?} installed.", service.name);
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    macro_rules! string_vec {
        ($($x:expr),*) => (vec![$($x.to_string()), *]);
    }

    #[test]
    fn test_render_service() {
        let service = Service {
            name: "hello".into(),
            command: string_vec!["/bin/sh", "-c", "echo hello"],
            level: ServiceLevel::System,
            env: convert_args!(btreemap!(
                "FOO" => "foo",
                "BAR" => "bar",
            )),
        };
        let cfg = service.to_systemd_unit().unwrap();
        assert_eq!(
            cfg,
            "[Unit]\n\
            Description=hello\n\
            [Install]\n\
            WantedBy=multi-user.target\n\
            [Service]\n\
            Environment=BAR=bar\n\
            Environment=FOO=foo\n\
            ExecStart=\"/bin/sh\" \"-c\" \"echo hello\"\n\
            Type=simple\n\
            ",
        )
    }
}
