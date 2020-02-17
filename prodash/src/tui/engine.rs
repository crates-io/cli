use crate::{tree::Root, tui::draw, tui::ticker};

use futures::{channel::mpsc, SinkExt, StreamExt};
use std::{io, time::Duration};
use termion::{event::Key, input::TermRead, raw::IntoRawMode, screen::AlternateScreen};
use tui::{backend::TermionBackend, layout::Rect};
use tui_react::Terminal;

/// Configure the terminal user interface
#[derive(Clone)]
pub struct TuiOptions {
    /// The initial title to show for the whole window.
    ///
    /// Can be adjusted later by sending `Event::SetTitle(…)`
    /// into the event stream, see see [`tui::render_with_input(…events)`](./fn.render_with_input.html) function.
    pub title: String,
    /// The amount of frames to draw per second. If below 1.0, it determines the amount of seconds between the frame.
    ///
    /// *e.g.* 1.0/4.0 is one frame every 4 seconds.
    pub frames_per_second: f32,
    /// The initial window size.
    ///
    /// If unset, it will be retrieved from the current terminal.
    pub window_size: Option<Rect>,
}

impl Default for TuiOptions {
    fn default() -> Self {
        TuiOptions {
            title: "Progress Dashboard".into(),
            frames_per_second: 10.0,
            window_size: None,
        }
    }
}

/// A line as used in [`Event::SetInformation`](./enum.Event.html#variant.SetInformation)
pub enum Line {
    /// Set a title with the given text
    Title(String),
    /// Set a line of text with the given content
    Text(String),
}

/// An event to be sent in the [`tui::render_with_input(…events)`](./fn.render_with_input.html) stream.
///
/// This way, the TUI can be instructed to draw frames or change the information to be displayed.
pub enum Event {
    /// Draw a frame
    Tick,
    /// Send any key - can be used to simulate user input, and is typically generated by the TUI's own input loop.
    Input(Key),
    /// Change the size of the window to the given rectangle.
    ///
    /// Useful to embed the TUI into other terminal user interfaces that can resize dynamically.
    SetWindowSize(Rect),
    /// Set the title of the progress dashboard
    SetTitle(String),
    /// Provide a list of titles and lines to populate the side bar on the right.
    SetInformation(Vec<Line>),
}

/// Returns a future that draws the terminal user interface indefinitely.
///
/// * `progress` is the progress tree whose information to visualize.
///    It will usually be changing constantly while the TUI holds it.
/// * `options` are configuring the TUI.
/// * `events` is a stream of `Event`s which manipulate the TUI while it is running
///
/// Failure may occour if there is no terminal to draw into.
pub fn render_with_input(
    progress: Root,
    options: TuiOptions,
    events: impl futures::Stream<Item = Event> + Send,
) -> Result<impl std::future::Future<Output = ()>, std::io::Error> {
    let TuiOptions {
        title,
        frames_per_second,
        window_size,
    } = options;
    let mut terminal = {
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend)?
    };

    let duration_per_frame = Duration::from_secs_f32(1.0 / frames_per_second);
    let (mut key_send, key_receive) = mpsc::channel::<Key>(1);

    // This brings blocking key-handling into the async world
    std::thread::spawn(move || -> Result<(), io::Error> {
        for key in io::stdin().keys() {
            let key = key?;
            futures::executor::block_on(key_send.send(key)).ok();
        }
        Ok(())
    });

    let render_fut = async move {
        let mut state = draw::State {
            title,
            duration_per_frame,
            ..draw::State::default()
        };
        let mut entries = Vec::with_capacity(progress.num_tasks());
        let mut messages = Vec::with_capacity(progress.messages_capacity());
        let mut events = futures::stream::select_all(vec![
            ticker(duration_per_frame).map(|_| Event::Tick).boxed(),
            key_receive.map(|key| Event::Input(key)).boxed(),
            events.boxed(),
        ]);

        while let Some(event) = events.next().await {
            let mut skip_redraw = false;
            match event {
                Event::Tick => {}
                Event::Input(key) => match key {
                    Key::Esc | Key::Char('q') | Key::Ctrl('c') | Key::Ctrl('[') => {
                        break;
                    }
                    Key::Char('`') => state.hide_messages = !state.hide_messages,
                    Key::Char('~') => state.messages_fullscreen = !state.messages_fullscreen,
                    Key::Char('J') => state.message_offset = state.message_offset.saturating_add(1),
                    Key::Char('D') => {
                        state.message_offset = state.message_offset.saturating_add(10)
                    }
                    Key::Char('j') => state.task_offset = state.task_offset.saturating_add(1),
                    Key::Char('d') => state.task_offset = state.task_offset.saturating_add(10),
                    Key::Char('K') => state.message_offset = state.message_offset.saturating_sub(1),
                    Key::Char('U') => {
                        state.message_offset = state.message_offset.saturating_sub(10)
                    }
                    Key::Char('k') => state.task_offset = state.task_offset.saturating_sub(1),
                    Key::Char('u') => state.task_offset = state.task_offset.saturating_sub(10),
                    Key::Char('[') => state.hide_info = !state.hide_info,
                    Key::Char('{') => state.maximize_info = !state.maximize_info,
                    _ => skip_redraw = true,
                },
                Event::SetWindowSize(bound) => state.user_provided_window_size = Some(bound),
                Event::SetTitle(title) => state.title = title,
                Event::SetInformation(info) => state.information = info,
            }
            if !skip_redraw {
                let terminal_window_size = terminal.pre_render().expect("pre-render to work");
                let window_size = state
                    .user_provided_window_size
                    .or(window_size)
                    .unwrap_or(terminal_window_size);
                let buf = terminal.current_buffer_mut();
                progress.sorted_snapshot(&mut entries);
                progress.copy_messages(&mut messages);

                draw::all(&mut state, &entries, &messages, window_size, buf);
                terminal.post_render().expect("post render to work");
            }
        }
    };
    Ok(render_fut)
}

/// An easy-to-use version of `render_with_input(…)` that does not allow state manipulation via an event stream.
pub fn render(
    progress: Root,
    config: TuiOptions,
) -> Result<impl std::future::Future<Output = ()>, std::io::Error> {
    return render_with_input(progress, config, futures::stream::pending());
}