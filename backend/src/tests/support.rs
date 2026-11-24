//! Shared test harness: a disposable PostgreSQL database, a dedicated Redis
//! database and the real `/api` router.
//!
//! Every test gets the same database and Redis instance, so tests are executed
//! one at a time through [`TestContext::new`]. The database is truncated and
//! Redis is flushed before each test, which keeps assertions on persisted rows
//! deterministic.
//!
//! Connection strings can be overridden with:
//! * `TEST_DATABASE_URL` — defaults to `postgres://postgres:postgres@localhost:5432/aeroflot_test`.
//! * `TEST_REDIS_URL` — defaults to `redis://127.0.0.1:6379/15`.
//! * `TEST_MAP_PATH` and `TEST_DATASET_PATH` — default to `assets/map2.json` and
//!   `assets/problem_classification_dataset.tsv`.

use std::{
    env,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, RwLock},
};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use diesel::{
    PgConnection,
    prelude::*,
    r2d2::{ConnectionManager, Pool, PooledConnection},
};
use diesel_migrations::MigrationHarness;
use redis::TypedCommands;
use serde_json::{Value, json};
use tokio::sync::{Mutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

use crate::{
    database::{establish_pg_connection, establish_redis_connection, schema},
    models::{dispatcher::Dispatcher, engineer::Engineer, task::Task},
    types::{
        config::AppState,
        enums::{EngineerType, UserRole},
    },
    utils::{create_token_pair, hash_password, revoke_and_store_tokens},
};

/// Password stored for users created by the harness.
pub const PASSWORD: &str = "test-password-123";

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn test_lock() -> &'static Mutex<()> {
    TEST_LOCK.get_or_init(|| Mutex::new(()))
}

/// One isolated API state. Dropping it releases the global test lock.
pub struct TestContext {
    _guard: MutexGuard<'static, ()>,
    app: Router,
    pub state: Arc<AppState>,
}

/// Account created directly in the database with a valid token in Redis.
pub struct TestUser {
    pub id: Uuid,
    pub email: String,
    pub token: String,
    pub refresh_token: String,
}

