//! Passive UDP-multicast monitor for conclave debates.
//!
//! Joins the multicast group with the same socket options as the agents
//! (SO_REUSEADDR/SO_REUSEPORT — multicast delivers a copy to every joined
//! socket, so nothing is stolen from running agents), decodes every
//! `AgentMessage` on the wire, and reports whether any `(sender_id, content)`
//! pair was observed more than once — the direct instrument for "are agents
//! duplicating messages on the wire?".
//!
//! Usage:
//! ```sh
//! cargo run --release --example wire_sniffer [MULTICAST_ADDRESS] [SECONDS]
//! ```
//! Defaults: `239.255.255.250:8080`, 60 seconds.

use conclave::message::AgentMessage;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "239.255.255.250:8080".to_string())
        .parse()?;
    let seconds: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let SocketAddr::V4(v4) = addr else {
        anyhow::bail!("IPv6 multicast is not supported: {addr}");
    };

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, v4.port())).into())?;
    socket.join_multicast_v4(v4.ip(), &Ipv4Addr::UNSPECIFIED)?;
    let socket: std::net::UdpSocket = socket.into();
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;

    eprintln!("Sniffing {addr} for {seconds}s (Ctrl-C to stop early)...");
    let start = Instant::now();
    let deadline = start + Duration::from_secs(seconds);
    let mut buf = vec![0u8; 65535];
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    let mut datagrams = 0usize;
    let mut failures = 0usize;

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                datagrams += 1;
                match AgentMessage::deserialize(&buf[..n]) {
                    Ok(msg) => {
                        let excerpt: String = msg.content.chars().take(70).collect();
                        println!(
                            "[{:>4}s] msg.timestamp={} {} ({}B via {from}): {excerpt}",
                            start.elapsed().as_secs(),
                            msg.timestamp,
                            msg.sender_id,
                            n
                        );
                        *seen.entry((msg.sender_id, msg.content)).or_insert(0) += 1;
                    }
                    Err(e) => {
                        failures += 1;
                        eprintln!("undecodable datagram ({n}B via {from}): {e}");
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(e) => return Err(e.into()),
        }
    }

    println!("---- summary ----");
    println!(
        "datagrams: {datagrams}, decode failures: {failures}, unique (sender, content) pairs: {}",
        seen.len()
    );
    let dups: Vec<_> = seen.iter().filter(|(_, count)| **count > 1).collect();
    if dups.is_empty() {
        println!("no duplicate (sender_id, content) pairs observed");
    } else {
        println!("DUPLICATES ON THE WIRE:");
        for ((sender, content), count) in dups {
            let excerpt: String = content.chars().take(70).collect();
            println!("  {sender} x{count}: {excerpt:?}");
        }
    }
    Ok(())
}
