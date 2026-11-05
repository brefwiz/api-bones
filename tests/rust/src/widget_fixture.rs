// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! PostgreSQL owns organization isolation for the batch contract fixture.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor, wait::LogWaitStrategy},
    runners::AsyncRunner,
};
use tokio_postgres::{Client, Config, NoTls, Transaction};

static SCENARIO_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct BatchDb {
    client: tokio::sync::Mutex<Client>,
    admin: tokio::sync::Mutex<Client>,
    reader_pid: i32,
    _cleanup: DatabaseCleanup,
}

struct DatabaseCleanup {
    config: Config,
    database: String,
    role: String,
    connections: Vec<tokio::task::JoinHandle<()>>,
    _container: Option<ContainerAsync<GenericImage>>,
}

impl Drop for DatabaseCleanup {
    fn drop(&mut self) {
        for connection in &self.connections {
            connection.abort();
        }
        let config = self.config.clone();
        let database = self.database.clone();
        let role = self.role.clone();
        // TwoPass owns its hooks. Join cleanup on an independent runtime so
        // scenario teardown finishes even on its current-thread shard runtimes.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("fixture cleanup runtime")
                .block_on(async move {
                    let (admin, connection) =
                        config.connect(NoTls).await.expect("cleanup admin connects");
                    let connection = tokio::spawn(connection);
                    admin
                        .batch_execute(&format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)"))
                        .await
                        .expect("scenario database drops");
                    admin
                        .batch_execute(&format!("DROP ROLE IF EXISTS {role}"))
                        .await
                        .expect("scenario reader role drops");
                    drop(admin);
                    connection
                        .await
                        .expect("cleanup connection task")
                        .expect("cleanup connection closes");
                });
        })
        .join()
        .expect("fixture database cleanup completes");
    }
}

struct StoredWidget {
    name: String,
    owner: Option<String>,
    parts: Vec<String>,
    revision: String,
}