impl TestContext {
    /// Provisions the database, flushes Redis, loads the map and builds the router.
    pub async fn new() -> Self {
        let guard = test_lock().lock().await;

        let database_url = env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/aeroflot_test".to_owned()
        });
        let redis_url =
            env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/15".to_owned());

        let url_for_setup = database_url.clone();
        tokio::task::spawn_blocking(move || ensure_database(&url_for_setup))
            .await
            .expect("database setup task should not panic");

        let db_pool = establish_pg_connection(&database_url)
            .await
            .expect("postgres should be reachable; start it with `docker compose up -d postgres`");
        let redis_pool = establish_redis_connection(&redis_url)
            .await
            .expect("redis should be reachable; start it with `docker compose up -d redis`");

        run_migrations_and_reset(db_pool.clone(), redis_pool.clone()).await;

        let map_path = json_path("TEST_MAP_PATH", "assets/map2.json");
        let dataset_path = json_path(
            "TEST_DATASET_PATH",
            "assets/problem_classification_dataset.tsv",
        );

        let (catalog, cli_map_id) = crate::maps::load_catalog(&map_path)
            .unwrap_or_else(|error| panic!("failed to load maps from {map_path}: {error:#}"));
        let entry = catalog
            .get(&cli_map_id)
            .unwrap_or_else(|| panic!("map '{cli_map_id}' is missing from the catalog"));
        let active = crate::maps::load_active(entry)
            .unwrap_or_else(|error| panic!("failed to load map '{cli_map_id}': {error:#}"));
        {
            let mut redis_conn = redis_pool.get().expect("redis connection");
            crate::maps::store_active(&mut redis_conn, &active.id)
                .expect("failed to store the active map id");
        }

        let classifier =
            crate::types::classifier::ProblemClassifier::train_from_dataset(&dataset_path)
                .unwrap_or_else(|error| {
                    panic!("failed to train the classifier from {dataset_path}: {error:#}")
                });

        let state = Arc::new(AppState {
            catalog,
            active: Arc::new(RwLock::new(active)),
            db_pool,
            redis_pool,
            jwt_secret: "integration-test-secret".to_owned(),
            dataset_path: PathBuf::from(&dataset_path),
            classifier: Arc::new(RwLock::new(classifier)),
        });
        let app = axum::Router::new().nest("/api", crate::build_api_router(Arc::clone(&state)));

        Self {
            _guard: guard,
            app,
            state,
        }
    }

    /// Sends a request to the in-process router and returns status plus JSON body.
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let (status, body) = self.raw_request(method, path, token, body).await;
        let json = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&body).unwrap_or(Value::Null)
        };
        (status, json)
    }

    /// Sends a request and returns status plus the raw response body.
    pub async fn raw_request(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, String) {
        let content_type = body.is_some().then_some("application/json");
        let bytes = body
            .map(|value| serde_json::to_vec(&value).expect("request body serializes"))
            .unwrap_or_default();
        self.raw_bytes(method, path, token, bytes, content_type)
            .await
    }

    /// Sends a request with a literal body and, when given, an explicit
    /// `Content-Type`. `None` leaves the header out entirely; an empty string
    /// sends `Content-Type:` with no value.
    pub async fn raw_bytes(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> (StatusCode, String) {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = builder
            .body(Body::from(bytes))
            .expect("request should build");
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("router should answer");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    pub async fn get(&self, path: &str, token: &str) -> (StatusCode, Value) {
        self.request(Method::GET, path, Some(token), None).await
    }

    pub async fn post(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.request(Method::POST, path, Some(token), Some(body))
            .await
    }

    pub async fn put(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.request(Method::PUT, path, Some(token), Some(body))
            .await
    }

    /// Creates a dispatcher row and a valid access token in Redis.
    pub fn dispatcher(&self) -> TestUser {
        self.seed_user(UserRole::Dispatcher, None)
    }

    /// Creates an engineer row and a valid access token in Redis.
    pub fn engineer(&self, engineer_type: EngineerType) -> TestUser {
        self.seed_user(UserRole::Engineer, Some(engineer_type))
    }

    fn seed_user(&self, role: UserRole, engineer_type: Option<EngineerType>) -> TestUser {
        let id = Uuid::new_v4();
        let email = format!("{}-{id}@test.example", role.to_string());
        let password_hash = hash_password(PASSWORD.to_owned()).expect("password should hash");
        let mut conn = self.db();

        match &role {
            UserRole::Dispatcher => {
                diesel::insert_into(schema::dispatchers::table)
                    .values(&Dispatcher {
                        id,
                        name: "Test Dispatcher".to_owned(),
                        email: email.clone(),
                        password_hash,
                    })
                    .execute(&mut conn)
                    .expect("dispatcher should be inserted");
            }
            UserRole::Engineer => {
                diesel::insert_into(schema::engineers::table)
                    .values(&Engineer {
                        id,
                        email: email.clone(),
                        name: "Test Engineer".to_owned(),
                        password_hash,
                        engineer_type: engineer_type.expect("engineer needs a specialization"),
                    })
                    .execute(&mut conn)
                    .expect("engineer should be inserted");
            }
        }

        let (access, refresh) =
            create_token_pair(id, role.clone(), self.state.jwt_secret.as_bytes())
                .expect("token pair should be created");
        let mut redis_conn = self.redis();
        revoke_and_store_tokens(&mut redis_conn, id, &access, &refresh, &role)
            .expect("tokens should be stored in redis");

        TestUser {
            id,
            email,
            token: access,
            refresh_token: refresh,
        }
    }

    /// Seeds a task row and returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn seed_task(
        &self,
        created_by: Uuid,
        assigned_engineer: Option<Uuid>,
        status: &str,
        plane_id: Option<Uuid>,
        point: crate::types::map::Point,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let task = Task {
            id,
            description: "seeded task".to_owned(),
            created_at: now,
            ends_at: now + chrono::Duration::seconds(900),
            created_by,
            assigned_engineer,
            issue_type: "seeded issue".to_owned(),
            plane_x: point.0,
            plane_y: point.1,
            plane_id,
            task_route: "[]".to_owned(),
            is_active: true,
            map_id: Some(self.active_id()),
            status: status.to_owned(),
            recorded_at: now,
            ata_chapter: None,
            defect_text: None,
            mel_category: None,
            mel_defer_until: None,
            waiting_part: false,
            tools_picked: false,
            arrived_at: None,
            source: Some("test".to_owned()),
            urgency: "normal".to_owned(),
            eta_seconds: None,
            candidate_ms: None,
            sla_alerted: false,
            sla_breached: false,
        };
        diesel::insert_into(schema::tasks::table)
            .values(&task)
            .execute(&mut self.db())
            .expect("task should be inserted");
        id
    }

    pub fn active_id(&self) -> String {
        self.state
            .active
            .read()
            .expect("active map lock")
            .id
            .clone()
    }

    pub fn db(&self) -> PooledConnection<ConnectionManager<PgConnection>> {
        self.state.db_pool.get().expect("postgres connection")
    }

    pub fn redis(&self) -> PooledConnection<redis::Client> {
        self.state.redis_pool.get().expect("redis connection")
    }

    pub fn redis_get(&self, key: &str) -> Option<String> {
        self.redis().get(key).expect("redis GET should succeed")
    }

    pub fn redis_set(&self, key: &str, value: &str) {
        self.redis()
            .set(key, value)
            .expect("redis SET should succeed");
    }

    pub fn redis_exists(&self, key: &str) -> bool {
        self.redis()
            .exists(key)
            .expect("redis EXISTS should succeed")
    }

    pub fn redis_del(&self, key: &str) {
        let _: usize = self.redis().del(key).expect("redis DEL should succeed");
    }

    pub fn task(&self, id: Uuid) -> Task {
        schema::tasks::table
            .find(id)
            .select(Task::as_select())
            .first(&mut self.db())
            .expect("task should exist")
    }

    pub fn movement(&self, id: Uuid) -> Option<crate::models::aircraft_movement::AircraftMovement> {
        use crate::models::aircraft_movement::AircraftMovement;
        schema::aircraft_movements::table
            .find(id)
            .select(AircraftMovement::as_select())
            .first(&mut self.db())
            .optional()
            .expect("movement query should succeed")
    }

    pub fn journal_count(&self) -> i64 {
        schema::journal::table
            .count()
            .get_result(&mut self.db())
            .expect("journal count")
    }

    pub fn dispatcher_count(&self) -> i64 {
        schema::dispatchers::table
            .count()
            .get_result(&mut self.db())
            .expect("dispatcher count")
    }

    pub fn engineer_count(&self) -> i64 {
        schema::engineers::table
            .count()
            .get_result(&mut self.db())
            .expect("engineer count")
    }

    pub fn defect_count(&self) -> i64 {
        schema::defect_types::table
            .count()
            .get_result(&mut self.db())
            .expect("defect count")
    }

    pub fn dispatcher_row(&self, id: Uuid) -> Option<Dispatcher> {
        schema::dispatchers::table
            .find(id)
            .select(Dispatcher::as_select())
            .first(&mut self.db())
            .optional()
            .expect("dispatcher query")
    }

    pub fn engineer_row(&self, id: Uuid) -> Option<Engineer> {
        schema::engineers::table
            .find(id)
            .select(Engineer::as_select())
            .first(&mut self.db())
            .optional()
            .expect("engineer query")
    }
}

