use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use pyo3::{prelude::*, types::PyDict};
use ratatui::{
    backend::{CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};

fn main() -> Result<()> {
    // 1) Parse CLI args for .py file
    let script_path = std::env::args()
        .nth(1)
        .expect("Usage: garden_repl <script.py>");
    let script_path = PathBuf::from(script_path);

    // 2) Initialize UI
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 3) Initialize Python & run user script
    let (py_locals, py_globals) = run_python_script(&script_path)
        .context("Failed to run Python script")?;

    // We'll store the REPL input in a buffer
    let mut input_buffer = String::new();

    // 4) Main event loop
    loop {
        // 4a) Draw the UI
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // REPL input area
                    Constraint::Min(1),    // Python variable list
                ])
                .split(f.size());

            // REPL input box
            let input = Paragraph::new(input_buffer.as_ref())
                .block(Block::default().borders(Borders::ALL).title("REPL Input"));
            f.render_widget(input, chunks[0]);

            // Show variables in second area
            let locals_list = build_locals_list(&py_locals, &py_globals);
            let locals_widget = List::new(locals_list)
                .block(Block::default().borders(Borders::ALL).title("In-Scope Variables"));
            f.render_widget(locals_widget, chunks[1]);
        })?;

        // 4b) Handle user input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char(c) => {
                        // Add typed characters to input buffer
                        input_buffer.push(c);
                    }
                    KeyCode::Backspace => {
                        input_buffer.pop();
                    }
                    KeyCode::Enter => {
                        // On ENTER, evaluate the buffer in Python
                        let code = input_buffer.trim();
                        if !code.is_empty() {
                            evaluate_in_python(code, &py_locals, &py_globals)?;
                        }
                        input_buffer.clear();
                    }
                    KeyCode::Esc => {
                        // ESC to quit
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    // 5) Cleanup
    disable_raw_mode()?;
    let mut stdout = terminal.into_inner().into_raw_mode()?;
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}

/// Run the user’s Python script and return references to the Python locals/globals
fn run_python_script(script: &PathBuf) -> Result<(Py<PyDict>, Py<PyDict>)> {
    let code = std::fs::read_to_string(script)
        .with_context(|| format!("Unable to read Python script {:?}", script))?;

    let gil = Python::acquire_gil();
    let py = gil.python();

    // We'll create a new dict for globals, which also serves as locals at top-level
    let globals = PyDict::new(py);
    let locals = PyDict::new(py);

    // Insert builtins
    globals.set_item("__builtins__", py.import("builtins")?)?;

    // Execute the script
    py.run(&code, Some(globals), Some(locals))
        .with_context(|| format!("Failed to execute script {:?}", script))?;

    // We return references that are valid for this Python interpreter lifetime
    Ok((locals.into(), globals.into()))
}

/// Evaluate a line of code in the existing Python locals/globals
fn evaluate_in_python(code: &str, locals: &Py<PyDict>, globals: &Py<PyDict>) -> Result<()> {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let locals_ref = locals.as_ref(py);
    let globals_ref = globals.as_ref(py);

    // Evaluate the code. This is a simplistic approach (exec for statements)
    py.run(code, Some(globals_ref), Some(locals_ref))
        .with_context(|| format!("Failed to eval: {}", code))?;

    Ok(())
}

/// Build a list of variables from the Python local scope
fn build_locals_list(locals: &Py<PyDict>, globals: &Py<PyDict>) -> Vec<ListItem<'static>> {
    let gil = Python::acquire_gil();
    let py = gil.python();

    let mut items = vec![];

    // Combine locals & globals
    // This is naive; you might want to deduplicate keys, or skip private names, etc.
    let local_vars = locals.as_ref(py).items();
    for (k, v) in local_vars {
        let item_str = format!("{} = {:?}", k, v);
        items.push(ListItem::new(item_str));
    }
    let global_vars = globals.as_ref(py).items();
    for (k, v) in global_vars {
        let item_str = format!("{} = {:?}", k, v);
        items.push(ListItem::new(item_str));
    }

    items
}
