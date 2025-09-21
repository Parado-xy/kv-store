Alright, let’s lock down a **clean, crash-safe, in-memory-with-persistence** KV store you can actually build and extend into a distributed system. I’ll give you an implementation plan that’s specific enough to execute **without dumping code**. If you want the code after, say the word and I’ll write it.

---

# Minimal-yet-solid design (single process)

## Core goals

* **Hot startup**: rebuild state fast from disk.
* **Safety**: never corrupt the log; tolerate crashes at any point.
* **Simplicity**: HashMap + append-only log + occasional compaction.
* **Concurrency**: multi-reader, single-writer; avoid global pauses.

## Public API (Rust signatures only; no code)

* `struct KvStore`
* `impl KvStore`

  * `fn open(path: impl AsRef<Path>) -> Result<KvStore, KvError>`
  * `fn set(&self, key: String, value: Vec<u8>) -> Result<(), KvError>`
  * `fn get(&self, key: &str) -> Result<Option<Vec<u8>>, KvError>`
  * `fn delete(&self, key: &str) -> Result<(), KvError>`
  * `fn flush(&self) -> Result<(), KvError>` (force durability when needed)
  * `fn len(&self) -> usize`
  * `fn compact(&self) -> Result<(), KvError>` (manual trigger; also auto)
* `enum KvError { Io, CorruptLog, NotFound, Closed, ... }`

Keys are UTF-8 strings; values are arbitrary bytes (lets you store JSON, bincode, whatever). Return `NotFound` for `delete` on missing key.

---

# Storage model

## In-memory index

* `HashMap<String, ValueMeta>` guarded by `RwLock`.
* `ValueMeta { file_id: u64, offset: u64, len: u32, crc32: u32 }`

  * For the current design (single active log), `file_id` is the active log id; after compaction it bumps.

## Write-ahead log (WAL)

* **Single active append-only file**: `wal/current.log` and a symlink/marker to it (`wal/ACTIVE`).
* **Record framing** (little-endian):

  ```
  [MAGIC: u32 = 0xKV01]
  [VERSION: u16 = 1]
  [OP: u8]            // 0=set, 1=del
  [key_len: u32]
  [val_len: u32]      // 0 for del
  [key bytes]
  [value bytes?]
  [CRC32: u32]        // of OP..value bytes
  ```
* **Durability policy** (choose one to start):

  * *Safe*: `fsync` on every write → slower but simplest.
  * *Batch*: `fsync` every N ms or after M bytes → much faster; expose `flush()`.

## Startup replay

1. Find `wal/ACTIVE`, open `current.log`, scan sequentially.
2. For each good record (CRC matches), apply to the HashMap:

   * `set`: index points to this record’s `(offset, len, crc)`.
   * `del`: remove from index.
3. Stop at first corrupt/truncated record → **ignore tail** (previous write crashed mid-flight).
4. Record `alive_bytes` as the sum of live value lengths for compaction heuristics.

---

# Concurrency model

* **Map**: `RwLock<HashMap<...>>`

  * `get`: `map.read()`.
  * `set/delete`: `map.write()` only for the map update; not for disk I/O.
* **Log appends**: a dedicated `Mutex<File>` (or a single-threaded writer task receiving messages over a channel for higher throughput).
* **Lock order** (to avoid deadlocks):

  1. Acquire WAL writer lock → append record → flush/optionally fsync → 2) Acquire `map.write()` → update index → release.
     Reads only take `map.read()`.

---

# Compaction

**Why**: the log grows; old values remain dead.
**Trigger**: when `dead_bytes / total_bytes > 0.5` **or** `total_bytes > 256MB` (tune later).

**Algorithm**:

1. Create `wal/compact-<new_id>.log`.
2. Iterate `map` under `map.read()`. For each `(k, meta)`, read from old log and write a **fresh SET** record into the new log; build a **new index** in RAM as you go.
3. `fsync` the new file; atomically:

   * Write `wal/ACTIVE` → `compact-<new_id>.log`.
   * Rename `compact-<new_id>.log` → `current.log` (or swap symlink).
