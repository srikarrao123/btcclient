use std::{fs::File, io::{self, prelude::*, BufReader, BufWriter, Write}, thread::{sleep, spawn}, time::Duration};
use clap::{builder::Str, Parser};
use serde::{Deserialize, Serialize};
use tungstenite::{accept, connect, WebSocket};
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream};
use tokio_tungstenite::tungstenite::protocol::Message;
// use args::{Args, ArgsError};
use tokio::net::{TcpStream, TcpListener};
use futures_util::{StreamExt};

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
    //         None => {
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
    let mut args = Args::parse();
    match args.mode.as_str() {
        "cache" => cache_mode(args.times).await,
        "read"  => read_mode(),
        _ => println!("Invalid mode. Use --mode=cache or --mode=read")
    }

}

fn read_mode() {
    let file = File::open("btc_usd_price.json").expect("Failed to open the file");
    let bufreader = BufReader::new(file);

    let btc_price_data: BTCPriceData = serde_json::from_reader(bufreader).expect("Failed to read the price from the file");

    println!("The Read data is {:?}", btc_price_data);
    println!("The Average USD BTC price is {:?}", btc_price_data.average)

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

