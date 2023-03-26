use std::{
    borrow::Cow,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use helix_core::{
    movement::move_next_word_end,
    movement::{is_word_boundary, move_prev_word_start},
    Range, Selection, Transaction,
};
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

#[derive(Eq, PartialEq)]
enum Motion {
    PrevWordStart,
    NextWordEnd,
    LineStart,
    LineEnd,
}

impl TryFrom<char> for Motion {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            'w' => Ok(Motion::NextWordEnd),
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

    fn get_selection(cx: &mut Context) -> Option<Selection> {
        let (view, doc) = current!(cx.editor);

        let mut selection: Option<Selection> = None;

        match cx.editor.mode {
            helix_view::document::Mode::Normal => {
                // TODO: even in Normal mode, there can be a selection -> should it be disregarded,
                // or can we assume this shouldn't happen in evil mode?
                // -> In Vim, this wouldn't be possible, so for now, let's assume this case doesn't exist and correct
                // this elsewhere later on if necessary.

                // TODO: recognize motion keys like w and b
                // TODO: see https://github.com/helix-editor/helix/blob/823eaad1a118e8865a6400afc22d37e060783d45/helix-term/src/ui/editor.rs#L1331-L1372

                if let Some(motion) = Self::context().motion.as_ref() {
                    // A motion was specified: Select accordingly
                    Self::trace(
                        cx,
                        "Motion keys are not supported yet, performing line-based selection",
                    );
                    // TODO
                    selection = Some(Self::get_word_based_selection(cx, motion));
                } else {
                    // No motion was specified: Perform a line-based selection
                    selection = Some(Self::get_line_based_selection(cx));
                }
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

    fn get_character_based_selection(cx: &mut Context) -> Selection {
        let (view, doc) = current!(cx.editor);

        // For each cursor, select one or more characters forward or backward according
        // to the count in the evil context and the motion respectively.
        return doc.selection(view.id).clone().transform(|range| {
            // TODO: it'd be nice if the get_*_selection() functions were independent of the
            // cx.count vs context().count logic
            // If we use an evil command which uses the hotkey twice (dd, yy, ...), we need to use the evil context,
            // but if we use an immediate command (x, ...), we need the regular context...
            //let mut count = Self::context().count.unwrap_or(1);
            let mut count = cx.count.map(|non_zero| non_zero.get()).unwrap_or(1);

            let anchor = range.anchor.min(range.head);
            let head = range.anchor.max(range.head);

            if head > anchor {
                count -= 1;
            }

            let head = head + count;

            Range::new(anchor, head)
        });
    }

    fn get_word_based_selection(cx: &mut Context, motion: &Motion) -> Selection {
        let (view, doc) = current!(cx.editor);

        // For each cursor, select one or more words forward or backward according
        // to the count in the evil context and the motion respectively.
        return doc.selection(view.id).clone().transform(|range| {
            let forward = match motion {
                Motion::NextWordEnd => true,
                Motion::PrevWordStart => false,
                _ => panic!("Invalid motion"),
            };

            let text = doc.text().slice(..);

            let char_current = text.char(range.anchor);
            let char_previous = match range.anchor > 0 {
                true => Some(text.char(range.anchor - 1)),
                false => None,
            };
            let char_next = match range.anchor < text.len_chars() - 1 {
                true => Some(text.char(range.anchor + 1)),
                false => None,
            };

            let mut count = Self::context().count.unwrap_or(1);

            // Handle the special case where we're on the last character of a word and moving forwards,
            // or on the first character of a word and moving backwards.
            // Note that these special cases do not apply when we're between words.

            if forward
                && char_next.is_some()
                && !char_current.is_whitespace()
                && is_word_boundary(char_current, char_next.unwrap())
            {
                count -= 1;
            }

            if !forward
                && char_previous.is_some()
                && !char_current.is_whitespace()
                && is_word_boundary(char_current, char_previous.unwrap())
            {
                count -= 1;
            }

            // If we're selecting backwards, inverse the anchor and the head
            // to ensure the current character is selected as well.
            let anchor = match forward {
                true => range.anchor.min(range.head),
                false => range.anchor.max(range.head),
            };

            let range = match forward {
                true => move_next_word_end(text, range, count),
                false => move_prev_word_start(text, range, count),
            };

            Range::new(anchor, range.head)
        });
    }

    fn get_line_based_selection(cx: &mut Context) -> Selection {
        let (view, doc) = current!(cx.editor);

        let lines_to_select = Self::context().count.unwrap_or(1);

        let text = doc.text();
        let extend = Extend::Below;

        // Process a number of lines: first create a temporary selection of the text to be processed
        return doc.selection(view.id).clone().transform(|range| {
            let (start_line, end_line) = range.line_range(text.slice(..));

            let start: usize = text.line_to_char(start_line);
            let end: usize = text.line_to_char((end_line + lines_to_select).min(text.len_lines()));

            // Extend to previous/next line if current line is selected
            let (anchor, head) = if range.from() == start && range.to() == end {
                match extend {
                    Extend::Above => (end, text.line_to_char(start_line.saturating_sub(1))),
                    Extend::Below => (
                        start,
                        text.line_to_char((end_line + lines_to_select).min(text.len_lines())),
                    ),
                }
            } else {
                (start, end)
            };

            Range::new(anchor, head)
        });
    }

    fn yank_selection(cx: &mut Context, selection: &Selection, _set_status_message: bool) {
        let (_view, doc) = current!(cx.editor);

        let text = doc.text().slice(..);

        let values: Vec<String> = selection.fragments(text).map(Cow::into_owned).collect();
        let _selections = values.len();

        cx.editor
            .registers
            .write(cx.register.unwrap_or('"'), values);
    }

    fn delete_selection(cx: &mut Context, selection: &Selection, _set_status_message: bool) {
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
            }
            _ => {
                // A command was initiated, but another one was executed: cancel the command.
                Self::trace(cx, "Command interrupted");
                Self::context_mut().reset();
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
                Self::evil_command(cx, active_command, set_mode);
            } else {
                // A command was initiated, but another command was initiated.
                Self::context_mut().reset();
                // TODO: proceed with initiating the other command?
            }
            return;
        }

        // Is the command being executed with a motion key?
        if let Some(motion) = e.char().and_then(|c| Motion::try_from(c).ok()) {
            Self::context_mut().motion = Some(motion);
            // TODO; a motion key should immediately execute the command
            Self::evil_command(cx, active_command, set_mode);
            return;
        }

        // Is the command receiving a new/increased count?
        // TODO: better way to parse a char?
        if let Some(value) = e
            .char()
            .and_then(|c| usize::from_str_radix(c.to_string().as_str(), 10).ok())
        {
            let mut evil_context = Self::context_mut();
            evil_context.count = Some(evil_context.count.map(|c| c * 10).unwrap_or(0) + value);

            log::info!(
                "Key callback: Increasing count to {}",
                evil_context.count.unwrap()
            );

            // TODO: doesn't seem to work
            cx.on_next_key_callback = Some(Box::new(move |cx: &mut Context, e: KeyEvent| {
                Self::evil_command_key_callback(cx, e);
            }));

            return;
        }

        // A command was initiated, but an unrelated key was pressed: cancel the command.
        Self::trace(cx, "Command interrupted");
        Self::context_mut().reset();
    }

    pub fn yank(cx: &mut Context) {
        Self::evil_command(cx, Command::Yank, None);
    }

    /// Delete/change one or more lines, words, or delete the selected text.
    /// If the operation is `Operation::Change`, change to insert mode after deletion.
    /// Example: *dd or d*d, cw, cc, C, ...
    pub fn delete(cx: &mut Context, op: Operation) {
        Self::evil_command(
            cx,
            Command::Delete,
            Some(match op {
                Operation::Delete => Mode::Normal,
                Operation::Change => Mode::Insert,
            }),
        );
    }

    /// Delete a single character or the selection immediately,
    /// and return to normal mode if the select mode was active.
    pub fn delete_immediate(cx: &mut Context) {
        let selection = Self::get_character_based_selection(cx);
        Self::delete_selection(cx, &selection, false);
        exit_select_mode(cx);
    }
}
