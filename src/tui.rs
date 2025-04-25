use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use indexmap::IndexMap;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph}, // Start with basic widgets
    Frame, Terminal,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher, event::{ModifyKind, EventKind}, Config};
use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    sync::mpsc, // Use std::sync::mpsc for channels
    thread,
    time::Duration,
    fs, // Import fs for canonicalize
};

// Import necessary items from main.rs (adjust path if needed)
use crate::{Value, evaluate_file};

/// Structure to hold the application's state
pub struct App {
    file_path: PathBuf,
    context: IndexMap<String, Value>,
    last_error: Option<String>,
    should_quit: bool,
}

impl App {
    fn new(file_path: PathBuf) -> Self {
        App {
            file_path,
            context: IndexMap::new(),
            last_error: None,
            should_quit: false,
        }
    }

    /// Initial evaluation of the file
    fn initial_evaluate(&mut self) {
        match evaluate_file(&self.file_path) {
            Ok((ctx, _)) => {
                self.context = ctx;
                self.last_error = None;
            }
            Err(e) => {
                self.context.clear(); // Clear context on error
                self.last_error = Some(format!("Error: {}", e));
            }
        }
    }

    /// Re-evaluate the file, updating context and error state
    fn re_evaluate(&mut self) {
        // Similar to initial_evaluate, potentially add logic for diffs later
        self.initial_evaluate();
    }
}

/// Enum to represent messages sent from the watcher thread
#[derive(Debug)]
enum WatcherMessage {
    FileModified,
}

/// Main function to run the TUI application
pub fn run(file_to_watch: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // --- Resolve Absolute Path --- 
    // Canonicalize the path to ensure it's absolute for reliable comparison
    let absolute_path = fs::canonicalize(file_to_watch)
        .map_err(|e| format!("Failed to find or canonicalize watched file '{}': {}", file_to_watch.display(), e))?;
    println!("DEBUG: Watching absolute path: {}", absolute_path.display()); // Debug
    // --- End Resolve Path --- 

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state using the absolute path
    let mut app = App::new(absolute_path.clone());
    app.initial_evaluate(); // Perform the first evaluation

    // --- File Watcher Setup ---
    let (tx, rx) = mpsc::channel::<WatcherMessage>();
    // Use the absolute path for the watcher setup as well
    let watched_path = absolute_path; // No need to clone here if app took ownership via clone()

    let watcher_thread = thread::spawn(move || {
        let tx = tx.clone();
        let path_for_closure = watched_path.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if event.kind.is_modify() {
                            if let EventKind::Modify(data) = event.kind {
                                if let ModifyKind::Data(data) = data {
                                    let _ = tx.send(WatcherMessage::FileModified);
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Watcher Error: {:?}", e),
                }
            },
            Config::default(),
        ).expect("Failed to create file watcher");

        // Use the (absolute) watched_path here
        watcher.watch(&watched_path, RecursiveMode::NonRecursive)
             .expect("Failed to start watching file directly");
        eprintln!("DEBUG: Watching file directly: {}", watched_path.display());

        // Keep the watcher thread alive...
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    });
    // --- End File Watcher Setup ---

    // Pass the receiver to the app loop
    let res = run_app(&mut terminal, app, rx);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Optional: Consider how to gracefully stop the watcher thread if needed
    // watcher_thread.join()... (might require signalling mechanism)

    if let Err(err) = res {
        eprintln!("TUI Error: {:?}", err);
        return Err(err.into());
    }

    Ok(())
}

/// Main application loop
fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
    watcher_rx: mpsc::Receiver<WatcherMessage>, // Add receiver argument
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        // --- Event Handling ---

        // 1. Check for file watcher messages (non-blocking)
        match watcher_rx.try_recv() {
            Ok(WatcherMessage::FileModified) => {
                app.re_evaluate(); // Re-run evaluation logic
            }
            Err(mpsc::TryRecvError::Empty) => {},
            Err(mpsc::TryRecvError::Disconnected) => {
                // Watcher thread likely panicked, set an error state?
                app.last_error = Some("File watcher disconnected!".to_string());
                // Consider quitting or logging
            }
        }

        // 2. Check for keyboard input (with timeout)
        if crossterm::event::poll(Duration::from_millis(100))? { // Shorter timeout
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Char('r') => app.re_evaluate(), // Manual re-evaluation
                    _ => {}
                }
            }
            // Handle other events like resize if needed
        }
        // --- End Event Handling ---

        if app.should_quit {
            return Ok(());
        }
    }
}

/// Function to draw the UI widgets
fn ui(f: &mut Frame, app: &App) {
    // Basic layout: one big block for now
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3), // Top: File info
                Constraint::Min(0),    // Middle: Values
                Constraint::Length(1), // Bottom: Status/Error
            ]
            .as_ref(),
        )
        .split(f.size());

    // Top: File Info
    let file_text = format!("Watching: {}", app.file_path.display());
    let file_paragraph = Paragraph::new(file_text).block(Block::default().borders(Borders::ALL).title("File"));
    f.render_widget(file_paragraph, chunks[0]);

    // Middle: Values (Placeholder)
    // Convert IndexMap to a simple string for now
    let mut value_text = String::new();
     if app.context.is_empty() && app.last_error.is_none() {
         value_text.push_str("No definitions found or file is empty.");
     } else {
         for (key, val) in &app.context {
             // Simple debug format, truncate later if needed
             value_text.push_str(&format!("{:<15} = {:?}\n", key, val));
         }
     }

    let values_paragraph = Paragraph::new(value_text)
        .block(Block::default().borders(Borders::ALL).title("Garden - Live Expression Values"));
    f.render_widget(values_paragraph, chunks[1]);


    // Bottom: Status/Error Bar
    let status_text = match &app.last_error {
        Some(err) => err.clone(),
        None => "OK | Press 'q' to quit".to_string(),
    };
     let status_style = match &app.last_error {
         Some(_) => Style::default().fg(Color::Red),
         None => Style::default(),
     };
    let status_paragraph = Paragraph::new(status_text).style(status_style);
    f.render_widget(status_paragraph, chunks[2]);
} 