impl BatchDb {
    pub async fn seeded(
        widgets: &HashMap<(String, String), String>,
        parts: &HashMap<(String, String), Vec<String>>,
        owners: &HashMap<(String, String), String>,
    ) -> Self {
        let suffix = format!(
            "{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
            SCENARIO_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let password = format!("p{suffix}");
        let database = format!("batch_{suffix}");
        let reader = format!("reader_{suffix}");
        let (mut config, container) = if std::env::var("SKIP_TESTCONTAINERS").as_deref() == Ok("1")
        {
            let url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL is required when SKIP_TESTCONTAINERS=1");
            (
                url.parse::<Config>()
                    .expect("DATABASE_URL must be a valid PostgreSQL connection configuration"),
                None,
            )
        } else {
            let container = GenericImage::new("postgres", "17-alpine")
                .with_exposed_port(5432.tcp())
                .with_wait_for(WaitFor::log(
                    LogWaitStrategy::stdout_or_stderr(
                        "database system is ready to accept connections",
                    )
                    .with_times(2),
                ))
                .with_env_var("POSTGRES_PASSWORD", &password)
                .start()
                .await
                .expect("real PostgreSQL starts");
            let host = container.get_host().await.expect("PostgreSQL host");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("PostgreSQL port");
            let mut config = Config::new();
            config
                .host(host.to_string())
                .port(port)
                .user("postgres")
                .password(&password)
                .dbname("postgres");
            (config, Some(container))
        };
        let (owner, connection) = config.connect(NoTls).await.expect("cluster admin connects");
        // Install the guard before allocating resources so failed setup also cleans up.
        let mut cleanup = DatabaseCleanup {
            config: config.clone(),
            database: database.clone(),
            role: reader.clone(),
            connections: vec![tokio::spawn(async move {
                connection.await.expect("cluster admin connection");
            })],
            _container: container,
        };
        owner
            .batch_execute(&format!("CREATE DATABASE {database}"))
            .await
            .expect("scenario database creates");
        owner
            .batch_execute(&format!(
                "CREATE ROLE {reader} LOGIN PASSWORD '{password}' NOSUPERUSER NOBYPASSRLS"
            ))
            .await
            .expect("scenario reader creates");
        config.dbname(&database);
        let (admin, connection) = config.connect(NoTls).await.expect("admin connects");
        cleanup.connections.push(tokio::spawn(async move {
            connection.await.expect("admin connection");
        }));
        admin.batch_execute(&format!("
            CREATE TABLE widgets(org_id text NOT NULL, id text NOT NULL, name text NOT NULL, owner text, PRIMARY KEY(org_id,id));
            CREATE TABLE parts(org_id text NOT NULL, widget_id text NOT NULL, position integer NOT NULL, value text NOT NULL);
            ALTER TABLE widgets ENABLE ROW LEVEL SECURITY;
            ALTER TABLE widgets FORCE ROW LEVEL SECURITY;
            ALTER TABLE parts ENABLE ROW LEVEL SECURITY;
            ALTER TABLE parts FORCE ROW LEVEL SECURITY;
            CREATE POLICY widgets_org ON widgets USING(org_id = current_setting('app.org_id', true));
            CREATE POLICY parts_org ON parts USING(org_id = current_setting('app.org_id', true));
            GRANT SELECT, UPDATE ON widgets TO {reader};
            GRANT SELECT, INSERT, DELETE ON parts TO {reader};
        ")).await.expect("RLS schema installs");
        for ((org, id), name) in widgets {
            let owner = owners.get(&(org.clone(), id.clone()));
            admin
                .execute(
                    "INSERT INTO widgets VALUES($1,$2,$3,$4)",
                    &[org, id, name, &owner],
                )
                .await
                .expect("widget seeded");
        }
        for ((org, id), parts) in parts {
            for (position, value) in parts.iter().enumerate() {
                let position = i32::try_from(position).expect("fixture position");
                admin
                    .execute(
                        "INSERT INTO parts VALUES($1,$2,$3,$4)",
                        &[org, id, &position, value],
                    )
                    .await
                    .expect("part seeded");
            }
        }
        config.user(&reader).password(&password);
        let (client, connection) = config
            .connect(NoTls)
            .await
            .expect("non-bypass reader connects");
        cleanup.connections.push(tokio::spawn(async move {
            connection.await.expect("reader connection");
        }));
        let role = client.query_one("SELECT current_user, rolsuper, rolbypassrls FROM pg_roles WHERE rolname=current_user", &[]).await.expect("reader role");
        assert_eq!(role.get::<_, String>(0), reader);
        assert!(!role.get::<_, bool>(1));
        assert!(!role.get::<_, bool>(2));
        let forced = client.query("SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE relname IN ('widgets','parts')", &[]).await.expect("RLS flags");
        assert_eq!(forced.len(), 2);
        assert!(
            forced
                .iter()
                .all(|row| row.get::<_, bool>(0) && row.get::<_, bool>(1))
        );
        let reader_pid = client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .expect("reader backend")
            .get(0);
        Self {
            client: tokio::sync::Mutex::new(client),
            admin: tokio::sync::Mutex::new(admin),
            reader_pid,
            _cleanup: cleanup,
        }
    }

    async fn organization(tx: &Transaction<'_>, org: &str) {
        tx.query_one("SELECT set_config('app.org_id', $1, true)", &[&org])
            .await
            .expect("transaction organization");
    }

    pub async fn widget(&self, org: &str, id: &str) -> Option<(String, Option<String>)> {
        self.read(org, id).await.map(|row| (row.name, row.owner))
    }

    pub async fn owner(&self, org: &str, id: &str) -> Option<String> {
        self.widget(org, id).await.and_then(|(_, owner)| owner)
    }

    pub async fn lock_reads(&self) {
        self.admin
            .lock()
            .await
            .batch_execute("BEGIN; LOCK TABLE widgets IN ACCESS EXCLUSIVE MODE")
            .await
            .expect("administrator holds the widget table lock");
    }

    pub async fn unlock_reads(&self) {
        self.admin
            .lock()
            .await
            .batch_execute("COMMIT")
            .await
            .expect("administrator releases the widget table lock");
    }

    pub async fn wait_for_blocked_read(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                self.admin
                    .lock()
                    .await
                    .query_one("SELECT pg_stat_clear_snapshot()", &[])
                    .await
                    .expect("refresh PostgreSQL activity observations");
                let blocked: bool = self
                    .admin
                    .lock()
                    .await
                    .query_one(
                        "SELECT EXISTS(SELECT FROM pg_stat_activity WHERE pid=$1 AND wait_event_type='Lock')",
                        &[&self.reader_pid],
                    )
                    .await
                    .expect("observe the reader in PostgreSQL")
                    .get(0);
                if blocked {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the real database read reaches its table lock");
    }

    async fn snapshot(tx: &Transaction<'_>, id: &str) -> Option<StoredWidget> {
        tx.query_opt(
            "SELECT w.name, w.owner, w.xmin::text, ARRAY(SELECT p.value FROM parts p WHERE p.widget_id=w.id ORDER BY p.position) FROM widgets w WHERE w.id=$1",
            &[&id],
        )
        .await
        .expect("RLS parts snapshot")
        .map(|row| StoredWidget { name: row.get(0), owner: row.get(1), revision: row.get(2), parts: row.get(3) })
    }

    async fn read(&self, org: &str, id: &str) -> Option<StoredWidget> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.expect("snapshot transaction");
        Self::organization(&tx, org).await;
        let snapshot = Self::snapshot(&tx, id).await;
        tx.commit().await.expect("snapshot commits");
        snapshot
    }

    pub async fn parts_snapshot(&self, org: &str, id: &str) -> Option<(Vec<String>, String)> {
        self.read(org, id)
            .await
            .map(|row| (row.parts, row.revision))
    }

    pub async fn authorized_parts_snapshot(
        &self,
        org: &str,
        id: &str,
        authorize: impl FnOnce(Option<&str>) -> Result<(), connectrpc::ConnectError>,
    ) -> Result<Option<(Vec<String>, String)>, connectrpc::ConnectError> {
        let Some(row) = self.read(org, id).await else {
            return Ok(None);
        };
        authorize(row.owner.as_deref())?;
        Ok(Some((row.parts, row.revision)))
    }

    pub async fn replace_parts(
        &self,
        org: &str,
        id: &str,
        items: &[String],
        owner: Option<&str>,
        authorize: impl FnOnce(Option<&str>) -> Result<(), connectrpc::ConnectError>,
    ) -> Result<Option<(Vec<String>, String)>, connectrpc::ConnectError> {
        let mut client = self.client.lock().await;
        let tx = client.transaction().await.expect("update transaction");
        Self::organization(&tx, org).await;
        let widget = tx
            .query_opt("SELECT owner FROM widgets WHERE id=$1 FOR UPDATE", &[&id])
            .await
            .expect("RLS widget update lock");
        let Some(widget) = widget else {
            tx.rollback().await.expect("absent widget rolls back");
            return Ok(None);
        };
        let current_owner: Option<String> = widget.get(0);
        if let Err(error) = authorize(current_owner.as_deref()) {
            tx.rollback().await.expect("unauthorized update rolls back");
            return Err(error);
        }
        tx.execute("DELETE FROM parts WHERE widget_id=$1", &[&id])
            .await
            .expect("RLS parts replacement");
        for (position, value) in items.iter().enumerate() {
            let position = i32::try_from(position).expect("part position");
            tx.execute(
                "INSERT INTO parts VALUES($1,$2,$3,$4)",
                &[&org, &id, &position, value],
            )
            .await
            .expect("RLS replacement part");
        }
        tx.execute(
            "UPDATE widgets SET owner=COALESCE($2,owner),name=name WHERE id=$1",
            &[&id, &owner],
        )
        .await
        .expect("widget revision advances with its parts");
        let saved = Self::snapshot(&tx, id).await;
        tx.commit().await.expect("parts update commits");
        Ok(saved.map(|row| (row.parts, row.revision)))
    }

    pub async fn parts(&self, org: &str, id: &str) -> Vec<String> {
        self.read(org, id)
            .await
            .map_or_else(Vec::new, |row| row.parts)
    }
}
