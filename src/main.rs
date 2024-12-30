use std::fs::File;
use std::io::{self, BufRead, BufReader};
use tui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, List, ListItem},
    Terminal,
    text::{Spans, Span},
    style::{Color, Style, Modifier},
};
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen},
    execute,
};
use regex::Regex;
use clap::Parser;

#[derive(Debug)]
struct LogEntry {
    raw_content: String,
    styled_spans: Vec<Spans<'static>>,  // スタイル情報を保持
}

struct App {
    logs: Vec<LogEntry>,
    search_pattern: String,
    is_searching: bool,
    search_regex: Option<Regex>,
    saved_patterns: Vec<String>,
    scroll: usize,
}

impl App {
    fn new(logs: Vec<LogEntry>) -> Self {
        Self {
            logs,
            search_pattern: String::new(),
            is_searching: false,
            search_regex: None,
            saved_patterns: Vec::new(),
            scroll: 0,
        }
    }

    fn filtered_logs(&self) -> Vec<&LogEntry> {
        match &self.search_regex {
            Some(regex) => self.logs
                .iter()
                .filter(|log| regex.is_match(&log.raw_content))
                .collect(),
            None => self.logs.iter().collect(),
        }
    }

    fn load_pattern(&mut self, key: u8) {
        let idx = key as usize;
        if key == 0 {
            self.search_pattern.clear();
            self.search_regex = None;
        } else if idx <= self.saved_patterns.len() {
            let pattern = &self.saved_patterns[idx - 1];
            self.search_pattern = pattern.clone();
            if let Ok(regex) = Regex::new(&self.search_pattern) {
                self.search_regex = Some(regex);
            }
        }
    }

    fn get_status_text(&self) -> String {
        let mut status = String::from("Searches: ");
        for (i, pattern) in self.saved_patterns.iter().enumerate() {
            status.push_str(&format!("[{}:'{}'] ", i + 1, pattern));
        }
        status
    }

    fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    fn scroll_down(&mut self, height: usize) {
        let max_scroll = self.filtered_logs().len().saturating_sub(height);
        if self.scroll < max_scroll {
            self.scroll += 1;
        }
    }

    fn page_up(&mut self, height: usize) {
        self.scroll = self.scroll.saturating_sub(height);
    }

    fn page_down(&mut self, height: usize) {
        let max_scroll = self.filtered_logs().len().saturating_sub(height);
        self.scroll = (self.scroll + height).min(max_scroll);
    }

    fn confirm_search(&mut self) {
        self.is_searching = false;
        if !self.search_pattern.is_empty() {
            if let Ok(regex) = Regex::new(&self.search_pattern) {
                self.search_regex = Some(regex);
                if !self.saved_patterns.contains(&self.search_pattern) {
                    if self.saved_patterns.len() >= 10 {
                        self.saved_patterns.remove(0);
                    }
                    self.saved_patterns.push(self.search_pattern.clone());
                }
            }
        }
    }
}

#[derive(Parser)]
#[command(name = "log-viewer")]
struct Args {
    #[arg(help = "Path to the log file")]
    log_file: String,
}

fn main() -> Result<(), io::Error> {
    let args = Args::parse();
    
    let file = File::open(&args.log_file)?;
    let reader = BufReader::new(file);
    let logs: Vec<LogEntry> = reader
        .lines()
        .filter_map(|line| line.ok())
        .filter_map(|line| parse_log_line(&line))
        .collect();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(logs);

    terminal.draw(|f| {
        let size = f.size();
        draw_logs(&app, f, size);
    })?;

    loop {
        let size = terminal.size()?;
        let height = size.height as usize;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => app.scroll_up(),
                KeyCode::Down => app.scroll_down(height),
                KeyCode::PageUp => app.page_up(height),
                KeyCode::PageDown => app.page_down(height),
                KeyCode::Char('q') if !app.is_searching => break,
                KeyCode::Char('/') if !app.is_searching => {
                    app.is_searching = true;
                },
                KeyCode::Enter if app.is_searching => {
                    app.confirm_search();
                },
                KeyCode::Char(c) if !app.is_searching && c.is_ascii_digit() => {
                    app.load_pattern(c as u8 - b'0');
                },
                KeyCode::Esc if app.is_searching => {
                    app.is_searching = false;
                    app.search_pattern.clear();
                    app.search_regex = None;
                },
                KeyCode::Backspace if app.is_searching => {
                    app.search_pattern.pop();
                },
                KeyCode::Char(c) => {
                    if key.code == KeyCode::Char(' ') && !app.is_searching {
                        app.page_down(height);
                    } else if app.is_searching {
                        app.search_pattern.push(c);
                    }
                },
                _ => {}
            }
        }

        terminal.draw(|f| {
            let size = f.size();
            draw_logs(&app, f, size);
        })?;
    }

    disable_raw_mode()?;
    Ok(())
}

fn draw_logs(app: &App, f: &mut tui::Frame<CrosstermBackend<io::Stdout>>, size: tui::layout::Rect) {
    let height = size.height as usize - 2;
    
    let items: Vec<ListItem> = app.filtered_logs()
        .iter()
        .skip(app.scroll)
        .take(height)
        .map(|log| ListItem::new(log.styled_spans.clone()))
        .chain(std::iter::repeat(ListItem::new(vec![
            Spans::from(vec![Span::raw("")])
        ])).take(height))
        .take(height)
        .collect();

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::TOP)
            .title(
                if app.is_searching {
                    format!("Search: {}_", app.search_pattern)
                } else {
                    format!("Log Viewer - {}", app.get_status_text())
                }
            ));

    f.render_widget(list, size);
}

fn parse_log_line(line: &str) -> Option<LogEntry> {
    if line.trim().is_empty() {
        return None;
    }

    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_style = Style::default();
    let mut chars = line.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '\x1B' && chars.peek() == Some(&'[') {
            chars.next();
            
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }
            
            let mut code = String::new();
            while let Some(c) = chars.next() {
                if c.is_ascii_alphabetic() {
                    break;
                }
                code.push(c);
            }
            
            current_style = match code.as_str() {
                "31" => Style::default().fg(Color::Red),
                "32" => Style::default().fg(Color::Green),
                "33" => Style::default().fg(Color::Yellow),
                "34" => Style::default().fg(Color::Blue),
                "35" => Style::default().fg(Color::Magenta),
                "36" => Style::default().fg(Color::Cyan),
                "1" => Style::default().add_modifier(Modifier::BOLD),
                "0" => Style::default(),
                _ => current_style,
            };
        } else {
            current_text.push(c);
        }
    }
    
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    Some(LogEntry {
        raw_content: line.to_string(),
        styled_spans: vec![Spans::from(spans)],
    })
}