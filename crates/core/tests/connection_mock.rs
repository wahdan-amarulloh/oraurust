use oraurust_core::{
    CancelHandle, Catalog, Column, Connection, DbError, FETCH_ROWS, MockCancel, MockConnection,
    Object, ObjectKind, ROW_CAP, StatementOutcome,
};

fn id_column() -> Column {
    Column {
        name: "ID".into(),
        type_name: "NUMBER".into(),
    }
}

#[test]
fn dml_returns_row_count_and_surfaces_ora_errors() {
    let conn = MockConnection::new()
        .with_row_count(3)
        .with_error(
            "select * from missing",
            "ORA-00942",
            "table or view does not exist",
            None,
        )
        .with_error(
            "update t set x = 'bad",
            "ORA-01756",
            "quoted string not properly terminated",
            Some(19),
        );

    assert_eq!(
        conn.execute("update t set x = 1").unwrap(),
        StatementOutcome::RowCount(3)
    );

    let err = conn
        .execute("update t set x = 'bad")
        .expect_err("scripted error");
    assert_eq!(err.code, "ORA-01756");
    assert_eq!(
        err.position,
        Some(19),
        "error position must survive the seam"
    );

    let err = conn
        .execute("select * from missing")
        .expect_err("scripted error");
    assert_eq!(
        err,
        DbError {
            code: "ORA-00942".into(),
            message: "table or view does not exist".into(),
            position: None,
        }
    );
}

#[test]
fn select_returns_columns_and_rows() {
    let conn =
        MockConnection::new().with_select(vec![id_column()], vec![vec![Some("1".into()), None]]);

    match conn.execute("select id from t").expect("rows") {
        StatementOutcome::Rows { page, cursor } => {
            assert_eq!(page.columns.len(), 1);
            assert_eq!(page.columns[0].name, "ID");
            assert_eq!(page.rows.len(), 1);
            assert_eq!(page.rows[0][0], Some("1".into()));
            assert_eq!(page.fetched, 1, "the first page reports its row count");
            assert!(cursor.is_none(), "one-page results have no cursor");
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn keyed_select_wins_over_the_wildcard_select() {
    let columns = vec![Column {
        name: "X".into(),
        type_name: "NUMBER".into(),
    }];
    let conn = MockConnection::new()
        .with_select(columns.clone(), vec![vec![Some("wildcard".into())]])
        .with_select_for(
            "select specific from t",
            columns,
            vec![vec![Some("keyed".into())]],
        );

    match conn.execute("select specific from t").expect("keyed") {
        StatementOutcome::Rows { page, .. } => assert_eq!(page.rows[0][0], Some("keyed".into())),
        other => panic!("expected Rows, got {other:?}"),
    }
    match conn.execute("select other from t").expect("wildcard") {
        StatementOutcome::Rows { page, .. } => assert_eq!(page.rows[0][0], Some("wildcard".into())),
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn a_paged_select_hands_out_a_cursor_and_accumulates_the_fetched_count() {
    // Issue #6: a SELECT is fetched in pages, and the grid shows how many
    // rows have been fetched so far.
    let conn = MockConnection::new().with_pages_for(
        "select id from many",
        vec![id_column()],
        vec![
            vec![vec![Some("1".into())], vec![Some("2".into())]],
            vec![vec![Some("3".into())]],
        ],
    );

    let execution = conn.execute("select id from many").expect("first page");
    let (first, mut cursor) = match execution {
        StatementOutcome::Rows { page, cursor } => (page, cursor.expect("more rows remain")),
        other => panic!("expected Rows, got {other:?}"),
    };
    assert_eq!(first.rows.len(), 2);
    assert_eq!(first.fetched, 2);
    assert!(first.more, "a page before the last reports more rows");
    assert!(!first.capped);

    let second = cursor.fetch(500).expect("second page");
    assert_eq!(second.rows, vec![vec![Some("3".into())]]);
    assert_eq!(second.fetched, 3, "fetched counts across pages");
    assert!(!second.more, "the last page reports no more rows");
    assert_eq!(second.columns, first.columns, "pages keep the same columns");
}

#[test]
fn a_cursor_stops_at_the_row_cap_and_reports_it() {
    // The cap is a seam-level rule (ADR-0002), so the mock enforces it too:
    // two pages totalling more than ROW_CAP must stop at the cap, capped.
    let row = || vec![Some("1".to_string())];
    let page_a: Vec<_> = (0..ROW_CAP / 2).map(|_| row()).collect();
    let page_b: Vec<_> = (0..ROW_CAP).map(|_| row()).collect();
    let conn = MockConnection::new().with_pages_for(
        "select id from many",
        vec![id_column()],
        vec![page_a, page_b],
    );

    let (first, mut cursor) = match conn.execute("select id from many").expect("first page") {
        StatementOutcome::Rows { page, cursor } => (page, cursor.expect("more rows remain")),
        other => panic!("expected Rows, got {other:?}"),
    };
    assert_eq!(first.fetched, ROW_CAP / 2);

    let last = loop {
        let page = cursor.fetch(FETCH_ROWS).expect("page");
        if page.capped {
            break page;
        }
        assert!(page.more, "a non-final page must offer more rows");
    };
    assert_eq!(last.fetched, ROW_CAP, "the cap is exactly ROW_CAP");
    assert!(!last.more, "the cap ends the Result Set");
}

#[test]
fn execute_is_reachable_through_a_shared_reference() {
    // Execution runs on the Session's worker thread while the UI keeps a
    // cancel handle (issue #6), so the seam must not need `&mut`.
    let conn = MockConnection::new().with_row_count(2);
    let shared = &conn;

    assert_eq!(
        shared.execute("update t set x = 1").unwrap(),
        StatementOutcome::RowCount(2)
    );
}

#[test]
fn cancel_is_a_separate_handle_over_the_same_connection() {
    // ADR-0002: cancel cannot go through the blocked `execute`, so a driver
    // hands out a second handle. The mock's handle succeeds and does nothing.
    let cancel: Box<dyn CancelHandle> = Box::new(MockCancel);

    assert_eq!(cancel.cancel(), Ok(()));
}

#[test]
fn ddl_fetch_and_schema_come_through_the_seam() {
    let table = Object {
        name: "EMP".into(),
        kind: ObjectKind::Table,
    };
    let conn = MockConnection::new()
        .with_ddl(&table, "CREATE TABLE EMP (ID NUMBER)")
        .with_source(
            &Object {
                name: "P".into(),
                kind: ObjectKind::Procedure,
            },
            "PROCEDURE P IS BEGIN NULL; END;",
        )
        .with_catalog(Catalog {
            objects: vec![table.clone()],
        });

    assert_eq!(
        conn.fetch_ddl(&table),
        Ok("CREATE TABLE EMP (ID NUMBER)".into())
    );
    assert_eq!(
        conn.fetch_source(&Object {
            name: "P".into(),
            kind: ObjectKind::Procedure
        }),
        Ok("PROCEDURE P IS BEGIN NULL; END;".into())
    );
    assert_eq!(conn.describe_schema().unwrap().objects, vec![table]);
}
