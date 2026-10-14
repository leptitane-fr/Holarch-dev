//! A line of text being typed: the address bar, the find bar, a page's
//! text field. The caret counts characters.

use alloc::string::String;

#[derive(Clone, Default)]
pub struct Line {
    pub text: String,
    pub caret: usize,
    /// Everything is selected: the next character replaces it.
    pub all: bool,
}

/// Where the `n`th character starts, in bytes.
pub fn byte_at(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}

impl Line {
    /// Starts over with `text`, all of it selected.
    pub fn set(&mut self, text: &str) {
        self.text = String::from(text);
        self.caret = text.chars().count();
        self.all = true;
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn insert(&mut self, c: char) {
        if self.all {
            self.text.clear();
            self.caret = 0;
            self.all = false;
        }
        let at = byte_at(&self.text, self.caret);
        self.text.insert(at, c);
        self.caret += 1;
    }

    pub fn backspace(&mut self) {
        if self.all {
            self.clear();
        } else if self.caret > 0 {
            self.caret -= 1;
            let at = byte_at(&self.text, self.caret);
            self.text.remove(at);
        }
    }

    pub fn delete(&mut self) {
        if self.all {
            self.clear();
        } else if self.caret < self.len() {
            let at = byte_at(&self.text, self.caret);
            self.text.remove(at);
        }
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.all = false;
    }

    /// Moves the caret by `by` characters, or to an end.
    pub fn go(&mut self, by: isize) {
        self.all = false;
        let len = self.len() as isize;
        self.caret = (self.caret as isize + by).clamp(0, len) as usize;
    }

    pub fn home(&mut self) {
        self.all = false;
        self.caret = 0;
    }

    pub fn end(&mut self) {
        self.all = false;
        self.caret = self.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_replaces_a_selection_then_edits() {
        let mut l = Line::default();
        l.set("exemple.fr");
        l.insert('â');
        l.insert('n');
        l.insert('e');
        assert_eq!(l.text, "âne");
        l.go(-2);
        l.backspace();
        assert_eq!((l.text.as_str(), l.caret), ("ne", 0));
        l.end();
        l.insert('!');
        l.home();
        l.delete();
        assert_eq!(l.text, "e!");
    }
}