/// Asserts the status code and returns the body for further checks.
pub fn expect_status(response: (StatusCode, Value), expected: StatusCode) -> Value {
    let (status, body) = response;
    assert_eq!(
        status,
        expected,
        "unexpected status, body: {}",
        serde_json::to_string(&body).unwrap_or_default()
    );
    body
}

/// Valid registration body for a dispatcher.
pub fn dispatcher_registration(email: &str) -> Value {
    json!({
        "email": email,
        "name": "Integration Dispatcher",
        "password": PASSWORD,
        "user_role": "Dispatcher",
    })
}

/// Valid registration body for an engineer.
pub fn engineer_registration(email: &str, engineer_type: &str) -> Value {
    json!({
        "email": email,
        "name": "Integration Engineer",
        "password": PASSWORD,
        "user_role": "Engineer",
        "engineer_type": engineer_type,
    })
}

fn json_path(variable: &str, fallback: &str) -> String {
    env::var(variable).unwrap_or_else(|_| manifest_path(fallback))
}

fn manifest_path(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(relative)
        .to_string_lossy()
        .into_owned()
}

async fn run_migrations_and_reset(
    db_pool: Pool<ConnectionManager<PgConnection>>,
    redis_pool: Pool<redis::Client>,
) {
    tokio::task::spawn_blocking(move || {
        let mut conn = db_pool.get().expect("postgres connection");
        conn.run_pending_migrations(crate::MIGRATIONS)
            .expect("migrations should apply to the test database");
        diesel::sql_query(
            "TRUNCATE TABLE tasks, engineers, dispatchers, aircraft_movements, journal, defect_types CASCADE",
        )
        .execute(&mut conn)
        .expect("test tables should truncate");
    })
    .await
    .expect("migration task should not panic");

    let mut redis_conn = redis_pool.get().expect("redis connection");
    redis::cmd("FLUSHDB")
        .query::<()>(&mut redis_conn)
        .expect("test redis database should flush");
}

/// Creates the test database when it does not exist yet.
fn ensure_database(url: &str) {
    if PgConnection::establish(url).is_ok() {
        return;
    }

    let (base, database) = url
        .rsplit_once('/')
        .expect("TEST_DATABASE_URL must contain a database name");
    let database = database.split(['?', '#']).next().unwrap_or(database);
    assert!(
        !database.is_empty()
            && database
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'),
        "test database name must be a simple identifier, got '{database}'"
    );

    let maintenance_url = format!("{base}/postgres");
    let mut conn = PgConnection::establish(&maintenance_url).unwrap_or_else(|error| {
        panic!(
            "postgres is not reachable at {maintenance_url}: {error}. \
             Start the databases with `docker compose up -d postgres redis`."
        )
    });

    #[derive(QueryableByName)]
    struct DatabaseRow {
        #[diesel(sql_type = diesel::sql_types::Integer)]
        present: i32,
    }

    let exists = diesel::sql_query("SELECT 1 AS present FROM pg_database WHERE datname = $1")
        .bind::<diesel::sql_types::Text, _>(database)
        .load::<DatabaseRow>(&mut conn)
        .expect("pg_database should be readable")
        .into_iter()
        .any(|row| row.present == 1);

    if !exists {
        diesel::sql_query(format!("CREATE DATABASE \"{database}\""))
            .execute(&mut conn)
            .unwrap_or_else(|error| panic!("failed to create test database '{database}': {error}"));
    }
}
