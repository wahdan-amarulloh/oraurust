use oraurust_core::{
    Catalog, Column, Connection, DbError, MockConnection, Object, ObjectKind, StatementOutcome,
};

#[test]
fn dml_returns_row_count_and_surfaces_ora_errors() {
    let mut conn = MockConnection::new()
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

    assert_eq!(conn.execute("update t set x = 1"), Ok(StatementOutcome::RowCount(3)));

    let err = conn
        .execute("update t set x = 'bad")
        .expect_err("scripted error");
    assert_eq!(err.code, "ORA-01756");
    assert_eq!(err.position, Some(19), "error position must survive the seam");

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
    let mut conn = MockConnection::new().with_select(
        vec![Column { name: "ID".into(), type_name: "NUMBER".into() }],
        vec![vec![Some("1".into()), None]],
    );

    let outcome = conn.execute("select id from t").expect("rows");
    match outcome {
        StatementOutcome::Rows(rs) => {
            assert_eq!(rs.columns.len(), 1);
            assert_eq!(rs.columns[0].name, "ID");
            assert_eq!(rs.rows.len(), 1);
            assert_eq!(rs.rows[0][0], Some("1".into()));
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn keyed_select_wins_over_the_wildcard_select() {
    let columns = vec![Column { name: "X".into(), type_name: "NUMBER".into() }];
    let mut conn = MockConnection::new()
        .with_select(columns.clone(), vec![vec![Some("wildcard".into())]])
        .with_select_for("select specific from t", columns, vec![vec![Some("keyed".into())]]);

    match conn.execute("select specific from t").expect("keyed") {
        StatementOutcome::Rows(rs) => assert_eq!(rs.rows[0][0], Some("keyed".into())),
        other => panic!("expected Rows, got {other:?}"),
    }
    match conn.execute("select other from t").expect("wildcard") {
        StatementOutcome::Rows(rs) => assert_eq!(rs.rows[0][0], Some("wildcard".into())),
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn ddl_fetch_and_schema_come_through_the_seam() {
    let table = Object { name: "EMP".into(), kind: ObjectKind::Table };
    let conn = MockConnection::new()
        .with_ddl(&table, "CREATE TABLE EMP (ID NUMBER)")
        .with_source(
            &Object { name: "P".into(), kind: ObjectKind::Procedure },
            "PROCEDURE P IS BEGIN NULL; END;",
        )
        .with_catalog(Catalog {
            objects: vec![table.clone()],
        });

    assert_eq!(conn.fetch_ddl(&table), Ok("CREATE TABLE EMP (ID NUMBER)".into()));
    assert_eq!(
        conn.fetch_source(&Object { name: "P".into(), kind: ObjectKind::Procedure }),
        Ok("PROCEDURE P IS BEGIN NULL; END;".into())
    );
    assert_eq!(conn.describe_schema().unwrap().objects, vec![table]);
}
