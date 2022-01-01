# mkservice

A CLI utility to stamp out a basic systemd service.

## Usage

```
USAGE:
    mkservice [OPTIONS] <NAME> [COMMAND]...

ARGS:
    <NAME>          
    <COMMAND>...    

OPTIONS:
    -e, --env <ENV>        
    -h, --help             Print help information
        --level <LEVEL>    [default: system] [possible values: user, system]
    -V, --version          Print version information
```

An example:

```
mkservice --env LOG_LEVEL=debug --env SECRET=abc myprogram /usr/local/bin/myprogram -- pos_arg --flag-arg
```

Would write out `/etc/systemd/system/myprogram.service`, reload systemd, and enable the service for start on next boot:

```ini
[Unit]
Description=myprogram
[Install]
WantedBy=multi-user.target
[Service]
Environment=LOG_LEVEL=debug
Environment=SECRET=abc
ExecStart="/usr/local/bin/myprogram" "pos_arg" "--flag-arg"
Type=simple
```
