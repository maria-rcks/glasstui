use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use glasstui::app::App;
use glasstui::ui::{self, Renderer};
use std::io;
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    crossterm::execute!(io::stdout(), EnableMouseCapture)?;
    // ratatui::init installs a panic hook restoring the terminal; chain one
    // that also releases mouse capture so a panic doesn't leave the terminal
    // swallowing mouse input.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
        prev(info);
    }));

    let result = run(&mut terminal);

    let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    let start = Instant::now();
    let mut app = App::new();
    let mut renderer = Renderer::new();

    while !app.quit {
        terminal.draw(|frame| ui::draw(frame, &mut app, &mut renderer))?;

        if event::poll(Duration::from_millis(33))? {
            // Drain everything queued so fast drags stay smooth.
            loop {
                let now_ms = start.elapsed().as_millis() as u64;
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            app.quit = true;
                        } else {
                            app.handle_key(key.code);
                        }
                    }
                    Event::Mouse(m) => app.handle_mouse(m.kind, m.column, m.row, now_ms),
                    _ => {}
                }
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
    Ok(())
}
