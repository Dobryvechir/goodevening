//! The search / replace dialog shared by the viewer and the editor.

use crate::dialog::DialogBuilder;
use crate::ui::Ui;
use gm_i18n::{MsgId, msg};
use gm_text::regex_adapter::SearchSpec;

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub spec: SearchSpec,
    pub replace: String,
    pub hex: bool,
    pub reverse: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchAction {
    Find,
    FindPrev,
    Replace,
    ReplaceAll,
}

/// Shows the dialog; `allow_hex` adds the Text/Hex choice (viewer).
pub fn search_dialog(ui: &mut Ui, st: &mut SearchState, replace: bool, allow_hex: bool, word: &str) -> Option<SearchAction> {
    let title = if replace { msg(MsgId::MSearchReplaceReplaceTitle) } else { msg(MsgId::MSearchReplaceSearchTitle) };
    let mut b = DialogBuilder::new(title, 76).help(if replace { "EditorSearch" } else { "ViewerSearch" });
    b.text(msg(MsgId::MSearchReplaceSearchFor));
    let initial = if st.spec.text.is_empty() { word } else { &st.spec.text };
    let find = b.edit(initial, Some("SearchText"));
    let repl = if replace {
        b.text(msg(MsgId::MSearchReplaceReplaceWith));
        Some(b.edit(&st.replace, Some("ReplaceText")))
    } else {
        None
    };
    b.separator();
    let (text_r, hex_r) = if allow_hex {
        let t = b.radio(msg(MsgId::MSearchReplaceText), !st.hex, true);
        let h = b.radio(msg(MsgId::MSearchReplaceHex), st.hex, false);
        (Some(t), Some(h))
    } else {
        (None, None)
    };
    let case = b.checkbox(msg(MsgId::MSearchReplaceCase), st.spec.case_sensitive);
    let words = b.checkbox(msg(MsgId::MSearchReplaceWholeWords), st.spec.whole_words);
    let regex = b.checkbox(msg(MsgId::MSearchReplaceRegexp), st.spec.regex);
    b.separator();
    let first = if replace {
        b.buttons(
            &[msg(MsgId::MSearchReplaceReplace), msg(MsgId::MSearchReplaceAll), msg(MsgId::MSearchReplaceFindPrev), msg(MsgId::MSearchReplaceCancel)],
            0,
        )
    } else {
        b.buttons(&[msg(MsgId::MSearchReplaceFind), msg(MsgId::MSearchReplaceFindPrev), msg(MsgId::MSearchReplaceCancel)], 0)
    };
    let _ = text_r;
    let mut d = b.build();
    let r = d.run(ui)?;
    let btn = r.saturating_sub(first);
    st.spec.text = d.text(find);
    if let Some(i) = repl {
        st.replace = d.text(i);
    }
    st.hex = hex_r.is_some_and(|h| d.checked(h));
    st.spec.case_sensitive = d.checked(case);
    st.spec.whole_words = d.checked(words);
    st.spec.regex = d.checked(regex);
    if st.spec.text.is_empty() {
        return None;
    }
    let action = if replace {
        match btn {
            0 => SearchAction::Replace,
            1 => SearchAction::ReplaceAll,
            2 => SearchAction::FindPrev,
            _ => return None,
        }
    } else {
        match btn {
            0 => SearchAction::Find,
            1 => SearchAction::FindPrev,
            _ => return None,
        }
    };
    st.reverse = action == SearchAction::FindPrev;
    Some(action)
}

/// Parses "48 65 6C" / "48656C" into bytes.
pub fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let digits: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.is_empty() || !digits.len().is_multiple_of(2) {
        return None;
    }
    (0..digits.len()).step_by(2).map(|i| u8::from_str_radix(&digits[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex() {
        assert_eq!(super::parse_hex("48 65 6c"), Some(vec![0x48, 0x65, 0x6c]));
        assert_eq!(super::parse_hex("4"), None);
    }
}
