use core::hash;
use std::{char, clone, collections::HashMap, fs::File, hash::Hasher, io::{self, prelude::*, BufReader, BufWriter, Write}, path::Prefix, sync, thread::{self, sleep, spawn}, time::{Duration, Instant}};
use clap::{builder::Str, Parser};
use hmac::{Hmac, HmacCore, Mac};
use rand::{distributions::Alphanumeric, Rng};
use ring::hmac::{sign, HMAC_SHA256};
use rsa::{pkcs1::pem::encode, pkcs8::PrivateKeyInfo, PaddingScheme, PublicKey, PublicKeyParts, RsaPrivateKey, RsaPublicKey};
use num_traits::cast::ToPrimitive;
use rand::rngs::OsRng;
use serde::{de::value, Deserialize, Serialize};
use tungstenite::{accept, connect, WebSocket};
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream};
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio::task;
use sha2::{Digest, Sha256};
// use args::{Args, ArgsError};
use tokio::{net::{TcpListener, TcpStream}, sync::mpsc, time};
use futures_util::{StreamExt};

const CLIENT_COUNT: usize = 5;
#[derive(Parser,Debug)]
struct Args {
    #[clap(long)]
    mode: String,
    #[clap(long, default_value = "10")]
    times: u32,
}

#[derive(Serialize, Deserialize, Debug)]

struct BTCPriceData {
    prices: Vec<f64>,
    average: f64
}

#[derive(Debug, Clone)]
struct SignedMessage {
    pub message: String,
    pub signature: Vec<u8>,
    pub public_key: RsaPublicKey,
    pub client_id: usize,
}

async fn cache_mode(times: u32) {
    let (mut socket, _response) =  connect_async("wss://stream.binance.com:9443/ws/btcusdt@ticker").await.expect("Failed to connect to the websocket");
    let mut socket_stream: WebSocketStream<MaybeTlsStream<TcpStream>> = socket;
    let mut prices: Vec<f64> = Vec::new();
    println!("Listening for {} seconds", times);
    let mut interval: tokio::time::Interval = tokio::time::interval(Duration::from_secs(10));
    interval.tick().await;

    
    loop {
        tokio::select! {
            msg = socket_stream.next() => {
                match msg {
                    Some(Ok(Message::Text(msg))) => {
                        println!("Received message {msg}");
        
                        let price = extract_usd_btc_price(&msg);
                        prices.push(price);
                        println!("Received BTC/USD price is {msg}")
                    }
                    Some(Err(e)) => {
                        eprintln!("Error receieving the message {e}")
                    }
                    None => {
                        println!("Websocket closed");
                        break;
                    }
                    _ => {
                        println!("Received Unwanted msg");
                    }
                }
            }
            _tick = interval.tick() => {
                println!("Duration Over");
                break;
            }
        }
    }

    // for _ in 0..times {
    //     match socket_stream.next().await {
    //         Some(Ok(Message::Text(msg))) => {
    //             println!("Received message {msg}");

    //             let price = extract_usd_btc_price(&msg);
    //             println!("Received BTC/USD price is {msg}")
    //         }
    //         Some(Err(e)) => {
    //             eprintln!("Error receieving the message {e}")
    //         }
    //         None => {6
    //             println!("Websocket closed");
    //             break;
    //         }
    //         _ => println!("Incorrect usage, Use --mode=cache or --mode=read")
    //     }
    //     sleep(Duration::from_secs(5));
    // }

    let average = prices.iter().sum::<f64>() / prices.len() as f64;
    println!("The cache complete the average prices of the BTC USD prices is : {}", average);

    let btc_price_data = BTCPriceData {
        prices,
        average
    };

    let file = File::create("btc_usd_price.json").expect("Failed to create a file");
    let writer = BufWriter::new(file);
    serde_json::to_writer(writer, &btc_price_data).expect("Failed to write the data to the file");
}



fn extract_usd_btc_price(msg: &str) -> f64 {
    let parsed: serde_json::Value = serde_json::from_str(msg).expect("Failed to parse the message");
    parsed["c"].as_str().expect("No Price Field").parse::<f64>().expect("Failed to parse the price")
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    match args.mode.as_str() {
        "cache" => cache_mode(args.times).await,
        "read"  => read_mode(),
        _ => println!("Invalid mode. Use --mode=cache or --mode=read")
    }

    // let key = "my secret key";
    // signed_messages("msg", key);

    let buffer:usize = 32;
    let (tx,  rx) = mpsc::channel(32);
    let mut handles = vec![];
    let mut rng = OsRng;
    let mut public_keys = HashMap::new();
    let mut private_keys: Vec<RsaPrivateKey> = Vec::new();
    
    for i in 0..5 {
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("Failed to generate private key");
        let public_key = RsaPublicKey::from(&private_key);

        public_keys.insert(i, public_key);
        private_keys.push(private_key);
        
    }
    

    for i in 0..5 {
        let tx_clone = tx.clone();
        let private_key_clone = private_keys[i].clone();
        let handle = task::spawn(async move {
            client_process(i, tx_clone, private_key_clone).await;
        });
        handles.push(handle);
    }

    drop(tx);

    let aggregator_process = task::spawn(async move {
        aggregator_process(rx, public_keys).await
    });

    for handle in handles {
        handle.await.unwrap();
    }

    aggregator_process.await.unwrap();

    println!("All processes completed")
    

}

