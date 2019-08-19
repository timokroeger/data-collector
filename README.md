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
        -c, --config <FILE>       Sets a custom config file [default: datacollector.toml]
            --logfile <FILE>      Sets a custom log file
            --loglevel <LEVEL>    Sets the logging level [default: warn]
                                  [possible values: off, error, warn, info, debug, trace]

## Configuration

By default configuration is loaded from `datacollector.toml` in the current directory.
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

### The `[modbus_rtu]` section
Ignored if the `[modbus]` is available.
Uses a hardcoded baudrate of 19200bps and even parity with one stop bit.
Pull requests to make this configurable are welcome.

#### The `port` field
The address of the serial device. On windows this is usually named `COMx`.

### The `[influxdb]` section

#### The `hostname` field
URL of the InfluxDB http api endpoint.

#### The `db` field
Database name to store the data in. The database must exists for datapoints to be stored.
To create a database manually you can use the `influx` tool with tha `create database <DB>' command.

#### The `username` and `password` fields
Optional fields to configure credentials when authentication is enabled for InfluxDB.

### The `[influxdb2]` section
Ignored if the `[influxdb]` is available.

#### The `hostname` field
URL of the InfluxDB http api endpoint.

#### The `organization` field
The organization in which to write data. Use your organization name or ID.

#### The `bucket` fields
The bucket in which to write data. Use the bucket name or ID. The bucket must belong to the specified organization.

### Sensor `[\<GROUP\>]` sections
Every other top level section in the configuration file specifies a sensor group.
A sensor group shares the same set of registers. Usually a group for each sensor type is used.

#### The `scan_interval` field
Polling interval for all defined `measurement_registers`.
Parses times in free form like: "1min 30s".

#### The `[\<GROUP\>.measurement_registers]` section
Name to register address mappings.
The name is used as the measurement key when storing data points in InfluxDB.
This mapping is shared among all sensors in this group.

#### The `[[\<GROUP\>.sensors]]` array
Array of sensors in this group.

##### The `id` field
Modbus Slave ID/Unit ID of the sensor.

##### Other fields
Optional string tags which are stored alogside each datapoint of this sensor.
