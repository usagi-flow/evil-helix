use std::{
    borrow::Cow,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use helix_core::movement::move_prev_word_start;
use helix_core::movement::{is_word_boundary, Direction};
use helix_core::{movement::move_next_word_end, Rope};
use helix_core::{Range, Selection, Transaction};
use helix_view::document::Mode;
use helix_view::input::KeyEvent;
use once_cell::sync::Lazy;

use crate::commands::{enter_insert_mode, exit_select_mode, Context, Extend, Operation};

use super::{select_mode, OnKeyCallbackKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Command {
    Yank,
    Delete,
    Change,
}

impl TryFrom<char> for Command {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            'c' => Ok(Command::Change),
            'd' => Ok(Command::Delete),
            'y' => Ok(Command::Yank),
            _ => Err(()),
        }
    }
}

#[derive(Eq, PartialEq)]
enum Modifier {
    InnerWord,
}

impl TryFrom<char> for Modifier {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            // :h object-select
            'i' => Ok(Self::InnerWord),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Motion {
    PrevWordStart,
    NextWordEnd,
    PrevLongWordStart,
    NextLongWordEnd,
    LineStart,
    LineEnd,
}

impl TryFrom<char> for Motion {
    type Error = ();

    fn try_from(value: char) -> Result<Self, Self::Error> {
        match value {
            'w' | 'e' => Ok(Self::NextWordEnd),
            'b' => Ok(Self::PrevWordStart),
            'W' | 'E' => Ok(Self::NextLongWordEnd),
            'B' => Ok(Self::PrevLongWordStart),
            '$' => Ok(Self::LineEnd),
            '0' => Ok(Self::LineStart),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub enum CollapseMode {
    Forward,
    Backward,
    ToAnchor,
    ToHead,
}

struct EvilContext {
    command: Option<Command>,
    motion: Option<Motion>,
    count: Option<usize>,
    modifiers: Vec<Modifier>,
    set_mode: Option<Mode>,
}

impl EvilContext {
    pub fn reset(&mut self) {
        self.command = None;
        self.motion = None;
        self.count = None;
        self.modifiers.clear();
        self.set_mode = None;
    }
}

static CONTEXT: Lazy<RwLock<EvilContext>> = Lazy::new(|| {
    RwLock::new(EvilContext {
        command: None,
        motion: None,
        count: None,
        modifiers: Vec::new(),
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

    /// Collapse selections such that the selections cover one character per cursor only.
    pub fn collapse_selections(cx: &mut Context, collapse_mode: CollapseMode) {
        let (view, doc) = current!(cx.editor);

        doc.set_selection(
            view.id,
            doc.selection(view.id).clone().transform(|mut range| {
                // TODO: when exiting insert mode after appending, we end up on the character _after_ the curson,
                // while vim returns to the character _before_ the cursor.

                match collapse_mode {
                    CollapseMode::Forward => {
                        let end = range.anchor.max(range.head);
                        range.anchor = 0.max(end.saturating_sub(1));
                        range.head = end;
                    }
                    CollapseMode::Backward => {
                        let start = range.anchor.min(range.head);
                        range.anchor = start;
                        range.head = start.saturating_add(1);
                    }
                    CollapseMode::ToAnchor => {
                        if range.head > range.anchor {
                            range.head = range.anchor.saturating_add(1);
                        } else {
                            range.head = 0.max(range.anchor.saturating_sub(1));
                        }
                    }
                    CollapseMode::ToHead => {
                        if range.head > range.anchor {
                            range.anchor = 0.max(range.head.saturating_sub(1));
                        } else {
                            range.anchor = range.head.saturating_add(1);
                        }
                    }
                }

                range
            }),
        );
    }

    fn context() -> RwLockReadGuard<'static, EvilContext> {
        return CONTEXT.read().unwrap();
    }

    fn context_mut() -> RwLockWriteGuard<'static, EvilContext> {
        return CONTEXT.write().unwrap();
    }

    fn get_mode(cx: &mut Context) -> Mode {
        cx.editor.mode()
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

                let has_inner_word_modifier =
                    Self::context().modifiers.contains(&Modifier::InnerWord);

                if let Some(motion) = Self::context().motion.as_ref() {
                    log::trace!("Calculating selection using motion: {:?}", motion);
                    // A motion was specified: Select accordingly
                    // TODO: handle other motion keys as well
                    selection = match motion {
                        Motion::PrevWordStart | Motion::NextWordEnd if has_inner_word_modifier => {
                            Self::get_bidirectional_word_based_selection(cx).ok()
                        }
                        Motion::PrevWordStart | Motion::NextWordEnd => {
                            Self::get_word_based_selection(cx, motion).ok()
                        }
                        Motion::PrevLongWordStart | Motion::NextLongWordEnd
                            if has_inner_word_modifier =>
                        {
                            // TODO: this doesn't support long words yet
                            Self::get_bidirectional_word_based_selection(cx).ok()
                        }
                        Motion::PrevLongWordStart | Motion::NextLongWordEnd => {
                            // TODO: this doesn't support long words yet
                            Self::get_word_based_selection(cx, motion).ok()
                        }
                        Motion::LineStart | Motion::LineEnd => {
                            Self::get_partial_line_based_selection(cx, motion).ok()
                        }
                    };
                } else {
                    // The inner word modifier isn't valid for a line-based selection
                    if !has_inner_word_modifier {
                        // No motion was specified: Perform a line-based selection
                        log::trace!("No motion was specified: Perform a line-based selection");

                        // If the command is a change command, do not include the final line break,
                        // to ensure an empty line is left in place.
                        selection = Some(Self::get_full_line_based_selection(
                            cx,
                            !Self::context()
                                .command
                                .is_some_and(|command| command == Command::Change),
                        ));
                    }
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

        selection
    }

    fn get_character_based_selection(cx: &mut Context) -> Selection {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);

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

            Range::new(text.len_chars().min(anchor), text.len_chars().min(head))
        });
    }

    fn get_bidirectional_word_based_selection(cx: &mut Context) -> Result<Selection, String> {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);

        Ok(doc.selection(view.id).clone().transform(|range| {
            let range = move_prev_word_start(text, range, 1);
            
            move_next_word_end(text, range, 1)
        }))
    }

    fn get_word_based_selection(cx: &mut Context, motion: &Motion) -> Result<Selection, String> {
        let (view, doc) = current!(cx.editor);
        let mut error: Option<String> = None;
        let text = doc.text().slice(..);

        // For each cursor, select one or more words forward or backward according
        // to the count in the evil context and the motion respectively.
        let selection = doc.selection(view.id).clone().transform(|range| {
            let forward = match motion {
                Motion::NextWordEnd => true,
                Motion::PrevWordStart => false,
                _ => {
                    error = Some("Unsupported motion".to_string());
                    return range;
                }
            };

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

            Range::new(
                text.len_chars().min(anchor),
                text.len_chars().min(range.head),
            )
        });

        if error.is_none() {
            Ok(selection)
        } else {
            Err(error.unwrap())
        }
    }

    fn get_partial_line_based_selection(
        cx: &mut Context,
        motion: &Motion,
    ) -> Result<Selection, String> {
        let (view, doc) = current!(cx.editor);

        let text = doc.text();

        // Process a number of lines: first create a temporary selection of the text to be processed
        let selection = doc.selection(view.id).clone().transform(|range| {
            let (start_line, end_line) = range.line_range(text.slice(..));

            let start: usize = text.line_to_char(start_line);
            let mut end: usize = text.line_to_char((end_line + 1).min(text.len_lines()));

            // Handle the edge case of finding the line end on the last line:
            // We normally have to keep the EOL char(s) from being selected,
            // but if there is no empty line at the end, we shouldn't skip characters.
            if end_line < text.len_lines() {
                end = end.saturating_sub(1); // TODO: we're removing LF, but what about multiple EOL characters?
            }

            match motion {
                Motion::LineStart => Range::new(start, range.anchor.max(range.head)),
                Motion::LineEnd => Range::new(range.anchor.min(range.head), end),
                _ => panic!("Unsupported motion"),
            }
        });

        Ok(selection)
    }

    fn get_full_line_based_selection(
        cx: &mut Context,
        include_final_line_break: bool,
    ) -> Selection {
        let (view, doc) = current!(cx.editor);

        let lines_to_select = Self::context().count.unwrap_or(1);

        let text = doc.text();
        let extend = Extend::Below;

        log::trace!("Calculating full line-based selection (lines to select: {}, extend below: {}, include final line break: {})", lines_to_select, match extend {
            Extend::Above => false,
            Extend::Below => true,
        }, include_final_line_break);

        // Process a number of lines: first create a temporary selection of the text to be processed
        return doc.selection(view.id).clone().transform(|range| {
            let (start_line, end_line) = range.line_range(text.slice(..));

            let start: usize = text.line_to_char(start_line);
            let end: usize = text.line_to_char((end_line + lines_to_select).min(text.len_lines()));

            // Extend to previous/next line if current line is selected
            let (mut anchor, mut head) = if range.from() == start && range.to() == end {
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

            // Strip the final line break if requested
            if !include_final_line_break {
                (anchor, head) = Self::strip_trailing_line_break(text, (anchor, head));
            }

            Range::new(anchor, head)
        });
    }

    fn strip_trailing_line_break(text: &Rope, range: (usize, usize)) -> (usize, usize) {
        let start = range.0.min(range.1);
        let mut end = range.0.max(range.1);
        let inversed = range.0 > range.1;

        // The end points to the next char, not to the last char which would be selected
        if end.saturating_sub(start) >= 2 && text.char(end - 1) == '\n' {
            end -= 1;

            // The line might end with CR & LF; in that case, strip CR as well
            if end.saturating_sub(start) >= 2 && text.char(end - 1) == '\r' {
                end -= 1;
            }
        }

        if !inversed {
            (start, end)
        } else {
            (end, start)
        }
    }

    fn yank_selection(cx: &mut Context, selection: &Selection, _set_status_message: bool) {
        let (_view, doc) = current!(cx.editor);

        let text = doc.text().slice(..);

        let values: Vec<String> = selection.fragments(text).map(Cow::into_owned).collect();
        let _selections = values.len();

        let _ = cx
            .editor
            .registers
            .write(cx.register.unwrap_or('"'), values);
    }

    fn delete_selection(cx: &mut Context, selection: &Selection, _set_status_message: bool) {
        if cx.register != Some('_') {
            // first yank the selection
            Self::yank_selection(cx, selection, false);
        };

        let (view, doc) = current!(cx.editor);
        let transaction = Transaction::change_by_selection(doc.text(), selection, |range| {
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
                    cx.on_next_key_callback = Some((
                        Box::new(move |cx: &mut Context, e: KeyEvent| {
                            Self::evil_command_key_callback(cx, e);
                        }),
                        OnKeyCallbackKind::PseudoPending,
                    ));
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
                        Command::Change | Command::Delete => {
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

        log::trace!("Key callback invoked, active command: {:?}", active_command);

        // Is the command being executed?
        if let Some(command) = e.char().and_then(|c| Command::try_from(c).ok()) {
            // Assume this callback is called only if a command was initiated
            if command == active_command {
                log::trace!("The active command is being executed: {:?}", active_command);
                Self::evil_command(cx, active_command, set_mode);
                return;
            } else {
                log::debug!(
                    "A command ({:?}) was active, but another command ({:?}) has been initiated",
                    active_command,
                    command
                );
                //Self::context_mut().reset();
                // TODO: proceed with initiating the other command?
            }
        }

        // Is the command receiving a new/increased count?
        // TODO: better way to parse a char?
        if let Some(value) = e
            .char()
            .and_then(|c| usize::from_str_radix(c.to_string().as_str(), 10).ok())
        {
            let mut evil_context = Self::context_mut();

            // If we start a count with 0, we don't mean a count, but most probably a motion (line start) instead.
            if value != 0 || evil_context.count.is_some() {
                evil_context.count = Some(evil_context.count.map(|c| c * 10).unwrap_or(0) + value);

                log::trace!(
                    "Key callback: Increasing count to {}",
                    evil_context.count.unwrap()
                );

                // TODO: cx.on_next_key()
                cx.on_next_key_callback = Some((
                    Box::new(move |cx: &mut Context, e: KeyEvent| {
                        Self::evil_command_key_callback(cx, e);
                    }),
                    OnKeyCallbackKind::PseudoPending,
                ));

                return;
            }
        }

        if let Some(c) = e.char() {
            // Is the command receiving a modifier?
            if let Ok(modifier) = Modifier::try_from(c) {
                log::trace!("Key callback: Detected modifier key '{}'", c);

                Self::context_mut().modifiers.push(modifier);

                // TODO: cx.on_next_key()
                cx.on_next_key_callback = Some((
                    Box::new(move |cx: &mut Context, e: KeyEvent| {
                        Self::evil_command_key_callback(cx, e);
                    }),
                    OnKeyCallbackKind::PseudoPending,
                ));

                return;
            }

            // Is the command being executed with a motion key?
            // Check this after the count check, because "0" could imply increasing the count,
            // and if it doesn't, it's probably a motion key.
            if let Some(motion) = e.char().and_then(|c| Motion::try_from(c).ok()) {
                log::trace!("Key callback: Detected motion key '{}'", c);

                Self::context_mut().motion = Some(motion);
                // TODO; a motion key should immediately execute the command
                Self::evil_command(cx, active_command, set_mode);
                return;
            }
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
            match op {
                Operation::Delete => Command::Delete,
                Operation::Change => Command::Change,
            },
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

    pub fn find_char<F>(cx: &mut Context, base_fn: F, direction: Direction, inclusive: bool)
    where
        F: FnOnce(&mut Context, Direction, bool, bool),
    {
        let extend = true; // pretty sure this should be true to match how vim works
        base_fn(cx, direction, inclusive, extend);
        let inner_callback = cx.on_next_key_callback.take();

        if let Some(inner_callback) = inner_callback {
            cx.on_next_key(move |cx, event| {
                inner_callback.0(cx, event);

                match Self::get_mode(cx) {
                    Mode::Normal => Self::collapse_selections(cx, CollapseMode::ToHead),
                    _ => {}
                }
            })
        } else {
            log::warn!("The find_char base function did not set a key callback");
        }
    }
}
