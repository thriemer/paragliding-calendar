use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::PgPool;
use testcontainers::{ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner};

static DB_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn start_postgis() -> ContainerAsync<GenericImage> {
    GenericImage::new("postgis/postgis", "16-3.4")
        .with_wait_for(testcontainers::core::WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_USER", "test")
        .with_env_var("POSTGRES_PASSWORD", "test")
        .with_env_var("POSTGRES_DB", "test")
        .start()
        .await
        .expect("failed to start PostGIS container")
}

static CONTAINER: tokio::sync::OnceCell<ContainerAsync<GenericImage>> =
    tokio::sync::OnceCell::const_new();

pub async fn test_pool() -> PgPool {
    let container = CONTAINER
        .get_or_init(|| async { start_postgis().await })
        .await;

    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get container port");

    let db_name = format!("test_{}", DB_COUNTER.fetch_add(1, Ordering::Relaxed));

    let admin_url = format!("postgres://test:test@127.0.0.1:{}/test", port);
    let admin_pool = PgPool::connect(&admin_url)
        .await
        .expect("failed to connect to admin db");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&admin_pool)
        .await
        .expect("failed to create test database");
    admin_pool.close().await;

    let test_url = format!("postgres://test:test@127.0.0.1:{}/{}", port, db_name);
    let pool = PgPool::connect(&test_url)
        .await
        .expect("failed to connect to test db");
    crate::MIGRATOR
        .run(&pool)
        .await
        .expect("failed to run migrations");

    pool
}
