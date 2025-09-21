# KV-Store

A crash-safe, persistent key-value store implementation in Rust with Write-Ahead Logging (WAL) for data durability and recovery.

## Features

- **Crash-safe persistence**: Uses Write-Ahead Logging with CRC32 checksums for data integrity
- **In-memory performance**: Fast HashMap-based lookups with disk persistence
- **Multiple data types**: Support for strings, integers, and floats with type-safe encoding
- **Atomic operations**: All operations are atomic and consistent
- **Log corruption recovery**: Gracefully handles partial writes and corrupted log entries
- **Hot startup**: Fast recovery by replaying the transaction log on startup

## Architecture

The KV-Store uses a hybrid approach combining:

- **In-memory HashMap**: For fast key-value lookups and storage
- **Write-Ahead Log (WAL)**: Binary log file for persistence and crash recovery
- **Frame-based protocol**: Structured binary format with checksums for reliability

### Storage Format

Each operation is stored as a binary frame in the WAL with the following structure:

```
[total_len: u32][magic: u8][version: u8][operation: u8][encoding: u8]
[key_len: u32][value_len: u32][key_bytes][value_bytes][crc32: u32]
```

- **Operations**: SET (0x01), DELETE (0x02)
- **Encodings**: String (0x00), Integer (0x01), Float (0x02)
- **Integrity**: CRC32 checksum covers the entire frame payload

## Installation

### Prerequisites

- Rust 1.70+ (edition 2024)
- Cargo

### Build from source

```bash
git clone <repository-url>
cd kv-store
cargo build --release
```

## Usage

### Basic Example

```rust
use kv_store::{KVstore, Value, Encoding};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open or create a KV store with magic byte 0xAA and version 0x01
    let mut store = KVstore::open("my_store.log", 0xAA, 0x01)?;

    // Store a string value
    let greeting = Value {
        encoding: Encoding::String,
        bytes: b"Hello, World!".to_vec(),
    };
    store.set("greeting", greeting)?;

    // Store an integer value
    let number = Value {
        encoding: Encoding::Integer,
        bytes: 42i64.to_le_bytes().to_vec(),
    };
    store.set("answer", number)?;

    // Retrieve values
    let greeting = store.get("greeting")?;
    println!("Greeting: {}", String::from_utf8_lossy(&greeting.bytes));

    let answer_bytes = store.get("answer")?.bytes;
    let answer = i64::from_le_bytes(answer_bytes.try_into().unwrap());
    println!("Answer: {}", answer);

    // Delete a key
    store.del("greeting")?;

    Ok(())
}
```

### Running the Demo

The included demo shows basic operations and persistence:

```bash
cargo run
```

The demo will:

1. Load any existing data from `kvstore.log`
2. Retrieve and display stored values
3. Show the current state of the in-memory map

Run the program multiple times to see persistence in action!

## API Reference

### `KVstore`

The main key-value store struct.

#### Methods

- `open(log_path, magic, version) -> Result<KVstore, KVerror>`

  - Opens or creates a KV store with the specified log file
  - `magic` and `version` are used for compatibility checking

- `get(key: &str) -> Result<Value, KVerror>`

  - Retrieves a value by key
  - Returns `KVerror::NotFound` if the key doesn't exist

- `set(key: &str, value: Value) -> Result<(), KVerror>`

  - Stores a key-value pair
  - Automatically persists to the WAL

- `del(key: &str) -> Result<(), KVerror>`
  - Deletes a key-value pair
  - Writes a deletion record to the WAL

### `Value`

Represents a typed value in the store.

```rust
pub struct Value {
    pub encoding: Encoding,
    pub bytes: Vec<u8>,
}
```

### `Encoding`

Supported data types:

- `Encoding::String` - UTF-8 strings
- `Encoding::Integer` - 64-bit integers (little-endian)
- `Encoding::Float` - 64-bit floats

### Error Handling

All operations return `Result<T, KVerror>` where `KVerror` can be:

- `KVerror::Startup` - Failed to initialize the store
- `KVerror::IO` - File I/O operation failed
- `KVerror::CorruptLog` - Log file corruption detected
- `KVerror::NotFound` - Key not found in store
- `KVerror::Encoding` - Invalid encoding type

## Data Safety and Recovery

### Crash Safety

The KV-Store is designed to be crash-safe:

- All writes are immediately persisted to the WAL
- Each frame includes a CRC32 checksum for integrity verification
- Partial writes are detected and ignored during recovery
- The store can recover to the last consistent state after any crash

### Recovery Process

On startup, the store:

1. Opens the existing log file (if present)
2. Sequentially reads and validates each frame
3. Applies SET operations to rebuild the in-memory map
4. Processes DELETE operations to remove keys
5. Stops at the first corrupted or incomplete frame
6. Ignores any corrupted tail data from incomplete writes

### Data Integrity

- CRC32 checksums protect against data corruption
- Magic bytes and version numbers ensure compatibility
- Frame length validation prevents buffer overruns
- UTF-8 validation for string keys

## Performance Characteristics

- **Read operations**: O(1) HashMap lookup performance
- **Write operations**: O(1) HashMap update + O(1) log append
- **Startup time**: O(n) where n is the number of operations in the log
- **Memory usage**: O(k) where k is the number of unique keys
- **Disk usage**: Grows with the number of operations (compaction not yet implemented)

## Limitations

- No automatic log compaction (planned for future versions)
- Single-threaded operations (no concurrent access)
- No compression of stored values
- No built-in backup or replication features
- Keys must be valid UTF-8 strings

## Future Enhancements

- [ ] Log compaction to reduce disk usage
- [ ] Concurrent read/write operations with proper locking
- [ ] Compression support for large values
- [ ] Batch operations for improved performance
- [ ] Snapshot and backup functionality
- [ ] Network protocol for distributed access
- [ ] Replication and clustering support

## Dependencies

- `byteorder` (1.5.0) - For endian-aware binary serialization
- `crc32fast` (1.5.0) - For fast CRC32 checksum computation

## License

MIT

## Contributing

Do as you please but do no harm!

## Acknowledgments

This implementation follows the design principles outlined in `process.md` for building a minimal yet robust key-value store suitable for extension into a distributed system.
