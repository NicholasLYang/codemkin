use crate::types::{ChangeElement, ChangeType};
use anyhow::Result;
use rusqlite::{params, Connection};
use std::convert::TryInto;
use std::fs;
use std::io::{stdin, stdout, Stdout};
use std::path::Path;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, Gauge, Paragraph, Text};
use tui::Terminal;

pub fn display_file_history(path: &Path, conn: &Connection) -> Result<()> {
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
    let stdin = stdin();
    let mut file_states = Vec::new();
    for change in changes {
        let elem_str: String = change?;
        let elements = serde_json::from_str::<Vec<ChangeElement>>(&elem_str)?;
        file_states.push(elements);
    }
    if file_states.len() == 0 {
        eprintln!("No history available for file");
        return Ok(());
    }
    let mut index: usize = 0;
    let mut scroll_index: u16 = 0;
    let stdout = stdout().into_raw_mode().unwrap();
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    let percent: u16 = (index * 100 / file_states.len()).try_into()?;
    let mut header = path.to_str().unwrap_or("File").to_string();
    draw_file(
        &mut terminal,
        &file_states[index],
        scroll_index,
        100 - percent,
        &header,
    )?;
    for event in stdin.keys() {
        header = path.to_str().unwrap_or("File").to_string();
        match event? {
            Key::Left => {
                index = (index + 1) % file_states.len();
            }
            Key::Right => {
                index = index.saturating_sub(1);
            }
            Key::Alt('<') => {
                scroll_index = 0;
            }
            Key::Char('\n') => {
                scroll_index += 1;
                scroll_index %= u16::MAX;
            }
            Key::Char('s') => {
                let file_text = get_file_text(&file_states[index]);
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
            _ => {}
        }
        let percent: u16 = (index * 100 / file_states.len()).try_into()?;
        draw_file(
            &mut terminal,
            &file_states[index],
            scroll_index,
            100 - percent,
            &header,
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
    percent: u16,
    header: &str,
) -> Result<()> {
    terminal.draw(|mut f| {
        let text = elements
            .into_iter()
            .map(|elem| match elem.type_ {
                ChangeType::Same => Text::Raw(elem.content.clone().into()),
                ChangeType::Add => Text::Styled(
                    format!("\n+ {}", elem.content).into(),
                    Style::default().fg(Color::Green),
                ),
                ChangeType::Remove => Text::Styled(
                    format!("\n- {}", elem.content).into(),
                    Style::default().fg(Color::Red),
                ),
            })
            .collect::<Vec<Text>>();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Percentage(95), Constraint::Percentage(5)].as_ref())
            .split(f.size());
        let block = Block::default()
            .borders(Borders::ALL)
            .title_style(Style::default().modifier(Modifier::BOLD));
        let percent_str = format!("{}", percent);
        let paragraph = Paragraph::new(text.iter())
            .block(block.clone().title(header))
            .alignment(Alignment::Left)
            .scroll(scroll_index);
        f.render_widget(paragraph, chunks[0]);
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
