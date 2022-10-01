use std::{
    borrow::Cow,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use helix_core::{Range, Selection, Transaction};
use helix_view::{document::Mode, input::KeyEvent};
use once_cell::sync::Lazy;

use crate::commands::{enter_insert_mode, exit_select_mode, Context, Extend, Operation};

use super::select_mode;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Command {
    Yank,
    Delete,
}

//impl Command {
//    pub fn from_key(e: KeyEvent) -> Option<Self> {
//        e.char().and_then(|char| match char {
//            'd' => Some(Command::Delete),
//            'y' => Some(Command::Yank),
//            _ => None,
//        })
//    }
//}

impl TryFrom<char> for Command {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            'd' => Ok(Command::Delete),
            'y' => Ok(Command::Yank),
            _ => Err(()),
        }
    }
}

enum Motion {
    PrevWordStart,
    NextWordStart,
    LineStart,
    LineEnd,
    Invalid,
}

impl TryFrom<char> for Motion {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            'w' => Ok(Motion::NextWordStart),
            'b' => Ok(Motion::PrevWordStart),
            '$' => Ok(Motion::LineEnd),
            '0' => Ok(Motion::LineStart),
            _ => Err(()),
        }
    }
}

struct EvilContext {
    command: Option<Command>,
    motion: Option<Motion>,
    count: Option<usize>,
    set_mode: Option<Mode>,
}

impl EvilContext {
    pub fn reset(&mut self) {
        self.command = None;
        self.motion = None;
        self.count = None;
        self.set_mode = None;
    }
}

static CONTEXT: Lazy<RwLock<EvilContext>> = Lazy::new(|| {
    RwLock::new(EvilContext {
        command: None,
        motion: None,
        count: None,
        set_mode: None,
    })
});

pub struct EvilCommands;

impl EvilCommands {
    fn trace<T>(cx: &mut Context, msg: T)
    where
        T: Into<Cow<'static, str>>,
    {
        cx.editor.set_status(msg);
    }

    pub fn is_enabled() -> bool {
        true
    }

