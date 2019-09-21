use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, PartialEq)]
pub enum DataType {
    U16,
    U32,
    U64,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl FromStr for DataType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "u16" => Ok(Self::U16),
            "u32" => Ok(Self::U32),
            "u64" => Ok(Self::U64),
            "i16" => Ok(Self::I16),
            "i32" => Ok(Self::I32),
            "i64" => Ok(Self::I64),
            "f32" => Ok(Self::F32),
            "f64" => Ok(Self::F64),
            _ => Err(()),
        }
    }
}

impl DataType {
    fn num_registers(&self) -> u16 {
        match self {
            Self::U16 | Self::I16 => 1,
            Self::U32 | Self::I32 | Self::F32 => 2,
            Self::U64 | Self::I64 | Self::F64 => 4,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Register {
    pub data_type: DataType,
    pub scaling: f64,

    pub name: String,
    pub tags: BTreeMap<String, String>,
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
                data_type: DataType::U64,
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
}
