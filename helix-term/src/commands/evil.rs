use std::{borrow::Cow, num::NonZeroUsize};

use helix_core::{register::Registers, Range, RopeSlice, Selection, Transaction};

use crate::commands::{enter_insert_mode, exit_select_mode, Context, Extend, Operation};

pub struct EvilCommands;

impl EvilCommands {
    fn yank_selection(
        text: RopeSlice,
        selection: &Selection,
        registers: &mut Registers,
        register_name: char,
    ) {
        let values: Vec<String> = selection.fragments(text).map(Cow::into_owned).collect();
        let register = registers.get_mut(register_name);
        register.write(values);
    }

    pub fn yank(cx: &mut Context) {
        let (view, doc) = current!(cx.editor);
        let text = doc.text().slice(..);
        let mut selection: Option<Selection> = None;

        match doc.mode {
            helix_view::document::Mode::Normal => {
                // TODO: even in Normal mode, there can be a selection -> should it be disregarded,
                // or can we assume this shouldn't happen in evil mode?

                // Delete a number of lines: first create a temporary selection of the text to be deleted
                let lines_to_select = cx.count.unwrap_or(NonZeroUsize::new(1).unwrap()).get();

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

        /*let msg = format!(
            "yanked {} selection(s) to register {}",
            values.len(),
            cx.register.unwrap_or('"')
        );*/

        if let Some(selection) = selection {
            let registers = &mut cx.editor.registers;
            let register_name = cx.register.unwrap_or('"');
            Self::yank_selection(text, &selection, registers, register_name);
        }

        //cx.editor.set_status(msg);
        exit_select_mode(cx);
    }

    /// Delete one or more lines, or delete the selected text.
    /// Default: *dd or d*d
    pub fn delete(cx: &mut Context, op: Operation) {
        let (view, doc) = current!(cx.editor);

        let text = doc.text().slice(..);
        let mut selection: Option<Selection> = None;

        match doc.mode {
            helix_view::document::Mode::Normal => {
                // TODO: even in Normal mode, there can be a selection -> should it be disregarded,
                // or can we assume this shouldn't happen in evil mode?

                // Delete a number of lines: first create a temporary selection of the text to be deleted
                let lines_to_select = cx.count.unwrap_or(NonZeroUsize::new(1).unwrap()).get();

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
                // Delete the selected text
                selection = Some(doc.selection(view.id).clone());
            }
            helix_view::document::Mode::Insert => {
                log::debug!("Attempted to select while in insert mode");
            }
        }

        if let Some(selection) = selection {
            if cx.register != Some('_') {
                // first yank the selection
                let registers = &mut cx.editor.registers;
                let register_name = cx.register.unwrap_or('"');
                Self::yank_selection(text, &selection, registers, register_name);
            };

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
                enter_insert_mode(doc);
            }
        }
    }

    pub fn delete_to_eol() {}

    /// Delete the character underneath/to the right of the cursor.
    /// Default: x
    pub fn delete_char() {}

    /// Delete the character left of the cursor.
    /// Default: X
    pub fn delete_char_left() {}
}
