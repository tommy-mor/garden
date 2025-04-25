use crate::{evaluate_form, Value, Error}; // Assuming evaluate_form exists in main.rs or lib.rs
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_bencode::{de, ser, value::Value as BencodeValue}; // Import BencodeValue for length heuristic
use serde_bytes::ByteBuf;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write, ErrorKind},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc, // Use std Arc (tokio re-exports it)
    error::Error as StdError // Import the standard Error trait
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, AsyncReadExt, BufReader}, // Import AsyncReadExt for read_buf
    net::{TcpListener, TcpStream},
    sync::{mpsc, Mutex}, // Use Tokio's Mutex
};
use bytes::{BytesMut, Buf}; // Added for buffer management

// Placeholder for nREPL message structure (adjust based on actual protocol needs)
#[derive(Serialize, Deserialize, Debug)]
struct NreplMsg {
    op: String,
    id: Option<String>,
    session: Option<String>,
    code: Option<String>,
    // Add other fields as needed (e.g., ns, file, line, column)
    #[serde(flatten)]
    extra: HashMap<String, serde_bencode::value::Value>, // Catch-all for unknown fields
}

// Response structure (simplified)
#[derive(Serialize, Debug)]
struct NreplResponse<'a> {
    id: Option<&'a str>,
    session: Option<&'a str>,
    #[serde(rename = "new-session", skip_serializing_if = "Option::is_none")]
    new_session: Option<&'a str>, // Field expected by clients for clone op
    status: Vec<&'a str>, // e.g., ["done"] or ["error", "eval-error"]
    value: Option<String>, // Value needs serialization - simple String for now
    ex: Option<String>, // Exception/Error message
    // Add other response fields (ns, etc.)
}


// Represents the state for each connected nREPL session
type SessionContext = Arc<Mutex<IndexMap<String, Value>>>; // Use Tokio's Mutex, std Arc
// Stores all active sessions
type SessionStore = Arc<Mutex<HashMap<String, SessionContext>>>; // Use Tokio's Mutex, std Arc


pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?; // Bind to localhost, random port
    let addr = listener.local_addr()?;
    println!("nREPL server listening on {}", addr);

    // Write .nrepl-port file
    write_nrepl_port_file(addr)?;

    // Initialize with Tokio's Arc and Mutex
    let sessions: SessionStore = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (stream, client_addr) = listener.accept().await?;
        println!("Accepted connection from: {}", client_addr);

        let sessions_clone = Arc::clone(&sessions); // Clone Arc for the new task

        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, sessions_clone).await {
                eprintln!("Error handling client {}: {}", client_addr, e);
            } else {
                 println!("Connection closed by client: {}", client_addr);
            }
        });
    }
}

fn write_nrepl_port_file(addr: SocketAddr) -> io::Result<()> {
    let port = addr.port();
    let path = PathBuf::from(".nrepl-port");
    let mut file = File::create(&path)?;
    write!(file, "{}", port)?;
    println!("Wrote port {} to {}", port, path.display());
    Ok(())
}

