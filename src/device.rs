use std::collections::BTreeMap;
use std::iter;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use modbus::{Client, Error};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DataType {
    U16,
    U32,
    I16,
    I32,
    F32,
    F64,
}

impl FromStr for DataType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "u16" => Ok(Self::U16),
            "u32" => Ok(Self::U32),
            "i16" => Ok(Self::I16),
            "i32" => Ok(Self::I32),
            "f32" => Ok(Self::F32),
            "f64" => Ok(Self::F64),
            _ => Err(()),
        }
    }
}

impl DataType {
    fn num_registers(self) -> u16 {
        match self {
            Self::U16 | Self::I16 => 1,
            Self::U32 | Self::I32 | Self::F32 => 2,
            Self::F64 => 4,
        }
    }

    pub fn parse_data(self, data: &[u16]) -> f64 {
        match self {
            Self::U16 => f64::from(data[0]),
            Self::U32 => f64::from((data[0] as u32) << 16 | data[1] as u32),
            Self::I16 => f64::from(data[0] as i16),
            Self::I32 => f64::from((data[0] as i32) << 16 | data[1] as i32),
            Self::F32 => f64::from(f32::from_bits((data[0] as u32) << 16 | data[1] as u32)),
            Self::F64 => f64::from_bits(
                (data[0] as u64) << 48
                    | (data[1] as u64) << 32
                    | (data[2] as u64) << 16
                    | data[3] as u64,
            ),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Device {
    pub id: u8,
    pub scan_interval: Duration,
    tags: BTreeMap<String, String>,
    input_registers: Registers,
}

impl Device {
    pub fn new(
        id: u8,
        scan_interval: Duration,
        tags: BTreeMap<String, String>,
        input_registers: BTreeMap<u16, Register>,
    ) -> Self {
        Self {
            id,
            scan_interval,
            tags,
            input_registers: Registers::new(input_registers),
        }
    }

    pub fn read(&self, mb: &mut impl Client) -> Result<String, Error> {
        let mut influx_lines = String::new();

        let register_map = &self.input_registers.map;
        for req in &self.input_registers.requests {
            mb.set_uid(self.id);
            let resp = mb.read_input_registers(req.start, req.len())?;

            let id_string = self.id.to_string();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            // Round to interval granularity
            let interval = self.scan_interval.as_nanos();
            let timestamp = (timestamp / interval) * interval;

            for (addr, reg) in register_map.range(req.start..req.end) {
                let start_idx = (addr - req.start) as usize;
                let data = &resp[start_idx..];

                let value = reg.data_type.parse_data(data) * reg.scaling;
                let tag_iter = self
                    .tags
                    .iter()
                    .chain(&reg.tags)
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .chain(iter::once(("modbus_id", id_string.as_str())));
                influx_lines.push_str(&influxdb_line(&reg.name, tag_iter, value, timestamp));
            }
        }

        Ok(influx_lines)
    }
}

fn influxdb_line<'a, I>(measurement: &str, tags: I, value: f64, timestamp: u128) -> String
where
    I: Iterator<Item = (&'a str, &'a str)>,
{
    let escape_meas = |s: &str| s.replace(',', "\\,").replace(' ', "\\ ");
    let escape_tag = |s: &str| escape_meas(s).replace('=', "\\=");

    let mut line = escape_meas(measurement);
    for (k, v) in tags {
        line.push_str(&format!(",{}={}", escape_tag(k), escape_tag(v)));
    }
    line.push_str(&format!(" value={} {}\n", value, timestamp));
    line
}

#[derive(Debug, PartialEq)]
struct Registers {
    // Addr as key
    map: BTreeMap<u16, Register>,
    requests: Vec<Request>,
}

impl Registers {
    fn new(map: BTreeMap<u16, Register>) -> Self {
        let mut requests: Vec<Request> = Vec::new();

        // Registers are sorted by address
        for reg in &map {
            let curr = Request::new(*reg.0, reg.1.data_type.num_registers());
            match requests.last_mut() {
                // Append consecutive registers to the current request
                Some(ref mut prev) if curr.start <= prev.end => {
                    if curr.end > prev.end {
                        prev.end = curr.end;
                    }
                }

                // Create a new request for all others
                _ => requests.push(curr),
            }
        }

        Self { map, requests }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Register {
    pub data_type: DataType,
    pub scaling: f64,

    pub name: String,
    pub tags: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq)]
struct Request {
    pub start: u16,
    pub end: u16,
}

impl Request {
    fn new(addr: u16, len: u16) -> Self {
        Self {
            start: addr,
            end: addr + len,
        }
    }

    fn len(&self) -> u16 {
        self.end - self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registers_consecutive() {
        let mut registers = BTreeMap::new();
        registers.insert(
            1,
            Register {
                name: String::from("foobar"),
                tags: BTreeMap::new(),
                data_type: DataType::F32,
                scaling: 8.7,
            },
        );
        registers.insert(
            3,
            Register {
                name: String::from("quxbaz"),
                tags: BTreeMap::new(),
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );

        let requests = vec![Request::new(1, 3)];
        assert_eq!(requests, Registers::new(registers).requests);
    }

    #[test]
    fn test_requests_from_registers_split() {
        let mut registers = BTreeMap::new();
        registers.insert(
            1,
            Register {
                name: String::from("foobar"),
                tags: BTreeMap::new(),
                data_type: DataType::F32,
                scaling: 8.7,
            },
        );
        registers.insert(
            8,
            Register {
                name: String::from("quxbaz"),
                tags: BTreeMap::new(),
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );

        let requests = vec![Request::new(1, 2), Request::new(8, 1)];
        assert_eq!(requests, Registers::new(registers).requests);
    }

    #[test]
    fn test_requests_from_registers_overlapping() {
        let mut registers = BTreeMap::new();
        registers.insert(
            1,
            Register {
                name: String::from("foobar"),
                tags: BTreeMap::new(),
                data_type: DataType::F64,
                scaling: 8.7,
            },
        );
        registers.insert(
            3,
            Register {
                name: String::from("quxbaz"),
                tags: BTreeMap::new(),
                data_type: DataType::U16,
                scaling: 1.0,
            },
        );

        let requests = vec![Request::new(1, 4)];
        assert_eq!(requests, Registers::new(registers).requests);
    }

    #[test]
    fn test_register_parse_data() {
        let data: [u16; 4] = [0x2468, 0xACF0, 0x0002, 0x0004];

        let dt = DataType::U16;
        assert_eq!(dt.parse_data(&data[..]), 0x2468u16 as f64);

        let dt = DataType::U32;
        assert_eq!(dt.parse_data(&data[..]), 0x2468ACF0u32 as f64);

        let dt = DataType::I16;
        assert_eq!(dt.parse_data(&data[..]), 0x2468i16 as f64);

        let dt = DataType::I32;
        assert_eq!(dt.parse_data(&data[..]), 0x2468ACF0i32 as f64);
    }
}
