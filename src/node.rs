use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crc32fast;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{BufReader, Cursor, Read, Write},
    path::Path,
    fmt,
    error::Error
};

// ------------------- Errors -------------------

#[derive(Debug)]
pub enum KVerror {
    Startup,
    IO,
    CorruptLog,
    NotFound,
    Encoding,
}

impl fmt::Display for KVerror {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KVerror::Startup => write!(f, "Failed to start KVstore"),
            KVerror::IO => write!(f, "IO operation failed"),
            KVerror::CorruptLog => write!(f, "Log file is corrupted"),
            KVerror::NotFound => write!(f, "Key not found"),
            KVerror::Encoding => write!(f, "Invalid encoding"),
        }
    }
}

impl Error for KVerror {}

// ------------------- Encoding -------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Encoding {
    String = 0x00,
    Integer = 0x01,
    Float = 0x02,
}

impl Encoding {
    fn from_u8(b: u8) -> Result<Encoding, KVerror> {
        match b {
            0x00 => Ok(Encoding::String),
            0x01 => Ok(Encoding::Integer),
            0x02 => Ok(Encoding::Float),
            _ => Err(KVerror::Encoding),
        }
    }
}

// ------------------- Frame -------------------

#[derive(Debug)]
pub struct Frame {
    total_len: u32,
    magic: u8,
    version: u8,
    operation: u8,
    encoding: u8,
    key_len: u32,
    value_len: u32,
    key_bytes: Vec<u8>,
    value_bytes: Vec<u8>,
}

// -------------------- Value ---------------------
#[derive(Clone, Debug)]
pub struct Value {
    pub encoding: Encoding,
    pub bytes: Vec<u8>,
}

// ------------------- KV Store -------------------

pub struct KVstore {
    pub map: HashMap<String, Value>,
    log: String,
    magic: u8,
    version: u8,
}

impl KVstore {
    pub fn open(log: impl AsRef<Path>, magic: u8, version: u8) -> Result<KVstore, KVerror> {
        let path = log.as_ref();
        let mut store = KVstore {
            map: HashMap::new(),
            log: log.as_ref().to_string_lossy().into_owned(),
            magic,
            version,
        };

        if path.exists() {
            let file = File::open(path).map_err(|_| KVerror::IO)?;
            store = build_kv_store(file, &store.log, magic, version)?;
        } else {
            File::create(path).map_err(|_| KVerror::IO)?;
        }

        Ok(store)
    }

    fn append(&self, value: Vec<u8>) -> Result<(), KVerror> {
        let mut log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .map_err(|_| KVerror::IO)?;
        log_file.write_all(&value).map_err(|_| KVerror::IO)
    }

    pub fn get(&self, key: &str) -> Result<Value, KVerror> {
        self.map.get(key).cloned().ok_or(KVerror::NotFound)
    }

    pub fn set(&mut self, key: &str, value: Value) -> Result<(), KVerror> {
        let frame = Frame::new_set(key, value.clone(), self.magic, self.version);
        let serialized = serialize(frame);

        self.append(serialized)?; // append first to maintain consistency
        self.map.insert(key.to_string(), value);
        Ok(())
    }

    pub fn del(&mut self, key: &str) -> Result<(), KVerror> {
        let frame = Frame::new_delete(key, self.magic, self.version);
        let serialized = serialize(frame);

        self.append(serialized)?; // append first
        self.map.remove(key);
        Ok(())
    }
}

// ------------------- Frame Helpers -------------------

impl Frame {
    pub fn new_set(key: &str, value: Value, magic: u8, version: u8) -> Frame {
        let key_bytes = key.as_bytes().to_vec();
        let value_bytes = value.bytes.clone();
        let key_len = key_bytes.len() as u32;
        let value_len = value_bytes.len() as u32;

        Frame {
            total_len: 0,
            magic,
            version,
            operation: 0x01,
            encoding: value.encoding as u8,
            key_len,
            value_len,
            key_bytes,
            value_bytes,
        }
    }

    pub fn new_delete(key: &str, magic: u8, version: u8) -> Frame {
        let key_bytes = key.as_bytes().to_vec();
        let key_len = key_bytes.len() as u32;

        Frame {
            total_len: 0,
            magic,
            version,
            operation: 0x02,
            encoding: 0x00,
            key_len,
            value_len: 0,
            key_bytes,
            value_bytes: vec![],
        }
    }
}

// ------------------- Build Store from Log -------------------