fn read_mode() {
    let file = File::open("btc_usd_price.json").expect("Failed to open the file");
    let bufreader = BufReader::new(file);

    let btc_price_data: BTCPriceData = serde_json::from_reader(bufreader).expect("Failed to read the price from the file");

    println!("The Read data is {:?}", btc_price_data);
    println!("The Average USD BTC price is {:?}", btc_price_data.average)

}


async fn client_process(client_id: usize, tx: mpsc::Sender<SignedMessage>, private_key: RsaPrivateKey) {
    println!("The client {} is getting started at {:?}", client_id, chrono::Local::now());

    let start_time = Instant::now();
    let mut values: Vec<f64> = Vec::new();

    // Collect random values for a certain duration
    while start_time.elapsed() < Duration::new(10, 0) {
        let value = rand::random::<f64>() * 100.0;
        values.push(value);
        println!("The client {} has provided the value: {:?}", client_id, value);
        sleep(Duration::from_secs(1)); // Simulate a delay between readings
    }

    // Calculate average
    let average = if !values.is_empty() {
        values.iter().sum::<f64>() / values.len() as f64
    } else {
        0.0
    };
    println!("The client {} provided an average value: {:?}", client_id, average);

    // Sign the average
    let average_message = average.to_string(); // Convert average to string for consistency
    let hashed_message = Sha256::digest(average_message.as_bytes()); // Hash the average message

    // Sign the hash using the private key
    let public_key = RsaPublicKey::from(&private_key);
    let prefix = format!("Client_Id: {}", client_id); // Create prefix
    let padding = PaddingScheme::PKCS1v15Sign { hash_len: None, prefix: Box::from(prefix.as_bytes()) }; // Use the prefix here
    let signature = private_key.sign(padding, &hashed_message).expect("Failed to sign message");

    // Create the signed message
    let signed_message = SignedMessage {
        message: average_message, // Use the string representation of the average
        signature,
        public_key,
        client_id
    };

    // Send the signed message to the aggregator
    tx.send(signed_message).await.unwrap_or_else(|e| {
        println!("Failed to send signed message: {:?}", e);
    });
}







type HmacSha256 = Hmac<Sha256>;
fn signed_messages(msg: &str, key: &str) {
    let random_msg: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();
    println!("The random messge is {:?}", random_msg);

    //generate a mac key for the provided key string
    let mut mac: HmacSha256 = HmacSha256::new_from_slice(&key.as_bytes()).expect("Failed to create a Hmac instance");

    //update the hmac with the random msg;
    mac.update(random_msg.as_bytes());

    //finalize the mac to get the result
    let result = mac.finalize();

    let signed_message = result.into_bytes();
    println!("The Signed message is {:?}", signed_message)


}

async fn aggregator_process(mut rx: mpsc::Receiver<SignedMessage>, public_keys: HashMap<usize, RsaPublicKey>) {
    println!("The aggregator is waiting for client averages.....");

    let mut averages: Vec<f64> = Vec::new();

    for _ in 0..5 {
        let signed_message = rx.recv().await.unwrap();

        // Hash the received message
        let hashed_message = Sha256::digest(signed_message.message.as_bytes()); // Hash the string representation
        let hashed_message_vec = hashed_message.to_vec();

        // Retrieve the public key using the correct client ID
        let client_id = signed_message.client_id; // Ensure SignedMessage has client_id field
        if let Some(public_key) = public_keys.get(&client_id) {
            // Creating a valid prefix for the padding
            let prefix = format!("Client_Id: {}", client_id);
            let padding = PaddingScheme::PKCS1v15Sign {
                hash_len: None,
                prefix: Box::from(prefix.as_bytes()), // Ensure this matches how it was signed
            };

            // Verify the signature
            if public_key.verify(padding, &hashed_message_vec, &signed_message.signature).is_ok() {
                println!("Signature valid for the client with average: {:?}", signed_message.message);

                // Parse the valid signed message into f64 to push to averages
                let average: f64 = signed_message.message.parse::<f64>().unwrap_or(0.0);
                averages.push(average);
            } else {
                println!("Signature invalid for the client with average: {:?}", signed_message.message);
            }
        } else {
            println!("Public key not found for the given client ID: {:?}", client_id);
        }
    }

    // Calculate overall averages
    let overall_averages: f64 = if !averages.is_empty() {
        averages.iter().sum::<f64>() / averages.len() as f64
    } else {
        0.0
    };
    println!("The aggregator computed overall averages from the clients: {:?}", overall_averages);
}


// fn main() {
//     let server = TcpListener::bind("127.0.0.1:9001").unwrap();

//     for stream in server.incoming() {
//         spawn(move || {
//             let mut websocket = accept(stream.unwrap()).unwrap();
//             loop {
//                 let msg = websocket.read().unwrap();
//                 if msg.is_binary() || msg.is_text() {
//                     websocket.send(msg).unwrap();
//                 }

//             }
//         });
//     }
    
// }

// fn handle_connection(mut stream: TcpStream) {
//     let buf_reader = BufReader::new(&mut stream);

//     let http_request: Vec<_>= buf_reader
//     .lines()
//     .map(|result| result.unwrap())
//     .take_while(|result| !result.is_empty())
//     .collect();

//     println!("The Request is {http_request:#?}");

// }

