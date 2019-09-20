# data-collector
[![Docker Cloud Build Status](https://img.shields.io/docker/cloud/build/timokroeger/data-collector.svg)](https://hub.docker.com/r/timokroeger/data-collector/builds)

Configurable Modbus client which sends the collected data to InfluxDB.

## Usage

    USAGE:
        data-collector.exe [OPTIONS]

    FLAGS:
        -h, --help       Prints help information
        -V, --version    Prints version information

    OPTIONS:
        -c, --config <FILE>       Sets a custom config file [default: config.toml]
            --logfile <FILE>      Sets a custom log file
            --loglevel <LEVEL>    Sets the logging level [default: warn]
                                  [possible values: off, error, warn, info, debug, trace]

## Configuration

By default configuration is loaded from `config.toml` in the current directory.
The configuration file can be overwritten with the `--config <FILE>` flag.
An example configuration file is provided in this repository.

### The `[modbus]` section

#### The `hostname` field
Hostname of the ModbusTCP server. Must be a string that can be converted to rusts
[`std::net::ToSocketAddrs`](https://doc.rust-lang.org/std/net/trait.ToSocketAddrs.html).

#### The `port` field
Port number the ModbusTCP server listens on, usually 502.

#### The `timeout` field
Time to wait for a response of a modbus device.
Parses times in free form like: "1s 500ms".

When no configured sensor responds within the timeout delay a reconnection to the modbus server is issued.

### The `[influxdb]` section

#### The `hostname` field
URL of the InfluxDB http api endpoint.

#### The `db` field
Database name to store the data in. The database must exists for datapoints to be stored.
To create a database manually you can use the `influx` tool with tha `create database <DB>' command.

### The `[influxdb2]` section
Ignored if the `[influxdb]` is available.

#### The `hostname` field
URL of the InfluxDB http api endpoint.

#### The `organization` field
The organization in which to write data. Use your organization name or ID.

#### The `bucket` fields
The bucket in which to write data. Use the bucket name or ID. The bucket must belong to the specified organization.

#### The `username` and `password` fields
Optional fields to configure credentials when authentication is enabled for InfluxDB.

### The `[[devices]]` array
Contains one entry for each modbus device on the bus.

#### The `template` field
Optional. Name of the device template that should be used. All settings from the template are copied to this device.

#### The `id` field
Modbus Slave ID/Unit ID of the sensor.

#### The `scan_interval` field
Polling interval for all defined `input_registers`.
Parses times in free form like: "1min 30s".

#### The `tags` table
Optional. Key value pairs that are stored in the database alongside each measurement from this device.

#### The `[[input_registers]]` array

##### The `addr` field
Address of the register (starting at 0). Not to be confused with the Modbus data model number (which starts at 1).

##### The `id` field
Name for the register. Used as `measurement` in InfluxDB.

##### The `data_type` field
Optional, default: "u16".
Data type of the register. Possible values: "u16", "u32", "u64", "i16", "i32", "i64", "f32", "f64"

##### The `tags` table
Optional. Key value pairs that are stored in the database alongside this measurement

### The `[templates.<template_name>]` section
See the descripition of the `[[devices]]` array.
