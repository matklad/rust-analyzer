mod generated;

use std::sync::Arc;

use hir::Semantics;
use ra_db::{fixture::WithFixture, FileId, FileRange, SourceDatabaseExt};
use ra_ide_db::{symbol_index::SymbolsDatabase, RootDatabase};
use ra_syntax::TextRange;
use test_utils::{
    add_cursor, assert_eq_text, extract_offset, extract_range, extract_range_or_offset,
    RangeOrOffset,
};

use crate::{handlers::Handler, resolved_assists, AssistCtx, AssistFile};

pub(crate) fn with_single_file(text: &str) -> (RootDatabase, FileId) {
    let (mut db, file_id) = RootDatabase::with_single_file(text);
    // FIXME: ideally, this should be done by the above `RootDatabase::with_single_file`,
    // but it looks like this might need specialization? :(
    db.set_local_roots(Arc::new(vec![db.file_source_root(file_id)]));
    (db, file_id)
}

pub(crate) fn check_assist(assist: Handler, ra_fixture_before: &str, ra_fixture_after: &str) {
    check(assist, ra_fixture_before, ExpectedResult::After(ra_fixture_after));
}

// FIXME: instead of having a separate function here, maybe use
// `extract_ranges` and mark the target as `<target> </target>` in the
// fixuture?
pub(crate) fn check_assist_target(assist: Handler, ra_fixture: &str, target: &str) {
    check(assist, ra_fixture, ExpectedResult::Target(target));
}

pub(crate) fn check_assist_not_applicable(assist: Handler, ra_fixture: &str) {
    check(assist, ra_fixture, ExpectedResult::NotApplicable);
}

fn check_doc_test(assist_id: &str, before: &str, after: &str) {
    let (selection, before) = extract_range_or_offset(before);
    let (db, file_id) = crate::tests::with_single_file(&before);
    let frange = FileRange { file_id, range: selection.into() };

    let assist = resolved_assists(&db, frange)
        .into_iter()
        .find(|assist| assist.label.id.0 == assist_id)
        .unwrap_or_else(|| {
            panic!(
                "\n\nAssist is not applicable: {}\nAvailable assists: {}",
                assist_id,
                resolved_assists(&db, frange)
                    .into_iter()
                    .map(|assist| assist.label.id.0)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });

    let actual = {
        let mut actual = before.clone();
        assist.action.edit.apply(&mut actual);
        actual
    };
    assert_eq_text!(after, &actual);
}

enum ExpectedResult<'a> {
    NotApplicable,
    After(&'a str),
    Target(&'a str),
}

fn check(assist: Handler, before: &str, expected: ExpectedResult) {
    let (text_without_caret, file_with_caret_id, range_or_offset, db) = if before.contains("//-") {
        let (mut db, position) = RootDatabase::with_position(before);
        db.set_local_roots(Arc::new(vec![db.file_source_root(position.file_id)]));
        (
            db.file_text(position.file_id).as_ref().to_owned(),
            position.file_id,
            RangeOrOffset::Offset(position.offset),
            db,
        )
    } else {
        let (range_or_offset, text_without_caret) = extract_range_or_offset(before);
        let (db, file_id) = with_single_file(&text_without_caret);
        (text_without_caret, file_id, range_or_offset, db)
    };

    let frange = FileRange { file_id: file_with_caret_id, range: range_or_offset.into() };

    let sema = Semantics::new(&db);
    let assist_ctx = AssistCtx::new(&sema, frange, true);

    match (assist(assist_ctx), expected) {
        (Some(assist), ExpectedResult::After(after)) => {
            let action = assist.0[0].action.clone().unwrap();

            let mut actual = if let AssistFile::TargetFile(file_id) = action.file {
                db.file_text(file_id).as_ref().to_owned()
            } else {
                text_without_caret
            };
            action.edit.apply(&mut actual);

            match action.cursor_position {
                None => {
                    if let RangeOrOffset::Offset(before_cursor_pos) = range_or_offset {
                        let off = action
                            .edit
                            .apply_to_offset(before_cursor_pos)
                            .expect("cursor position is affected by the edit");
                        actual = add_cursor(&actual, off)
                    }
                }
                Some(off) => actual = add_cursor(&actual, off),
            };

            assert_eq_text!(after, &actual);
        }
        (Some(assist), ExpectedResult::Target(target)) => {
            let range = assist.0[0].label.target;
            assert_eq_text!(&text_without_caret[range], target);
        }
        (Some(_), ExpectedResult::NotApplicable) => panic!("assist should not be applicable!"),
        (None, ExpectedResult::After(_)) | (None, ExpectedResult::Target(_)) => {
            panic!("code action is not applicable")
        }
        (None, ExpectedResult::NotApplicable) => (),
    };
}

#[test]
fn assist_order_field_struct() {
    let before = "struct Foo { <|>bar: u32 }";
    let (before_cursor_pos, before) = extract_offset(before);
    let (db, file_id) = with_single_file(&before);
    let frange = FileRange { file_id, range: TextRange::empty(before_cursor_pos) };
    let assists = resolved_assists(&db, frange);
    let mut assists = assists.iter();

    assert_eq!(
        assists.next().expect("expected assist").label.label,
        "Change visibility to pub(crate)"
    );
    assert_eq!(assists.next().expect("expected assist").label.label, "Add `#[derive]`");
}

#[test]
fn assist_order_if_expr() {
    let before = "
    pub fn test_some_range(a: int) -> bool {
        if let 2..6 = <|>5<|> {
            true
        } else {
            false
        }
    }";
    let (range, before) = extract_range(before);
    let (db, file_id) = with_single_file(&before);
    let frange = FileRange { file_id, range };
    let assists = resolved_assists(&db, frange);
    let mut assists = assists.iter();

    assert_eq!(assists.next().expect("expected assist").label.label, "Extract into variable");
    assert_eq!(assists.next().expect("expected assist").label.label, "Replace with match");
}
