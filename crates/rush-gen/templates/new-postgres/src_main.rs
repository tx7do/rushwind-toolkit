//! @@NAME@@ — a RushWind service on PostgreSQL.
//!
//! One YAML document assembles a PostgreSQL storage engine (the SeaORM
//! dynamic repository over the schema captured in the factory closure), a
//! CRUD HTTP edge over it, and an HTTP server with two route packs, all
//! under the RushWind lifecycle. Tables are created on startup via
//! `migrate_create` (create-if-missing; it does not alter existing
//! schemas).
//!
//! The DSN comes from the YAML `storage.settings.url`. Try it:
//! `cargo run`, then `curl http://<printed-endpoint>/health`, `/wired`,
//! and the `/items` CRUD edge.
//!
//! File-based configs work the same way: `Bootstrap::from_yaml_path`.

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use rushwind_bootstrap::{Bootstrap, BootstrapError, RouteInput, RouteSurface};
use rushwind_storage::{ColumnKind, Repository, Schema};
use rushwind_storage_seaorm::SeaRepo;
use rushwind_transport::StopSignal;

const CONFIG: &str = r#"
app:
  name: @@NAME@@
  version: v0.1.0
  stop_timeout_secs: 5
storage:
  engine: postgres
  settings:
    url: postgres://@@NAME@@:@@NAME@@@127.0.0.1:5432/@@NAME@@
storage_endpoints:
  - nest: /items
    api: crud
servers:
  - kind: http
    bind: 127.0.0.1:0
    route_packs:
      - name: health
      - name: wired
"#;

/// The application schema — captured by the storage factory, because the
/// schema is application knowledge, not configuration.
fn schema() -> Schema {
    Schema::builder("items", "id")
        .column("name", ColumnKind::Text)
        .build()
        .expect("schema is valid")
}

fn assemble() -> Result<Bootstrap, BootstrapError> {
    let bootstrap = Bootstrap::from_yaml_str(CONFIG)?.storage_factory("postgres", |settings| {
        Box::pin(async move {
            let url = settings
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("postgres://@@NAME@@:@@NAME@@@127.0.0.1:5432/@@NAME@@");
            let repo = SeaRepo::connect(url, schema())
                .await
                .map_err(|e| BootstrapError::Failed(e.to_string()))?;
            repo.migrate_create()
                .await
                .map_err(|e| BootstrapError::Failed(e.to_string()))?;
            Ok(Arc::new(repo) as Arc<dyn Repository>)
        })
    });
    Ok(bootstrap
        .route_pack("health", |_, _| {
            Ok(RouteSurface::new(
                Router::new().route("/health", get(|| async { "ok" })),
            ))
        })
        .route_pack("wired", |_settings, input: RouteInput| {
            // Proves the configured storage reaches route packs.
            let repository = input.repository.clone();
            Ok(RouteSurface::new(Router::new().route(
                "/wired",
                get(move || {
                    let repository = repository.clone();
                    async move { format!("repo={}", repository.is_some()) }
                }),
            )))
        }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrapped = assemble()?.build().await?;
    for endpoint in &bootstrapped.endpoints {
        println!("[@@NAME@@] serving on {endpoint}");
    }
    println!(
        "[@@NAME@@] storage configured: {}; crud edge at /items",
        bootstrapped.repository.is_some()
    );

    let result = bootstrapped.app.run(StopSignal::new()).await;
    println!("[@@NAME@@] lifecycle finished: {result:?}");
    Ok(())
}
