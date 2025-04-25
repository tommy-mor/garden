use crate::{evaluate_form, Value, Error}; // Assuming evaluate_form exists in main.rs or lib.rs
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_bencode::{de, ser}; // Corrected import for bencode ser/de
use serde_bytes::ByteBuf;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex}, // Using std Mutex for simplicity, could switch to tokio::sync::Mutex
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc, // For potential inter-task communication if needed later
};

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
    status: Vec<&'a str>, // e.g., ["done"] or ["error", "eval-error"]
    value: Option<String>, // Value needs serialization - simple String for now
    ex: Option<String>, // Exception/Error message
    // Add other response fields (ns, etc.)
}


// Represents the state for each connected nREPL session
type SessionContext = Arc<Mutex<IndexMap<String, Value>>>;
// Stores all active sessions
type SessionStore = Arc<Mutex<HashMap<String, SessionContext>>>;


pub async fn start_server() -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?; // Bind to localhost, random port
    let addr = listener.local_addr()?;
    println!("nREPL server listening on {}", addr);

    // Write .nrepl-port file
    write_nrepl_port_file(addr)?;

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
    let mut buf_reader = BufReader::new(reader);

    // We need a way to associate this connection with a session.
    // For now, let's assume a default session per connection or manage via 'clone' op.
    // A simple unique ID for this connection might serve as a temporary session ID.
    let conn_id = uuid::Uuid::new_v4().to_string(); // Requires `uuid` crate
    let mut current_session_id: Option<String> = None;


    loop {
        // nREPL uses bencode. We need to read bencoded messages.
        // Reading bencode streams isn't trivial with just BufReader lines.
        // We'll need a more robust way to delimit and parse bencode messages.
        // Placeholder: Read line by line (INCORRECT for bencode, just for structure)
        let mut line = String::new();
        match buf_reader.read_line(&mut line).await {
            Ok(0) => break, // Connection closed
            Ok(_) => {
                // Trim whitespace
                let trimmed_line = line.trim();
                if trimmed_line.is_empty() {
                    continue;
                }

                 // !!! --- THIS IS A HUGE SIMPLIFICATION --- !!!
                 // Actual implementation requires a bencode parser that can read from the stream.
                 // serde_bencode::from_reader might work, but needs careful handling of message boundaries.
                 // For now, let's pretend the line IS the bencode msg content for structure.
                println!("Received (raw): {}", trimmed_line);

                // Attempt to decode as bencode (will likely fail often with line reader)
                let msg: NreplMsg = match de::from_str::<NreplMsg>(trimmed_line) {
                     Ok(m) => m,
                     Err(e) => {
                         eprintln!("Bencode decode error: {}", e);
                         // Send an error response? Requires knowing message ID.
                         // Skipping proper error response for now.
                         continue;
                     }
                 };

                println!("Received msg: {:?}", msg);

                // --- Message Handling Logic ---
                let session_id_for_op = msg.session.as_ref().or(current_session_id.as_ref());

                 match msg.op.as_str() {
                     "clone" => {
                        let new_session_id = uuid::Uuid::new_v4().to_string(); // Requires `uuid` crate
                        let parent_context = session_id_for_op.and_then(|sid| sessions.lock().unwrap().get(sid).cloned());
                        let new_context = parent_context.map_or_else(
                             || Arc::new(Mutex::new(IndexMap::new())), // New empty context
                             |ctx_arc| Arc::new(Mutex::new(ctx_arc.lock().unwrap().clone())) // Cloned context
                        );

                        sessions.lock().unwrap().insert(new_session_id.clone(), new_context);
                        current_session_id = Some(new_session_id.clone()); // Set current session for this handler

                        let response = NreplResponse {
                             id: msg.id.as_deref(),
                             session: Some(&new_session_id),
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
                             // Send error response: no code
                             continue;
                         }
                         let code = msg.code.as_ref().unwrap();

                         // Get or create session context
                        let session_id = match session_id_for_op {
                            Some(id) => id.clone(),
                            None => {
                                // If no session specified and none active, create one? Or error?
                                // Let's create one implicitly for now.
                                let new_id = uuid::Uuid::new_v4().to_string();
                                sessions.lock().unwrap().insert(new_id.clone(), Arc::new(Mutex::new(IndexMap::new())));
                                current_session_id = Some(new_id.clone()); // Track it for subsequent ops
                                new_id
                            }
                        };

                         let context_arc = sessions.lock().unwrap().get(&session_id).cloned();

                         if let Some(ctx_arc) = context_arc {
                            let mut context_guard = ctx_arc.lock().unwrap(); // Lock the context for evaluation
                            // !!! Need evaluate_form function !!!
                            match evaluate_form(code, &mut context_guard) {
                                Ok(value) => {
                                    let response = NreplResponse {
                                        id: msg.id.as_deref(),
                                        session: Some(&session_id),
                                        status: vec!["done"],
                                        value: Some(format!("{:?}", value)), // Simple debug format for now
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
                              // Session ID provided but not found? Internal error.
                             eprintln!("Error: Session ID '{}' not found in store.", session_id);
                              // Send error response
                         }
                     }
                     "describe" => {
                         // Respond with basic info about available ops, etc.
                         // Hardcoded for now.
                         let description = NreplResponse { // Using same struct, maybe needs dedicated one
                             id: msg.id.as_deref(),
                             session: session_id_for_op.map(|s| s.as_str()),
                             status: vec!["done"],
                             value: Some(format!("{{\"ops\":{{\"eval\":{{}},\"clone\":{{}},\"describe\":{{}}}},\"versions\":{{\"garden\":\"{}\",\"nrepl\":\"{}\"}}}}",
                                env!("CARGO_PKG_VERSION"), "0.x (basic)")), // Provide some basic info
                             ex: None,
                         };
                         let resp_bytes = ser::to_bytes(&description)?;
                         writer.write_all(&resp_bytes).await?;
                         println!("Sent describe response: {:?}", description);
                     }
                     // Handle other ops like "load-file", "interrupt", "lookup", etc.
                     _ => {
                         eprintln!("Unhandled op: {}", msg.op);
                          // Send 'unknown-op' error response
                         let response = NreplResponse {
                             id: msg.id.as_deref(),
                             session: session_id_for_op.map(|s| s.as_str()),
                             status: vec!["error", "unknown-op"],
                             value: None,
                             ex: Some(format!("Unknown op: {}", msg.op)),
                         };
                         let resp_bytes = ser::to_bytes(&response)?;
                         writer.write_all(&resp_bytes).await?;
                         println!("Sent unknown-op response: {:?}", response);
                     }
                 }

                 writer.flush().await?; // Ensure response is sent
            }
            Err(e) => {
                // Handle read errors (e.g., connection reset)
                eprintln!("Error reading from client: {}", e);
                break;
            }
        }
    }

    // Cleanup: Remove the session associated with this connection if it was implicitly created?
    // Or rely on client explicitly closing sessions? nREPL spec needed.
    if let Some(sid) = current_session_id {
        println!("Cleaning up session: {}", sid);
        sessions.lock().unwrap().remove(&sid);
    }

    Ok(())
}


// We need a way to evaluate a single form string
// This likely involves parsing the string and then calling the existing `eval` function
fn evaluate_form_placeholder(code: &str, context: &mut IndexMap<String, Value>) -> Result<Value, Error> {
     // 1. Parse the code string into one or more ExprAst nodes
     // 2. Evaluate each node, updating the context
     // 3. Return the result of the *last* evaluated node
     // This is a placeholder - the actual implementation needs the parser from main.rs
    println!("Evaluating form (placeholder): {}", code);
    // Example: parse(code).and_then(|nodes| eval(&nodes[0], context)) // Simplified
    Err(Error::EvalError("evaluate_form not fully implemented yet".to_string()))
}
