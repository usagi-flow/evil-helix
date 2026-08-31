use std::io;
use std::time::Duration;

use anyhow::{bail, ensure};
use crossterm::event::{Event, KeyEvent};
use helix_term::{application::Application, args::Args, config::Config};
use helix_view::{document::Mode, input::parse_macro};
use tempfile::NamedTempFile;
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::wrappers::UnboundedReceiverStream;

type Input = io::Result<Event>;

pub struct EditorSession {
    app: Application,
    input_tx: UnboundedSender<Input>,
    input_rx: UnboundedReceiverStream<Input>,
    _file: NamedTempFile,
}

#[allow(dead_code)]
impl EditorSession {
    pub async fn open() -> anyhow::Result<Self> {
        Self::open_with_contents("").await
    }

    pub async fn open_with_contents(contents: &str) -> anyhow::Result<Self> {
        let file = NamedTempFile::new()?;
        std::fs::write(file.path(), contents)?;

        let mut args = Args::default();
        args.files
            .insert(file.path().to_path_buf(), vec![Default::default()]);

        let mut config = Config {
            theme: Some("base16_default".to_owned()),
            ..Config::default()
        };
        config.editor.lsp.enable = false;
        config.editor.editor_config = false;
        // Speed up `wait_until_idle`: each wait lasts one `idle_timeout`.
        // Must stay comfortably above the 33ms redraw debounce so `Redraw`
        // always wins the race against `IdleTimer` (stale screen otherwise).
        config.editor.idle_timeout = Duration::from_millis(100);

        let app = Application::new(args, config, helix_core::config::default_lang_loader())?;
        let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut session = Self {
            app,
            input_tx,
            input_rx: UnboundedReceiverStream::new(input_rx),
            _file: file,
        };

        session.send(Event::FocusGained)?;
        session.wait_until_idle().await?;
        Ok(session)
    }

    pub async fn type_keys(&mut self, sequence: &str) -> anyhow::Result<()> {
        for key in parse_macro(sequence)? {
            self.send(Event::Key(KeyEvent::from(key)))?;
        }
        self.wait_until_idle().await
    }

    pub fn text(&self) -> String {
        let (_, doc) = helix_view::current_ref!(self.app.editor);
        doc.text().to_string()
    }

    pub fn assert_mode(&self, expected: Mode) {
        assert_eq!(self.app.editor.mode(), expected, "unexpected editor mode");

        let label = match expected {
            Mode::Normal => "NOR",
            Mode::Insert => "INS",
            Mode::Select => "VIS",
        };
        let status = self.screen_row(self.status_row());
        assert!(
            status.starts_with(&format!(" {label} ")),
            "status line did not start with the expected mode label\n\n{}",
            self.screen_dump(),
        );
    }

    pub fn screen_cell(&self, x: u16, y: u16) -> &str {
        &self
            .app
            .screen()
            .get(x, y)
            .unwrap_or_else(|| panic!("screen position ({x}, {y}) is out of bounds"))
            .symbol
    }

    pub fn screen_row(&self, y: u16) -> String {
        let screen = self.app.screen();
        assert!(
            y >= screen.area.top() && y < screen.area.bottom(),
            "screen row {y} is out of bounds"
        );
        (screen.area.left()..screen.area.right())
            .map(|x| screen.get(x, y).unwrap().symbol.as_str())
            .collect()
    }

    pub fn screen_width(&self) -> u16 {
        self.app.screen().area.width
    }

    pub fn status_row(&self) -> u16 {
        self.app.screen().area.bottom() - 2
    }

    pub fn message_row(&self) -> u16 {
        self.app.screen().area.bottom() - 1
    }

    pub async fn close(mut self) -> anyhow::Result<()> {
        let errors = self.app.close().await;
        if !errors.is_empty() {
            bail!("errors while closing test application: {errors:?}");
        }
        Ok(())
    }

    fn send(&self, event: Event) -> anyhow::Result<()> {
        self.input_tx.send(Ok(event))?;
        Ok(())
    }

    async fn wait_until_idle(&mut self) -> anyhow::Result<()> {
        ensure!(
            self.app.event_loop_until_idle(&mut self.input_rx).await,
            "test application exited unexpectedly\n\n{}",
            self.screen_dump(),
        );
        Ok(())
    }

    fn screen_dump(&self) -> String {
        let area = self.app.screen().area;
        (area.top()..area.bottom())
            .map(|y| self.screen_row(y))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
