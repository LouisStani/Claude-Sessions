//! Replays captured terminal output and prints the final screen as text.
//! Usage: cargo run --example replay -- <capture> <columns> <lines>

use std::{env, fs, process::ExitCode};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;

struct Size(usize, usize);

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.1
    }
    fn screen_lines(&self) -> usize {
        self.1
    }
    fn columns(&self) -> usize {
        self.0
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let [_, path, columns, lines] = args.as_slice() else {
        eprintln!("usage: replay <capture> <columns> <lines>");
        return ExitCode::from(2);
    };
    let (Ok(columns), Ok(lines)) = (columns.parse(), lines.parse()) else {
        eprintln!("replay: columns and lines must be numbers");
        return ExitCode::from(2);
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("replay: {path}: {error}");
            return ExitCode::FAILURE;
        },
    };
    let mut term = Term::new(Config::default(), &Size(columns, lines), VoidListener);
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, &bytes);
    let mut rows = vec![String::new(); lines];
    for indexed in term.renderable_content().display_iter {
        if let Ok(row) = usize::try_from(indexed.point.line.0) {
            rows[row].push(indexed.cell.c);
        }
    }
    for row in rows {
        println!("{}", row.trim_end());
    }
    ExitCode::SUCCESS
}
