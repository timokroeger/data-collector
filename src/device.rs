use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

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
    id: u8,
    scan_interval: Duration,
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

    pub fn get_scan_interval(&self) -> Duration {
        self.scan_interval
    }

    pub fn read(&self, mb: &mut impl Client) -> Result<(), Error> {
        let register_map = &self.input_registers.map;
        for req in &self.input_registers.requests {
            mb.set_uid(self.id);
            let resp = mb.read_input_registers(req.start, req.len())?;

            for (addr, reg) in register_map.range(req.start..req.end) {
                let start_idx = (addr - req.start) as usize;
                let data = &resp[start_idx..];

                // TODO: Store and return parsed values
                println!(
                    "{} = {}",
                    reg.name,
                    reg.data_type.parse_data(data) * reg.scaling
                );
            }
        }

        Ok(())
    }
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
