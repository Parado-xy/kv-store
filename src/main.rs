use crate::node::{KVstore, Value, Encoding};

mod node;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Path to WAL file
    let log_file = "kvstore.log";

    // Open store with default magic/version
    let mut store = KVstore::open(log_file, 0xAA, 0x01)?;
    println!("Initial map state: {:?}", store.map);

    // // ---- SET ---- (to test persistence)
    // let val1 = Value {
    //     encoding: Encoding::String,
    //     bytes: b"Hello, KVstore!".to_vec(),
    // };
    // store.set("greeting", val1)?;
    // println!("Set 'greeting'");

    // let val2 = Value {
    //     encoding: Encoding::Integer,
    //     bytes: 42i64.to_le_bytes().to_vec(),
    // };
    // store.set("answer", val2)?;
    // println!("Set 'answer'");

    // // Add a new key to test persistence
    // let val3 = Value {
    //     encoding: Encoding::String,
    //     bytes: b"Persistent data test".to_vec(),
    // };
    // store.set("persistence_test", val3)?;
    // println!("Set 'persistence_test'");

    // println!("Map after sets: {:?}", store.map);

    // // ---- GET ----
    let greeting = store.get("greeting")?;
    println!(
        "greeting: {}",
        String::from_utf8_lossy(&greeting.bytes)
    );

    let answer_bytes = store.get("answer")?.bytes;
    let answer = i64::from_le_bytes(answer_bytes.try_into().unwrap());
    println!("answer: {}", answer);

    let persistence_test = store.get("persistence_test")?;
    println!(
        "persistence_test: {}",
        String::from_utf8_lossy(&persistence_test.bytes)
    );

    // ---- DELETE ---- (comment this out to test persistence)
    // store.del("greeting")?;
    // match store.get("greeting") {
    //     Ok(_) => println!("Error: greeting still exists!"),
    //     Err(e) => println!("greeting deleted successfully: {:?}", e),
    // }

    println!("Final map state: {:?}", store.map);
    println!("Data should persist between runs. Run this program again to test!");

    Ok(())
}