fn build_kv_store(
    log_file: File,
    log_path: &str,
    magic: u8,
    version: u8,
) -> Result<KVstore, KVerror> {
    let mut store = KVstore {
        map: HashMap::new(),
        log: log_path.to_string(),
        magic,
        version,
    };

    let mut reader = BufReader::new(log_file);

    loop {
        // First step is to get the length bytes (u32)
        let mut len_buf = [0u8; 4];
        if reader.read_exact(&mut len_buf).is_err() {
            break; // EOF
        }
        let total_len = u32::from_le_bytes(len_buf);

        let mut frame_buf = vec![0u8; total_len as usize];
        reader
            .read_exact(&mut frame_buf)
            .map_err(|_| KVerror::CorruptLog)?;

        let frame = deserialize(total_len, frame_buf)?;

        match frame.operation {
            0x01 => {
                let key = String::from_utf8(frame.key_bytes).map_err(|_| KVerror::CorruptLog)?;
                let encoding = Encoding::from_u8(frame.encoding)?;
                store.map.insert(
                    key,
                    Value {
                        encoding,
                        bytes: frame.value_bytes,
                    },
                );
            }
            0x02 => {
                let key = String::from_utf8(frame.key_bytes).map_err(|_| KVerror::CorruptLog)?;
                store.map.remove(&key);
            }
            _ => return Err(KVerror::CorruptLog),
        }
    }

    Ok(store)
}

// ------------------- Checksum -------------------

fn compute_checksum(frame: &Frame) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[frame.magic]);
    hasher.update(&[frame.version]);
    hasher.update(&[frame.operation]);
    hasher.update(&[frame.encoding]);
    hasher.update(&frame.key_len.to_le_bytes());
    hasher.update(&frame.value_len.to_le_bytes());
    hasher.update(&frame.key_bytes);
    hasher.update(&frame.value_bytes);
    hasher.finalize()
}

// ------------------- Serialize -------------------

fn serialize(frame: Frame) -> Vec<u8> {
    let checksum = compute_checksum(&frame);
    let total_len: u32 = 1 + 1 + 1 + 1 + 4 + 4 + frame.key_len + frame.value_len + 4;

    let mut buffer = vec![];
    buffer.write_u32::<LittleEndian>(total_len).unwrap();
    buffer.write_u8(frame.magic).unwrap();
    buffer.write_u8(frame.version).unwrap();
    buffer.write_u8(frame.operation).unwrap();
    buffer.write_u8(frame.encoding).unwrap();
    buffer.write_u32::<LittleEndian>(frame.key_len).unwrap();
    buffer.write_u32::<LittleEndian>(frame.value_len).unwrap();
    buffer.write_all(&frame.key_bytes).unwrap();
    buffer.write_all(&frame.value_bytes).unwrap();
    buffer.write_u32::<LittleEndian>(checksum).unwrap();

    buffer
}

// ------------------- Deserialize -------------------

fn deserialize(total_len: u32, mut bytes: Vec<u8>) -> Result<Frame, KVerror> {
    let mut cursor = Cursor::new(&mut bytes);

    let magic = cursor.read_u8().map_err(|_| KVerror::CorruptLog)?;
    let version = cursor.read_u8().map_err(|_| KVerror::CorruptLog)?;
    let operation = cursor.read_u8().map_err(|_| KVerror::CorruptLog)?;
    let encoding = cursor.read_u8().map_err(|_| KVerror::CorruptLog)?;
    let key_len = cursor.read_u32::<LittleEndian>().map_err(|_| KVerror::CorruptLog)?;
    let value_len = cursor.read_u32::<LittleEndian>().map_err(|_| KVerror::CorruptLog)?;

    let expected_len = 1 + 1 + 1 + 1 + 4 + 4 + key_len + value_len + 4;
    if total_len != expected_len {
        return Err(KVerror::CorruptLog);
    }

    let mut key_bytes = vec![0u8; key_len as usize];
    cursor
        .read_exact(&mut key_bytes)
        .map_err(|_| KVerror::CorruptLog)?;

    let mut value_bytes = vec![0u8; value_len as usize];
    cursor
        .read_exact(&mut value_bytes)
        .map_err(|_| KVerror::CorruptLog)?;

    let original_checksum = cursor
        .read_u32::<LittleEndian>()
        .map_err(|_| KVerror::CorruptLog)?;

    let frame = Frame {
        total_len,
        magic,
        version,
        operation,
        encoding,
        key_len,
        value_len,
        key_bytes,
        value_bytes,
    };

    let computed = compute_checksum(&frame);
    if computed != original_checksum {
        return Err(KVerror::CorruptLog);
    }

    Ok(frame)
}
