use crate::types::{ChangeElement, ChangeType};
use anyhow::Result;
use clipboard::{ClipboardContext, ClipboardProvider};
use rusqlite::{params, Connection};
use std::borrow::Cow;
use std::convert::TryInto;
use std::fs;
use std::io::{stdin, stdout, Stdout};
use std::path::Path;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, Gauge, Paragraph, Text};
use tui::{Frame, Terminal};

struct FileState {
    // Number of
    snippet_count: usize,
    elements: Vec<ChangeElement>,
}

fn get_file_states(path: &Path, conn: &Connection) -> Result<Vec<FileState>> {
    let mut changes_query = conn.prepare(
        "SELECT change_elements FROM changes \
    JOIN documents ON changes.document_id = documents.id \
    WHERE documents.canonical_path == ?1\
    ORDER BY created_at DESC \
    LIMIT 100",
    )?;
    let changes = changes_query
        .query_map(params![path.canonicalize()?.to_str().unwrap()], |row| {
            Ok(row.get(0)?)
        })?;
    let mut file_states = Vec::new();
    for change in changes {
        let elem_str: String = change?;
        let elements = serde_json::from_str::<Vec<ChangeElement>>(&elem_str)?;
        let snippet_count = get_snippets_count(&elements);
        file_states.push(FileState {
            snippet_count,
            elements,
        });
    }
    Ok(file_states)
}

// Count all non-same elements, i.e. add or remove
fn get_snippets_count(elements: &Vec<ChangeElement>) -> usize {
    let mut snippets_count = 0;
    for elem in elements {
        if let ChangeType::Same = elem.type_ {
        } else {
            snippets_count += 1;
        }
    }
    snippets_count
}

fn copy_snippet(elements: &Vec<ChangeElement>, snippet_index: usize) -> Result<()> {
    let mut ctx = ClipboardContext::new().expect("Unable to open clipboard");
    let mut index = 0;
    for elem in elements {
        if let ChangeType::Same = elem.type_ {
        } else {
            if snippet_index == index {
                ctx.set_contents(elem.content.to_string())
                    .expect("Unable to copy to clipboard");
                return Ok(());
            }
            index += 1;
        }
    }
    Ok(())
}

pub fn display_file_history(path: &Path, conn: &Connection) -> Result<()> {
    let stdin = stdin();
    let file_states = get_file_states(path, conn)?;
    if file_states.len() == 0 {
        eprintln!("No history available for file");
        return Ok(());
    }

    let mut is_help_screen = true;
    let mut file_index: usize = 0;
    let mut scroll_index: u16 = 0;
    let mut selected_snippet: usize = 0;
    let percent: u16 = (file_index * 100 / file_states.len()).try_into()?;
    let mut header = path.to_str().unwrap_or("File").to_string();
    let stdout = stdout().into_raw_mode().unwrap();
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    draw_file(
        &mut terminal,
        &file_states[file_index].elements,
        scroll_index,
        selected_snippet,
        100 - percent,
        &header,
        is_help_screen,
    )?;
    for event in stdin.keys() {
        header = path.to_str().unwrap_or("File").to_string();
        match event? {
            Key::Char('h') => is_help_screen = !is_help_screen,
            Key::Left => {
                file_index = (file_index + 1) % file_states.len();
                selected_snippet = 0;
            }
            Key::Right => {
                file_index = file_index.saturating_sub(1);
                selected_snippet = 0;
            }
            Key::Alt('<') => {
                scroll_index = 0;
            }
            Key::Char('\n') => {
                scroll_index += 1;
                scroll_index %= u16::MAX;
            }
            Key::BackTab => {
                selected_snippet = selected_snippet.saturating_sub(1);
            }
            Key::Char('\t') => {
                selected_snippet = (selected_snippet + 1) % file_states[file_index].snippet_count
            }
            Key::Char('s') => {
                let file_text = get_file_text(&file_states[file_index].elements);
                fs::write(path, file_text)?;
                header = format!("{} (saved)", path.to_str().unwrap_or("File"));
            }
            Key::Up | Key::Ctrl('p') => {
                scroll_index = scroll_index.saturating_sub(1);
            }
            Key::Char(' ') => {
                scroll_index = (scroll_index + 16) % u16::MAX;
            }
            Key::Down | Key::Ctrl('n') => {
                scroll_index = (scroll_index + 1) & u16::MAX;
            }
            Key::Char('q') => {
                terminal.clear()?;
                break;
            }
            Key::Char('c') => {
                copy_snippet(&file_states[file_index].elements, selected_snippet)?;
                header = format!("{} --- Copied snippet", path.to_str().unwrap_or("File"));
            }
            _ => {}
        }
        let percent: u16 = (file_index * 100 / file_states.len()).try_into()?;
        draw_file(
            &mut terminal,
            &file_states[file_index].elements,
            scroll_index,
            selected_snippet,
            100 - percent,
            &header,
            is_help_screen,
        )?;
    }
    Ok(())
}