4. Replace the in-memory index with the new one in a single `RwLock` write section.
5. Delete old log(s).

**Safety**: If a crash happens mid-compaction, you still have the old ACTIVE log; on boot you either see the old one or a complete new one—never a half state.

---

# Observability

* Use `tracing`:

  * `info`: node id, opened path, startup replay stats.
  * `debug`: set/get/delete commands, offsets.
  * `warn`: CRC mismatch, truncated tail, compaction retries.
* Expose `/metrics` later (Prometheus) but don’t block MVP on it.

---

# Tests you should absolutely run (no excuses)

1. **Replay correctness**

   * Write N mixed ops; close; reopen; verify last-write-wins.
2. **Tail truncation**

   * Append half a record; simulate crash (kill process); reopen; ensure it ignores tail without panicking.
3. **Compaction idempotence**

   * After compaction, re-open and verify all keys/values identical; old files gone.
4. **Concurrency**

   * Hammer with multiple reader threads + one writer; assert no panics, results consistent.
5. **Fuzz the parser**

   * Feed random bytes to the WAL reader; ensure it never UB/panics; only returns `CorruptLog`.
6. **Durability**

   * With `fsync` on each write: kill process after `set`; on reboot `get` must return the value.
7. **Load**

   * Insert 1M small keys; ensure memory stays bounded except for map; compaction reclaims disk.

*Use `proptest` for fuzz/property tests and `criterion` for microbenchmarks later.*

---

# Directory layout

```
your-kv/
├─ Cargo.toml
├─ src/
│  ├─ lib.rs           // KvStore, errors, open()
│  ├─ wal.rs           // record encode/decode, reader, writer
│  ├─ index.rs         // ValueMeta, map ops
│  ├─ compaction.rs    // compact logic
│  └─ errors.rs
├─ wal/
│  ├─ ACTIVE           // points to current.log (or contains its file_id)
│  └─ current.log
└─ tests/
   ├─ replay.rs
   ├─ compaction.rs
   └─ concurrency.rs
```

---

# Hard edges & choices (decide now)

* **Checksum**: CRC32 is fast and sufficient here. If you want stronger, use `blake3` (slower, stronger).
* **Serialization**: Don’t use JSON/TOML/YAML; they add ambiguity. Stick to your fixed binary frame.
* **Value size**: Allow up to `u32::MAX` in the frame, but practically cap large values (e.g., 32–64MB) to avoid pathological memory spikes during compaction reads.
* **Fsync policy**: Start safe (per write). Add a config `Durability::Immediate | ::Batch { bytes, millis }`.

---

# Small text protocol (for later networking)

Keep it dead simple so you can wrap your store in a TCP server without refactoring:

```
SET <key> <len>\n<raw bytes>
GET <key>\n
DEL <key>\n
OK\n / NOT_FOUND\n / VALUE <len>\n<raw bytes>
```

This maps 1:1 to your API and keeps the store logic separate from transport.

---

# Roadmap to “distributed later”

1. **Process ID & node identity**: give each instance a stable `node_id` (e.g., a UUID in `wal/NODE_ID`).
2. **Replication**: add a *logical log sequence number* (LSN). Every appended record gets `lsn += 1`. Followers replicate by LSN.
3. **Membership**: start with a static peers list in a config file. Don’t touch consensus yet.
4. **Consistency**: after you’re comfy, pick a path:

   * *Primary/replica* (simpler; single-writer).
   * *Raft* (stronger guarantees; more moving parts).

---

# What you build first (checklist)

* [ ] `KvStore::open()` that replays `current.log` and builds the index.
* [ ] Framed WAL writer with CRC and `flush()`/`fsync` policy.
* [ ] `set/get/delete` wired to WAL + index with the correct lock order.
* [ ] Tail-tolerant WAL reader (stops cleanly at first bad frame).
* [ ] Auto-compaction trigger + safe, atomic swap.
* [ ] Tests: replay, truncation, compaction, concurrency, durability.

If you want, say the word and I’ll translate this spec into **production-ready Rust code** (crate layout, modules, and a few targeted tests) in one go.
