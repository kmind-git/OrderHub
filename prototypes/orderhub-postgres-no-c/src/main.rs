//! Links the retained production PostgreSQL crate without a database service.

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;

fn main() {
    let options = PostgresConnectOptions::new(
        String::from("localhost"),
        5432,
        String::from("orderhub"),
        String::from("probe-only"),
        String::from("orderhub"),
    );
    assert_eq!(
        options.connection_string_masked(),
        "postgres://orderhub:***@localhost:5432/orderhub"
    );
}
