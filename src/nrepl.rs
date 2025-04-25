use crate::{evaluate_form, Value, Error}; // Assuming evaluate_form exists in main.rs or lib.rs
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_bencode::{de, ser, value::Value as BencodeValue}; // Import BencodeValue for length heuristic
use serde_bencode::Error as BencodeError;
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
    // TODO: Remove this once session handling is robust via clone/init message
    let conn_id = uuid::Uuid::new_v4().to_string();
    let mut current_session_id: Option<String> = None; // Stores the *active* session ID for this connection


    loop {
        let bytes_read = reader.read_buf(&mut buffer).await?;
        if bytes_read == 0 {
            // Connection closed cleanly by peer
            if buffer.is_empty() {
                break; // Clean exit
            } else {
                // Connection closed with partial message in buffer
                eprintln!("Connection closed with partial data in buffer");
                // Depending on protocol requirements, might try to parse remaining buffer once more
                // Or just return an error. Let's return error for now.
                return Err("Connection closed with partial data".into());
            }
        }

        // Try to deserialize messages from the buffer
        loop {
            let buf_slice = buffer.chunk(); // Get a slice of the current data
            if buf_slice.is_empty() {
                break; // No more data in buffer for now
            }

            // Use a reader adapter for the slice
            let mut slice_reader = buf_slice; // &[u8] implements Read
            let initial_len = slice_reader.len();

            // Attempt to deserialize one message using the Deserializer directly
            let mut deserializer = de::Deserializer::new(&mut slice_reader); // Create the deserializer
            match NreplMsg::deserialize(&mut deserializer) { // Deserialize directly
                Ok(msg) => {
                    // Calculate how many bytes were actually consumed from the slice_reader
                    let consumed = initial_len - slice_reader.len();
                    println!("Received msg: {:?} (consumed {} bytes)", msg, consumed);

                    // --- Message Handling Logic (moved inside success block) ---
                    let mut sessions_guard = sessions.lock().await;
                    // Determine the session ID to use for this operation
                    // 1. Use the session field from the message if present.
                    // 2. Otherwise, use the session ID established for this connection (e.g., via a previous 'clone').
                    let session_id_for_op = msg.session.as_ref().or(current_session_id.as_ref());

                    // Clone is special: it *creates* the connection's session ID if needed
                    if msg.op == "clone" {
                        let new_session_id = uuid::Uuid::new_v4().to_string();
                        println!("Cloning session. Old context: {:?}, New ID: {}", session_id_for_op, new_session_id);

                        // Clone the context if a parent session exists
                        let new_context = match session_id_for_op.and_then(|sid| sessions_guard.get(sid)) {
                             Some(parent_ctx_arc) => {
                                 let parent_guard = parent_ctx_arc.lock().await;
                                 println!("Cloning context from existing session: {}", session_id_for_op.unwrap());
                                 Arc::new(Mutex::new(parent_guard.clone())) // Clone the inner IndexMap
                             }
                             None => {
                                 println!("No parent session found for clone, creating new empty context.");
                                 Arc::new(Mutex::new(IndexMap::new())) // Create a fresh context
                             }
                         };

                        sessions_guard.insert(new_session_id.clone(), new_context);
                        drop(sessions_guard); // Release SessionStore lock

                        // Associate this *connection* with the newly created session
                        current_session_id = Some(new_session_id.clone());
                        println!("Connection now associated with session: {}", new_session_id);

                        let response = NreplResponse {
                            id: msg.id.as_deref(),
                            session: None, // Per nREPL spec for clone response
                            new_session: Some(current_session_id.as_deref().unwrap()), // Send the ID back
                            status: vec!["done"],
                            value: None,
                            ex: None,
                        };
                        let resp_bytes = ser::to_bytes(&response)?;
                        writer.write_all(&resp_bytes).await?;
                        println!("Sent clone response: {:?}", response);

                    } else if let Some(sid) = session_id_for_op {
                        // Handle ops requiring an existing session ('eval', 'describe', etc.)
                         let sid_clone = sid.to_string(); // Clone sid for use after dropping guard
                         let context_arc_opt = sessions_guard.get(&sid_clone).cloned(); // Clone Arc if found
                         drop(sessions_guard); // Release SessionStore lock

                         if let Some(ctx_arc) = context_arc_opt {
                             match msg.op.as_str() {
                                 "eval" => {
                                     if let Some(code) = msg.code.as_ref() {
                                         let mut context_guard = ctx_arc.lock().await;
                                         match evaluate_form(code, &mut context_guard).await {
                                             Ok(value) => {
                                                 let response = NreplResponse {
                                                     id: msg.id.as_deref(),
                                                     session: Some(&sid_clone),
                                                     new_session: None,
                                                     status: vec!["done"],
                                                     value: Some(format!("{:?}", value)), // TODO: Better value serialization
                                                     ex: None,
                                                 };
                                                 let resp_bytes = ser::to_bytes(&response)?;
                                                 writer.write_all(&resp_bytes).await?;
                                                 println!("Sent eval response: {:?}", response);
                                             }
                                             Err(e) => {
                                                 let response = NreplResponse {
                                                     id: msg.id.as_deref(),
                                                     session: Some(&sid_clone),
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
                                         eprintln!("Eval request received without code for session {}", sid_clone);
                                         // Send error response: missing code
                                          let response = NreplResponse {
                                              id: msg.id.as_deref(),
                                              session: Some(&sid_clone),
                                              new_session: None,
                                              status: vec!["error", "eval-error", "no-code"],
                                              value: None,
                                              ex: Some("No :code provided for eval".to_string()),
                                          };
                                          let resp_bytes = ser::to_bytes(&response)?;
                                          writer.write_all(&resp_bytes).await?;
                                     }
                                 }
                                 "describe" => {
                                     let description_val = format!(
                                         r#"{{"ops":{{"clone":{{}},"describe":{{}},"eval":{{}}}},"versions":{{"garden":"{}","nrepl":"0.x"}}}}"#,
                                         env!("CARGO_PKG_VERSION")
                                     );
                                     let response = NreplResponse {
                                         id: msg.id.as_deref(),
                                         session: Some(&sid_clone),
                                         new_session: None,
                                         status: vec!["done"],
                                         value: Some(description_val),
                                         ex: None,
                                     };
                                     let resp_bytes = ser::to_bytes(&response)?;
                                     writer.write_all(&resp_bytes).await?;
                                     println!("Sent describe response: {:?}", response);
                                 }
                                 // Add other session-aware ops here
                                 _ => {
                                     eprintln!("Unhandled op '{}' for session {}", msg.op, sid_clone);
                                     let response = NreplResponse {
                                         id: msg.id.as_deref(),
                                         session: Some(&sid_clone),
                                         new_session: None,
                                         status: vec!["error", "unknown-op"],
                                         value: None,
                                         ex: Some(format!("Unknown op: {}", msg.op)),
                                     };
                                     let resp_bytes = ser::to_bytes(&response)?;
                                     writer.write_all(&resp_bytes).await?;
                                 }
                             }
                         } else {
                             // Session ID was provided in message or connection state, but not found in store
                             // This might happen if a client sends messages for a session closed by the server
                             eprintln!("Error: Session ID '{}' not found in store.", sid_clone);
                             let response = NreplResponse {
                                 id: msg.id.as_deref(),
                                 session: Some(&sid_clone), // Echo back the problematic session
                                 new_session: None,
                                 status: vec!["error", "session-error", "unknown-session"],
                                 value: None,
                                 ex: Some(format!("Unknown session: {}", sid_clone)),
                             };
                             let resp_bytes = ser::to_bytes(&response)?;
                             writer.write_all(&resp_bytes).await?;
                         }

                    } else {
                        // Op received that requires a session ('eval', 'describe'), but none exists for this connection
                        // and none was provided in the message.
                        // According to nREPL practice, we should probably send an error.
                         drop(sessions_guard); // Ensure lock is dropped if not already
                         eprintln!("Error: Op '{}' requires a session, but none is active for this connection.", msg.op);
                         let response = NreplResponse {
                             id: msg.id.as_deref(),
                             session: None, // No session to report
                             new_session: None,
                             status: vec!["error", "session-error", "no-session"],
                             value: None,
                             ex: Some(format!("Op '{}' requires an active session.", msg.op)),
                         };
                         let resp_bytes = ser::to_bytes(&response)?;
                         writer.write_all(&resp_bytes).await?;
                    }

                    // Common cleanup after handling a message
                    writer.flush().await?;
                    // Advance the buffer past the message we just processed
                    buffer.advance(consumed);

                } // End Ok(msg)
                Err(e) => {
                    // Check if the error is due to insufficient data (EOF)
                    match e {
                         BencodeError::EndOfStream => {
                            // Not enough data in the buffer to complete a message.
                            // Break the inner loop and wait for more data from the socket.
                            // println!("Partial message, waiting for more data...");
                            break;
                        }
                        _ => {
                            // A real bencode parsing error occurred.
                            eprintln!("Bencode deserialization error: {}", e);
                            eprintln!("Buffer contents (partial): {:?}", buffer.chunk());
                            // Close the connection as we can't reliably recover the stream state.
                            return Err(format!("Bencode deserialization error: {}", e).into());
                        }
                    }
                } // End Err(e)
            } // End match on NreplMsg::deserialize
        } // End inner loop (message processing from buffer)
    } // End outer loop (reading from socket)

    // Clean up the session associated with *this connection* when it closes
    if let Some(sid) = current_session_id {
        println!("Connection closed, cleaning up session: {}", sid);
        sessions.lock().await.remove(&sid);
        println!("Session {} removed.", sid);
    } else {
         println!("Connection closed, no active session to clean up.");
    }

    Ok(())
}
