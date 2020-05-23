use crate::types::{ChangeElement, ChangeType};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::io::{stdin, stdout, Stdout};
use std::path::Path;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use tui::backend::TermionBackend;
use tui::layout::Alignment;
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, Borders, Paragraph, Text};
use tui::Terminal;

pub fn display_file_history(path: &Path, conn: &Connection) -> Result<()> {
    let mut changes_query = conn.prepare(
        "SELECT change_elements, created_at FROM changes \
    JOIN documents ON changes.document_id = documents.id \
    WHERE documents.canonical_path == ?1\
    ORDER BY created_at DESC \
    LIMIT 100",
    )?;
    let changes = changes_query
        .query_map(params![path.canonicalize()?.to_str().unwrap()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    let stdin = stdin();
    let mut file_states = Vec::new();
    for change in changes {
        let (elem_str, created_at): (String, chrono::DateTime<Utc>) = change?;
        let elements = serde_json::from_str::<Vec<ChangeElement>>(&elem_str)?;
        let file = elements
            .into_iter()
            .filter_map(|elem| {
                if let ChangeType::Same = elem.type_ {
                    Some(elem.content)
                } else {
                    None
                }
            })
            .collect::<Vec<String>>()
            .join("");
        file_states.push((file, created_at));
    }
    let mut index: usize = 0;
    let mut scroll_index: u16 = 0;
    let stdout = stdout().into_raw_mode().unwrap();
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    draw_file(
        &mut terminal,
        &file_states[index].0,
        &file_states[index].1,
        scroll_index,
    )?;
    for event in stdin.keys() {
        draw_file(
            &mut terminal,
            &file_states[index].0,
            &file_states[index].1,
            scroll_index,
        )?;
        match event? {
            Key::Left => {
                index = (index + 1) % file_states.len();
            }
            Key::Right => {
                index = index.saturating_sub(1);
            }
            Key::Char('\n') => {
                scroll_index += 1;
                scroll_index %= u16::MAX;
            }
            Key::Up => {
                scroll_index = scroll_index.saturating_sub(1);
            }
            Key::Char(' ') | Key::Down => {
                scroll_index = (scroll_index + 16) % u16::MAX;
            }
            Key::Char('q') => {
                terminal.clear()?;
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

fn draw_file(
    terminal: &mut Terminal<TermionBackend<RawTerminal<Stdout>>>,
    file: &str,
    created_at: &chrono::DateTime<Utc>,
    scroll_index: u16,
) -> Result<()> {
    terminal.draw(|mut f| {
        let size = f.size();
        let text = [
            Text::raw(file),
            Text::Styled(
                format!("{:?}", created_at).into(),
                Style::default().fg(Color::Red),
            ),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .title_style(Style::default().modifier(Modifier::BOLD));
        let paragraph = Paragraph::new(text.iter())
            .block(block.clone().title("File"))
            .alignment(Alignment::Left)
            .scroll(scroll_index);
        f.render_widget(paragraph, size);
    })?;
    Ok(())
}