    fn context() -> RwLockReadGuard<'static, EvilContext> {
        return CONTEXT.read().unwrap();
    }

    fn context_mut() -> RwLockWriteGuard<'static, EvilContext> {
        return CONTEXT.write().unwrap();
    }

    fn get_mode(cx: &mut Context) -> Mode {
        return cx.editor.mode();
    }

    pub fn prev_word_start(_cx: &mut Context) {}

    pub fn next_word_start(_cx: &mut Context) {}

    fn get_selection(cx: &mut Context) -> Option<Selection> {
        let (view, doc) = current!(cx.editor);

        let mut selection: Option<Selection> = None;

        match cx.editor.mode {
            helix_view::document::Mode::Normal => {
                // TODO: even in Normal mode, there can be a selection -> should it be disregarded,
                // or can we assume this shouldn't happen in evil mode?

                // TODO: recognize motion keys like w and b
                // TODO: see https://github.com/helix-editor/helix/blob/823eaad1a118e8865a6400afc22d37e060783d45/helix-term/src/ui/editor.rs#L1331-L1372

                // Process a number of lines: first create a temporary selection of the text to be processed
                let lines_to_select = Self::context().count.unwrap_or(1);

                let text = doc.text();
                let extend = Extend::Below;
                selection = Some(doc.selection(view.id).clone().transform(|range| {
                    let (start_line, end_line) = range.line_range(text.slice(..));

                    let start = text.line_to_char(start_line);
                    let end = text.line_to_char((end_line + lines_to_select).min(text.len_lines()));

                    // Extend to previous/next line if current line is selected
                    let (anchor, head) = if range.from() == start && range.to() == end {
                        match extend {
                            Extend::Above => (end, text.line_to_char(start_line.saturating_sub(1))),
                            Extend::Below => (
                                start,
                                text.line_to_char(
                                    (end_line + lines_to_select).min(text.len_lines()),
                                ),
                            ),
                        }
                    } else {
                        (start, end)
                    };

                    Range::new(anchor, head)
                }));
            }
            helix_view::document::Mode::Select => {
                // Yank the selected text
                selection = Some(doc.selection(view.id).clone());
            }
            helix_view::document::Mode::Insert => {
                log::debug!("Attempted to select while in insert mode");
            }
        }

        return selection;
    }

    fn yank_selection(cx: &mut Context, selection: &Selection, set_status_message: bool) {
        let (_view, doc) = current!(cx.editor);

        let registers = &mut cx.editor.registers;
        let register_name = cx.register.unwrap_or('"');
        let text = doc.text().slice(..);

        let values: Vec<String> = selection.fragments(text).map(Cow::into_owned).collect();
        let register = registers.get_mut(register_name);
        let selections = values.len();
        register.write(values);

        if set_status_message {
            let message;
            if selections == 1 {
                message = format!(
                    "Yanked {} selection to register {}",
                    selections,
                    cx.register.unwrap_or('"')
                );
            } else {
                message = format!(
                    "Yanked {} selections to register {}",
                    selections,
                    cx.register.unwrap_or('"')
                );
            }

            cx.editor.set_status(message);
        }
    }

    fn delete_selection(cx: &mut Context, selection: &Selection, set_status_message: bool) {
        let selection = Self::get_selection(cx);

        if let Some(selection) = selection {
            if cx.register != Some('_') {
                // first yank the selection
                Self::yank_selection(cx, &selection, false);
            };

            let (view, doc) = current!(cx.editor);
            let transaction = Transaction::change_by_selection(doc.text(), &selection, |range| {
                (range.from(), range.to(), None)
            });

            doc.apply(&transaction, view.id);
        }

        /*match op {
            Operation::Delete => {
                // exit select mode, if currently in select mode
                exit_select_mode(cx);
            }
            Operation::Change => {
                let (_view, doc) = current!(cx.editor);
                enter_insert_mode(doc);
            }
        }*/
    }

    fn evil_command(cx: &mut Context, requested_command: Command, set_mode: Option<Mode>) {
        let active_command;
        {
            active_command = Self::context().command;
        }

        match active_command {
            None => {
                // The command is being initiated
                {
                    let mut evil_context = Self::context_mut();
                    evil_context.command = Some(requested_command);
                    evil_context.count = cx.count.map(|c| c.get());
                    evil_context.set_mode = set_mode;
                }

                if Self::get_mode(cx) != Mode::Select {
                    cx.on_next_key_callback =
                        Some(Box::new(move |cx: &mut Context, e: KeyEvent| {
                            Self::evil_command_key_callback(cx, e);
                        }));

                    if let Some(count) = Self::context().count {
                        Self::trace(cx, format!("Command initiated with count {}", count));
                    } else {
                        Self::trace(cx, format!("Command initiated without count"));
                    }
                } else {
                    // We're in the select mode, execute the command immediately.
                    Self::evil_command(cx, requested_command, set_mode);
                }
            }
            Some(active_command) if active_command == requested_command => {
                // The command is being executed
                let selection = Self::get_selection(cx);

                if let Some(selection) = selection {
                    // TODO: use accessor to obtain the function
                    match active_command {
                        Command::Yank => {
                            Self::yank_selection(cx, &selection, true);
                        }
                        Command::Delete => {
                            Self::delete_selection(cx, &selection, true);
                        }
                    }
                }

                let set_mode = Self::context().set_mode;
                if let Some(mode) = set_mode {
                    match mode {
                        Mode::Normal => {
                            exit_select_mode(cx);
                        }
                        Mode::Insert => {
                            enter_insert_mode(cx);
                        }
                        Mode::Select => {
                            select_mode(cx);
                        }
                    }
                } else {
                    exit_select_mode(cx);
                }

                // The command was executed, reset the context.
                Self::context_mut().reset();

                //Self::trace(cx, "Command executed");
            }
            _ => {
                // A command was initiated, but another one was executed: cancel the command.
                Self::context_mut().reset();

                Self::trace(cx, "Command reset");
            }
        }
    }

    fn evil_command_key_callback(cx: &mut Context, e: KeyEvent) {
        let active_command;
        let set_mode;
        {
            let context = Self::context();
            active_command = context.command.unwrap();
            set_mode = context.set_mode;
        }

        // Is the command being executed?
        if let Some(command) = e.char().and_then(|c| Command::try_from(c).ok()) {
            // Assume this callback is called only if a command was initiated
            if command == active_command {
                Self::trace(cx, "Key callback: Executing command");
                Self::evil_command(cx, active_command, set_mode);
            } else {
                // A command was initiated, but another command was initiated.
                Self::trace(
                    cx,
                    "Key callback: Command interrupted due to another command",
                );
                Self::context_mut().reset();
                // TODO: proceed with initiating the other command?
            }
            return;
        }

        // Is the command being executed with a motion key?
        if let Some(motion) = e.char().and_then(|c| Motion::try_from(c).ok()) {
            Self::trace(cx, "Key callback: Motion key detected");
            Self::context_mut().motion = Some(motion);
            // TODO; a motion key should immediately execute the command
            return;
        }

        // Is the command receiving a new/increased count?
        // TODO: better way to parse a char?
        if let Some(value) = e
            .char()
            .and_then(|c| usize::from_str_radix(c.to_string().as_str(), 10).ok())
        {
            Self::trace(cx, "Key callback: Increasing count");
            let mut evil_context = Self::context_mut();
            evil_context.count = Some(evil_context.count.map(|c| c * 10).unwrap_or(0) + value);
            return;
        }

        // A command was initiated, but an illegal motion was used: cancel the command.
        Self::trace(cx, "Key callback: Command interrupted");
        Self::context_mut().reset();
    }

    pub fn yank(cx: &mut Context) {
        Self::evil_command(cx, Command::Yank, None);
    }

    /// Delete one or more lines, or delete the selected text.
    /// Default: *dd or d*d
    pub fn delete(cx: &mut Context, op: Operation) {
        Self::evil_command(
            cx,
            Command::Delete,
            Some(match op {
                Operation::Delete => Mode::Normal,
                Operation::Change => Mode::Insert,
            }),
        );
        /*let selection = Self::get_selection(cx);

        if let Some(selection) = selection {
            if cx.register != Some('_') {
                // first yank the selection
                Self::yank_selection(cx, &selection, false);
            };

            let (view, doc) = current!(cx.editor);
            let transaction = Transaction::change_by_selection(doc.text(), &selection, |range| {
                (range.from(), range.to(), None)
            });

            doc.apply(&transaction, view.id);
        }

        match op {
            Operation::Delete => {
                // exit select mode, if currently in select mode
                exit_select_mode(cx);
            }
            Operation::Change => {
                let (_view, doc) = current!(cx.editor);
                enter_insert_mode(doc);
            }
        }*/
    }

    pub fn delete_to_eol() {}

    /// Delete the character underneath/to the right of the cursor.
    /// Default: x
    pub fn delete_char() {}

    /// Delete the character left of the cursor.
    /// Default: X
    pub fn delete_char_left() {}

    /// Clear text and switch to insert mode.
    /// In normal mode, first wait for a character to indicate what should be deleted:
    /// - c: whole line
    /// - w: word
    /// In selection mode, clear the selected text.
    pub fn clear(_cx: &mut Context) {}
}
