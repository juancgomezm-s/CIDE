use jc_app::{
    snippets::{ParameterRule, SnippetParameter, SnippetSession, SnippetTarget, SnippetTemplate},
    FieldEdit, Workbench,
};
use jc_text::{encode, TextEncoding, TextPosition};
const NAME: SnippetParameter = SnippetParameter {
    key: "name",
    label: "Name",
    default: "demo",
    rule: ParameterRule::Identifier { extra: "-" },
};
static INSERT: SnippetTemplate = SnippetTemplate {
    id: "test.insert",
    name: "Insert",
    parameters: &[NAME],
    body: "BEGIN ${name}\n    ${cursor}\nEND",
    surround: false,
};
static WRAP: SnippetTemplate = SnippetTemplate {
    id: "test.wrap",
    name: "Wrap",
    parameters: &[],
    body: "DO:\n${selection}\nEND.${cursor}",
    surround: true,
};
fn app(text: &str, encoding: TextEncoding) -> Workbench {
    let mut a = Workbench::default();
    a.new_file("snippet-memory.txt", encoding);
    a.insert_text(text).unwrap();
    a
}
#[test]
fn snippet_metadata_and_nonrecursive_variables_are_explicit_and_bounded() {
    assert!(INSERT.validate("test").is_ok());
    assert!(WRAP.validate("test").is_ok());
    for body in [
        "${unknown}",
        "${name",
        "\r\n",
        "${cursor}${cursor}",
        "$${name}",
    ] {
        let template = SnippetTemplate { body, ..INSERT };
        assert!(template.validate("test").is_err(), "{body}");
    }
    static LITERAL: SnippetTemplate = SnippetTemplate {
        id: "test.literal",
        name: "Literal",
        parameters: &[SnippetParameter {
            key: "value",
            label: "Value",
            default: "${cursor}",
            rule: ParameterRule::SingleLine,
        }],
        body: "$${selection} ${value}${cursor}",
        surround: false,
    };
    let a = app("", TextEncoding::Utf8);
    let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), &LITERAL).unwrap();
    s.prepare(&a).unwrap();
    assert_eq!(s.plan().unwrap().text(), "${selection} ${cursor}");
}
#[test]
fn snippet_parameter_editing_preview_is_readonly_and_apply_is_one_undo() {
    let mut a = app("before\n", TextEncoding::Utf8);
    let original = a.active_bytes().unwrap();
    let revision = a.active_document().unwrap().session.document().revision();
    let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), &INSERT).unwrap();
    s.edit_parameter(0, FieldEdit::Clear).unwrap();
    s.edit_parameter(0, FieldEdit::Insert("hello")).unwrap();
    s.prepare(&a).unwrap();
    assert_eq!(a.active_bytes().unwrap(), original);
    assert_eq!(
        a.active_document().unwrap().session.document().revision(),
        revision
    );
    assert_eq!(s.plan().unwrap().text(), "BEGIN hello\n    \nEND");
    s.apply(&mut a).unwrap();
    assert_eq!(a.active_bytes().unwrap(), b"before\nBEGIN hello\n    \nEND");
    assert_eq!(
        a.active_document().unwrap().session.document().revision(),
        revision + 1
    );
    assert_eq!(
        a.active_position(),
        Some(TextPosition { line: 2, column: 4 })
    );
    a.undo().unwrap();
    assert_eq!(a.active_bytes().unwrap(), original);
    a.redo().unwrap();
    assert!(a.active_bytes().unwrap().ends_with(b"END"));
}
#[test]
fn snippet_surround_preserves_bom_mixed_eol_terminal_newline_and_shared_views_in_all_encodings() {
    for encoding in [
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_1,
        TextEncoding::Ibm850,
    ] {
        let bom = if encoding == TextEncoding::Utf8 {
            "\u{feff}"
        } else {
            ""
        };
        let text = format!("{bom}alpha\r\nniño\nend\r\n");
        let mut a = app(&text, encoding);
        let original = a.active_bytes().unwrap();
        let id = a.active_document().unwrap().id;
        let tab = a.tabs().active_id().unwrap();
        a.split_editor_right();
        a.select_all().unwrap();
        let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), &WRAP).unwrap();
        s.prepare(&a).unwrap();
        s.apply(&mut a).unwrap();
        let expected = encode(
            &format!("{bom}DO:\r\nalpha\r\nniño\nend\r\nEND.\r\n"),
            encoding,
        )
        .unwrap();
        assert_eq!(a.active_bytes().unwrap(), expected);
        assert_eq!(a.active_document().unwrap().id, id);
        assert_eq!(
            a.panes()
                .view(jc_app::PaneId::Primary)
                .unwrap()
                .location()
                .unwrap()
                .tab,
            tab
        );
        assert_eq!(
            a.panes()
                .view(jc_app::PaneId::Secondary)
                .unwrap()
                .location()
                .unwrap()
                .tab,
            tab
        );
        a.undo().unwrap();
        assert_eq!(a.active_bytes().unwrap(), original);
        a.redo().unwrap();
        assert_eq!(a.active_bytes().unwrap(), expected);
    }
}
#[test]
fn snippet_cursor_crlf_or_new_bom_and_unrepresentable_body_fail_before_mutation() {
    static CRLF: SnippetTemplate = SnippetTemplate {
        id: "test.crlf",
        name: "Illegal caret",
        parameters: &[],
        body: "${cursor}\n",
        surround: false,
    };
    static BOM: SnippetTemplate = SnippetTemplate {
        id: "test.bom",
        name: "Illegal BOM caret",
        parameters: &[],
        body: "${cursor}\u{feff}content",
        surround: false,
    };
    static LEGACY: SnippetTemplate = SnippetTemplate {
        id: "test.legacy",
        name: "Unicode body",
        parameters: &[],
        body: "Ω${cursor}",
        surround: false,
    };
    for (text, encoding, template) in [
        ("a\nb\nx\r", TextEncoding::Utf8, &CRLF),
        ("", TextEncoding::Utf8, &BOM),
        ("before", TextEncoding::Windows1252, &LEGACY),
    ] {
        let mut a = app(text, encoding);
        let original = a.active_bytes().unwrap();
        let revision = a.active_document().unwrap().session.document().revision();
        let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), template).unwrap();
        assert!(s.prepare(&a).is_err());
        assert!(s.apply(&mut a).is_err());
        assert_eq!(a.active_bytes().unwrap(), original);
        assert_eq!(
            a.active_document().unwrap().session.document().revision(),
            revision
        );
    }
}
#[test]
fn snippet_stale_caret_selection_revision_encoding_or_view_cannot_apply() {
    for change in 0..5 {
        let mut a = app("content", TextEncoding::Utf8);
        let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), &INSERT).unwrap();
        s.prepare(&a).unwrap();
        match change {
            0 => a.move_left(false).unwrap(),
            1 => a.select_all().unwrap(),
            2 => a.insert_text("x").unwrap(),
            3 => a.set_encoding(TextEncoding::Iso8859_1).unwrap(),
            _ => {
                a.split_editor_right();
                a.focus_editor_pane(jc_app::PaneId::Secondary);
            }
        }
        let bytes = a.active_bytes().unwrap();
        assert!(s.apply(&mut a).is_err());
        assert_eq!(a.active_bytes().unwrap(), bytes);
    }
}
#[test]
fn snippet_selection_and_rendered_expansion_quotas_reject_without_partial_edits() {
    let mut a = app(&"x".repeat(16 * 1024 + 1), TextEncoding::Utf8);
    a.select_all().unwrap();
    assert!(SnippetTarget::capture(&a).is_err());
    let body = Box::leak("${value}".repeat(300).into_boxed_str());
    let value = Box::leak("z".repeat(256).into_boxed_str());
    let parameters = Box::leak(
        vec![SnippetParameter {
            key: "value",
            label: "Value",
            default: value,
            rule: ParameterRule::SingleLine,
        }]
        .into_boxed_slice(),
    );
    let template = Box::leak(Box::new(SnippetTemplate {
        id: "test.limit",
        name: "Limit",
        parameters,
        body,
        surround: false,
    }));
    let a = app("content", TextEncoding::Utf8);
    let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), template).unwrap();
    assert!(s.prepare(&a).is_err());
    assert!(s.plan().is_none());
    assert_eq!(a.active_bytes().unwrap(), b"content");
}
#[test]
fn snippet_preview_rows_are_bounded_and_save_as_invalidates_even_without_revision_change() {
    let mut a = app("content", TextEncoding::Utf8);
    let mut s = SnippetSession::new(SnippetTarget::capture(&a).unwrap(), &INSERT).unwrap();
    s.prepare(&a).unwrap();
    for columns in [0, 1, 40, usize::MAX] {
        let rows = s.plan().unwrap().rows(0, usize::MAX, columns);
        assert!(rows.len() <= 96);
        assert!(rows
            .iter()
            .all(|r| r.is_ascii() && r.len() <= columns.min(256)));
    }
    assert!(s
        .plan()
        .unwrap()
        .rows(usize::MAX, usize::MAX, 80)
        .is_empty());
    let root = std::env::temp_dir().join(format!("cide-snippet-saveas-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("saved.txt");
    a.save_active_as(&path).unwrap();
    assert!(s.apply(&mut a).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"content");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}
