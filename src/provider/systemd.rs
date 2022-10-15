use crate::config::{ServiceConfig, ServiceLevel};
use crate::ServiceOperator;
use anyhow::Result;
use maplit::{btreemap, convert_args};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::fs::File;
use std::io::Write;
use std::process::Command;

#[derive(Debug)]
enum SystemdValue {
    List(Vec<String>),
    Str(String),
}

type SystemdSection = BTreeMap<String, SystemdValue>;

#[derive(Debug, Default, Serialize)]
struct SystemdServiceUnit {
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

pub struct Systemd {
    pub service: ServiceConfig,
}

impl Systemd {
    fn systemctl_command(&self) -> Command {
        let mut command = Command::new("systemctl");
        if self.service.level == ServiceLevel::User {
            command.arg("--user");
        }
        command
    }

    pub fn to_systemd_unit(&self) -> Result<String> {
        let service_unit = SystemdServiceUnit {
            unit: convert_args!(btreemap!(
                "Description" => self.service.name.clone(),
            )),
            service: convert_args!(btreemap!(
                "Type" => "simple",
                "ExecStart" => systemd_quote(self.service.command.clone()),
                "Environment" => self.service.env
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<String>>(),
            )),
            install: convert_args!(btreemap!(
                "WantedBy" => "multi-user.target",
            )),
        };

        serialize_to_string(&service_unit)
    }
}

impl ServiceOperator for Systemd {
    fn install(&self) -> Result<()> {
        let safe_unit_name = systemd_escape(vec![self.service.name.clone()], vec![])?;
        let unit_file_name = format!("{}.service", safe_unit_name);

        let unit_path = match self.service.level {
            ServiceLevel::System => PathBuf::from(r"/etc/systemd/system"),
            ServiceLevel::User => {
                let home_dir = env::var("HOME")?;
                let unit_dir = PathBuf::from(format!(r"{}/.config/systemd/user", home_dir));
                fs::create_dir_all(&unit_dir)?;
                unit_dir
            }
        }
        .join(unit_file_name);

        let content = self.to_systemd_unit()?;
        let debug_prefix = "\n>  ";
        log::info!(
            "Writing systemd unit to {:?}:{}{}",
            unit_path,
            debug_prefix,
            content.replace('\n', debug_prefix)
        );
        let mut file = File::create(&unit_path)?;
        file.write_all(content.as_bytes())?;

        let mut base_command = vec!["systemctl"];
        if self.service.level == ServiceLevel::User {
            base_command.push("--user");
        }

        log::info!("Reloading systemd daemon...");
        self.systemctl_command()
            .arg("daemon-reload")
            .spawn()?
            .wait()?;

        log::info!("Enabling service...");
        self.systemctl_command()
            .arg("enable")
            .arg(self.service.name.clone())
            .spawn()?
            .wait()?;

        Ok(())
    }

    fn start(&self) -> Result<()> {
        self.systemctl_command()
            .arg("start")
            .arg(self.service.name.clone())
            .spawn()?
            .wait()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    macro_rules! string_vec {
        ($($x:expr),*) => (vec![$($x.to_string()), *]);
    }

    #[test]
    fn test_systemd_unit_render() {
        let service = ServiceConfig {
            name: "hello".into(),
            command: string_vec!["/bin/sh", "-c", "echo hello"],
            level: ServiceLevel::System,
            env: convert_args!(btreemap!(
                "FOO" => "foo",
                "BAR" => "bar",
            )),
        };
        let systemd = Systemd { service };
        let unit_cfg = systemd.to_systemd_unit().unwrap();
        assert_eq!(
            unit_cfg,
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