fn get_file_text(elements: &Vec<ChangeElement>) -> String {
    elements
        .iter()
        .filter_map(|elem| match elem.type_ {
            ChangeType::Add | ChangeType::Same => Some(elem.content.clone()),
            ChangeType::Remove => None,
        })
        .collect::<Vec<String>>()
        .join("")
}

fn draw_file(
    terminal: &mut Terminal<TermionBackend<RawTerminal<Stdout>>>,
    elements: &Vec<ChangeElement>,
    scroll_index: u16,
    selected_snippet: usize,
    percent: u16,
    header: &str,
    is_help_screen: bool,
) -> Result<()> {
    terminal.draw(|mut f| {
        let mut snippet_index = 0;
        let text = elements
            .into_iter()
            .map(|elem| {
                let inner_str: Cow<str> = match elem.type_ {
                    ChangeType::Same => format!("\n{}", elem.content.clone()).into(),
                    ChangeType::Add => format!("\n+ {}", elem.content).into(),
                    ChangeType::Remove => format!("\n- {}", elem.content).into(),
                };
                let styles = match elem.type_ {
                    ChangeType::Same => Style::default(),
                    _ => {
                        let styles = if snippet_index == selected_snippet {
                            Style::default().bg(Color::LightCyan)
                        } else {
                            Style::default()
                        };
                        snippet_index += 1;
                        styles
                    }
                };

                match elem.type_ {
                    ChangeType::Same => Text::Styled(inner_str, styles),
                    ChangeType::Add => Text::Styled(inner_str, styles.fg(Color::Green)),
                    ChangeType::Remove => Text::Styled(inner_str, styles.fg(Color::Red)),
                }
            })
            .collect::<Vec<Text>>();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Percentage(95), Constraint::Percentage(5)].as_ref())
            .split(f.size());

        draw_text(
            &mut f,
            text,
            is_help_screen,
            chunks[0],
            header,
            scroll_index,
        );
        let percent_str = format!("{}", percent);
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(&percent_str))
            .style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Black)
                    .modifier(Modifier::ITALIC),
            )
            .percent(percent);
        f.render_widget(gauge, chunks[1]);
    })?;
    Ok(())
}

fn draw_text<B: tui::backend::Backend>(
    f: &mut Frame<B>,
    text: Vec<Text>,
    is_help_screen: bool,
    area: Rect,
    header: &str,
    scroll_index: u16,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title_style(Style::default().modifier(Modifier::BOLD));
    let paragraph = Paragraph::new(text.iter())
        .block(block.title(header))
        .alignment(Alignment::Left)
        .scroll(scroll_index);
    if is_help_screen {
        let text = vec![Text::Raw(
            "h: Toggle help screen
<-: Go to previous version of file
->: Go to next version of file
↑ or C-p: Go up
↓ or C-n: Go down
<TAB>: Select next diff element
<SHIFT> <TAB>: Select previous element
<SPACE>: Go down faster
q: Quit
c: Copy selected snippet
s: Overwrite file with current version
M-<: Go to beginning of file"
                .into(),
        )];
        let block = Block::default()
            .borders(Borders::ALL)
            .title_style(Style::default().modifier(Modifier::BOLD))
            .title("Help");
        let help_screen = Paragraph::new(text.iter())
            .block(block)
            .alignment(Alignment::Left);
        let split_screen = Layout::default()
            //.direction(Dire)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area);
        f.render_widget(paragraph, split_screen[0]);
        f.render_widget(help_screen, split_screen[1]);
    } else {
        f.render_widget(paragraph, area);
    }
}
