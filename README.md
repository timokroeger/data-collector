# data-collector
![Docker Cloud Build Status](https://img.shields.io/docker/cloud/build/timokroeger/data-collector.svg)
![GitHub](https://img.shields.io/github/license/timokroeger/data-collector.svg)

Configurable Modbus client which sends the collected data to InfluxDB.

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

### The `[influxdb]` section

#### The `hostname` field
URL of the InfluxDB http api endpoint.

#### The `db` field
Database name to store the data in. The database must exists for datapoints to be stored.
To create a database manually you can use the `influx` tool with tha `create database <DB>' command.

#### The `username` and `password` fields
Optional fields to configure credentials when authentication is enabled for InfluxDB.

### Sensor [\<GROUP\>] sections
Every other top level section in the configuration file specifies a sensor group.
A sensor group shares the same set of registers. Usually a group for each sensor type is used.

#### The `scan_interval` field
Polling interval for `measurement_registers`.
Parses times in free form like: "1min 30s".

#### The [\<GROUP\>.measurement_registers] section
Table of name to register address mappings.
The name is used as the measurement key when storing data points in InfluxDB.
This mapping is shared among all sensors in the group.

#### The [[\<GROUP\>.sensors]] array
Array of sensors in this group.

##### The `id` field
Modbus Slave ID/Unit ID of the sensor.

##### Other fields
Optional tags that are stored alogside each datapoint of this sensor.