async fn handle_client(stream: TcpStream, sessions: SessionStore) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader); // Use reader directly
    let mut buffer = BytesMut::with_capacity(4096); // Buffer for incoming data


    // We need a way to associate this connection with a session.
    // For now, let's assume a default session per connection or manage via 'clone' op.
    // A simple unique ID for this connection might serve as a temporary session ID.
    let conn_id = uuid::Uuid::new_v4().to_string(); // Requires `uuid` crate
    let mut current_session_id: Option<String> = None;


    loop {
        let bytes_read = reader.read_buf(&mut buffer).await?;
        if bytes_read == 0 {
            // Connection closed cleanly by peer
            if buffer.is_empty() {
                break; // Clean exit
            } else {
                // Connection closed with partial message in buffer
                eprintln!("Connection closed with partial data in buffer");
                return Err("Connection closed unexpectedly".into());
            }
        }

        // Try to deserialize messages from the buffer
        loop {
            let buf_slice = buffer.chunk();
            if buf_slice.is_empty() {
                break; // No more data in buffer for now
            }

            // Attempt direct deserialization from the slice
            match de::from_bytes::<NreplMsg>(buf_slice) {
                Ok(msg) => {
                    // Estimate consumed size by deserializing into a generic bencode value
                    // This is a heuristic and might not be perfectly accurate for all bencode forms
                    // We need a better way to know how many bytes were consumed.
                    // `from_bytes` consumes the whole slice or errors.
                    // Workaround: Try parsing a generic Value first to estimate size.
                    let consumed = match de::from_bytes::<BencodeValue>(buf_slice) {
                        Ok(value) => {
                            // This is tricky. `serde_bencode` doesn't easily tells us bytes read.
                            // We'll *assume* successful parsing means the message fit within the buffer.
                            // Let's try to serialize the parsed msg back to estimate size.
                            // THIS IS VERY INEFFICIENT AND A HACK.
                            match ser::to_bytes(&msg) {
                                Ok(bytes) => bytes.len(),
                                Err(_) => buf_slice.len() // Fallback: consume whole buffer on error
                            }
                        },
                        Err(_) => buf_slice.len() // Fallback: consume whole buffer on error
                    };

                    println!("Received msg: {:?} (consumed ~{} bytes)", msg, consumed);
                    buffer.advance(consumed); // Consume the estimated message bytes from buffer

                    // --- Message Handling Logic ---
                    let mut sessions_guard = sessions.lock().await;
                    let session_id_for_op = msg.session.as_ref().or(current_session_id.as_ref());

                    match msg.op.as_str() {
                        "clone" => {
                            let new_session_id = uuid::Uuid::new_v4().to_string();
                            // Use .cloned() on the Option<Arc<...>> directly
                            let parent_context_opt = session_id_for_op.and_then(|sid| sessions_guard.get(sid).cloned());

                            let new_context = if let Some(parent_ctx_arc) = parent_context_opt {
                                let parent_guard = parent_ctx_arc.lock().await;
                                Arc::new(Mutex::new(parent_guard.clone()))
                            } else {
                                Arc::new(Mutex::new(IndexMap::new()))
                            };

                            sessions_guard.insert(new_session_id.clone(), new_context);
                            drop(sessions_guard);

                            current_session_id = Some(new_session_id.clone());

                            let response = NreplResponse {
                                id: msg.id.as_deref(),
                                session: None, // Not used in clone response
                                new_session: Some(&new_session_id), // Use the new field
                                status: vec!["done"],
                                value: None,
                                ex: None,
                            };
                            let resp_bytes = ser::to_bytes(&response)?;
                            writer.write_all(&resp_bytes).await?;
                            writer.flush().await?;
                            println!("Sent clone response: {:?}", response);
                        }
                        "eval" => {
                            if msg.code.is_none() {
                                eprintln!("Eval request received without code");
                                // TODO: Implement proper error response
                                drop(sessions_guard);
                                continue; // Should ideally break inner loop or handle error state better
                            }
                            let code = msg.code.as_ref().unwrap();

                            let session_id = match session_id_for_op {
                                Some(id) => id.clone(),
                                None => {
                                    let new_id = uuid::Uuid::new_v4().to_string();
                                    sessions_guard.insert(new_id.clone(), Arc::new(Mutex::new(IndexMap::new())));
                                    current_session_id = Some(new_id.clone());
                                    new_id
                                }
                            };

                            let context_arc = sessions_guard.get(&session_id).cloned();
                            drop(sessions_guard); // Release lock on session store IMPORTANT

                            if let Some(ctx_arc) = context_arc {
                                let mut context_guard = ctx_arc.lock().await;
                                match evaluate_form(code, &mut context_guard) {
                                    Ok(value) => {
                                        let response = NreplResponse {
                                            id: msg.id.as_deref(),
                                            session: Some(&session_id),
                                            new_session: None, // Not used in eval response
                                            status: vec!["done"],
                                            value: Some(format!("{:?}", value)),
                                            ex: None,
                                        };
                                        let resp_bytes = ser::to_bytes(&response)?;
                                        writer.write_all(&resp_bytes).await?;
                                        println!("Sent eval response: {:?}", response);
                                    }
                                    Err(e) => {
                                        let response = NreplResponse {
                                            id: msg.id.as_deref(),
                                            session: Some(&session_id),
                                            new_session: None,
                                            status: vec!["error", "eval-error"],
                                            value: None,
                                            ex: Some(e.to_string()),
                                        };
                                        let resp_bytes = ser::to_bytes(&response)?;
                                        writer.write_all(&resp_bytes).await?;
                                        println!("Sent eval error response: {:?}", response);
                                    }
                                }
                            } else {
                                eprintln!("Error: Session ID '{}' context arc not found after unlocking store.", session_id);
                                // TODO: Send error response
                            }
                        }
                        "describe" => {
                             drop(sessions_guard);
                             let description = NreplResponse {
                                 id: msg.id.as_deref(),
                                 session: session_id_for_op.map(|s| s.as_str()),
                                 new_session: None,
                                 status: vec!["done"],
                                 value: Some(format!("{{\"ops\":{{\"eval\":{{}},\"clone\":{{}},\"describe\":{{}}}},\"versions\":{{\"garden\":\"{}\",\"nrepl\":\"{}\"}}}}",
                                     env!("CARGO_PKG_VERSION"), "0.x (basic)")),
                                 ex: None,
                             };
                             let resp_bytes = ser::to_bytes(&description)?;
                             writer.write_all(&resp_bytes).await?;
                             println!("Sent describe response: {:?}", description);
                        }
                        _ => {
                            drop(sessions_guard);
                            eprintln!("Unhandled op: {}", msg.op);
                            let response = NreplResponse {
                                id: msg.id.as_deref(),
                                session: session_id_for_op.map(|s| s.as_str()),
                                new_session: None,
                                status: vec!["error", "unknown-op"],
                                value: None,
                                ex: Some(format!("Unknown op: {}", msg.op)),
                            };
                            let resp_bytes = ser::to_bytes(&response)?;
                            writer.write_all(&resp_bytes).await?;
                            println!("Sent unknown-op response: {:?}", response);
                        }
                    }
                    writer.flush().await?;
                }
                Err(e) => {
                    // Check if the error is due to insufficient data by checking the source
                    let mut is_eof = false;
                    if let Some(source) = e.source() {
                        if let Some(io_err) = source.downcast_ref::<io::Error>() {
                            if io_err.kind() == ErrorKind::UnexpectedEof {
                                is_eof = true;
                            }
                        }
                    }

                    if is_eof {
                        // Not enough data in the buffer yet, break inner loop and wait for more
                         break;
                    } else {
                        // It's a persistent Bencode error or different IO error
                        eprintln!("Bencode decode error: {}", e);
                        // Simple error recovery: clear buffer. Might lose data.
                        buffer.clear();
                        // TODO: Send nREPL error response if possible (need msg ID)
                        break; // Break inner loop on error
                    }
                }
            }
        } // End inner loop (message processing from buffer)
    } // End outer loop (reading from socket)

    if let Some(sid) = current_session_id {
        println!("Cleaning up session: {}", sid);
        sessions.lock().await.remove(&sid);
    }

    Ok(())
}
