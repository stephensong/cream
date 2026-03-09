use chrono::{DateTime, Utc};
use deadpool_postgres::Pool;
use tokio_postgres::types::ToSql;

use crate::contracts::ContractType;

#[derive(Debug, Clone)]
pub struct ContractRow {
    pub contract_instance_id: String,
    pub contract_key_bytes: Vec<u8>,
    pub contract_type: ContractType,
    pub parameters_bytes: Vec<u8>,
    pub state_bytes: Vec<u8>,
    pub state_json: serde_json::Value,
    pub wasm_code: Option<Vec<u8>>,
    pub code_hash: Vec<u8>,
}

#[derive(Debug, serde::Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: DateTime<Utc>,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub old_state: Option<serde_json::Value>,
    pub new_state: Option<serde_json::Value>,
    pub update_data: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
}

impl ContractType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContractType::Directory => "directory",
            ContractType::Storefront => "storefront",
            ContractType::UserContract => "user_contract",
            ContractType::Inbox => "inbox",
            ContractType::MarketDirectory => "market_directory",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "directory" => Some(ContractType::Directory),
            "storefront" => Some(ContractType::Storefront),
            "user_contract" => Some(ContractType::UserContract),
            "inbox" => Some(ContractType::Inbox),
            "market_directory" => Some(ContractType::MarketDirectory),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Store {
    pool: Pool,
}

impl Store {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub async fn run_migrations(&self) -> Result<(), anyhow::Error> {
        let client = self.pool.get().await?;

        // Run migrations idempotently
        let migration = include_str!("../migrations/001_initial.sql");

        // Split by semicolons and execute each statement, ignoring "already exists" errors
        for stmt in migration.split(';') {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            match client.execute(&format!("{stmt};"), &[]).await {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("already exists") || msg.contains("duplicate") {
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        Ok(())
    }

    pub async fn get_contract(&self, instance_id: &str) -> Result<Option<ContractRow>, anyhow::Error> {
        let client = self.pool.get().await?;

        let row = client
            .query_opt(
                "SELECT contract_instance_id, contract_key_bytes, contract_type,
                    parameters_bytes, state_bytes, state_json, wasm_code, code_hash
                FROM contracts WHERE contract_instance_id = $1",
                &[&instance_id],
            )
            .await?;

        Ok(row.map(|r| ContractRow {
            contract_instance_id: r.get(0),
            contract_key_bytes: r.get(1),
            contract_type: ContractType::from_str(r.get::<_, &str>(2)).unwrap_or(ContractType::Directory),
            parameters_bytes: r.get(3),
            state_bytes: r.get(4),
            state_json: r.get(5),
            wasm_code: r.get(6),
            code_hash: r.get(7),
        }))
    }

    pub async fn put_contract(&self, row: &ContractRow) -> Result<(), anyhow::Error> {
        let mut client = self.pool.get().await?;

        let ct = row.contract_type.as_str();

        let tx = client.transaction().await?;

        tx.execute(
            "INSERT INTO contracts (
                contract_instance_id, contract_key_bytes, contract_type,
                parameters_bytes, state_bytes, state_json, wasm_code, code_hash
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (contract_instance_id) DO UPDATE SET
                state_bytes = EXCLUDED.state_bytes,
                state_json = EXCLUDED.state_json,
                updated_at = NOW()",
            &[
                &row.contract_instance_id,
                &row.contract_key_bytes,
                &ct,
                &row.parameters_bytes,
                &row.state_bytes,
                &row.state_json,
                &row.wasm_code as &(dyn ToSql + Sync),
                &row.code_hash,
            ],
        )
        .await?;

        tx.execute(
            "INSERT INTO audit_log (entity_type, entity_id, action, new_state, metadata)
            VALUES ($1, $2, 'put', $3, '{}')",
            &[&ct, &row.contract_instance_id, &row.state_json],
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Atomically load contract state (with row lock), apply a merge function,
    /// and persist the result + audit entry. Prevents lost updates under concurrency.
    pub async fn get_and_update_contract<F>(
        &self,
        instance_id: &str,
        merge_fn: F,
    ) -> Result<(ContractRow, Vec<u8>), anyhow::Error>
    where
        F: FnOnce(&ContractRow) -> Result<Vec<u8>, anyhow::Error>,
    {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;

        let row = tx
            .query_opt(
                "SELECT contract_instance_id, contract_key_bytes, contract_type,
                    parameters_bytes, state_bytes, state_json, wasm_code, code_hash
                FROM contracts WHERE contract_instance_id = $1 FOR UPDATE",
                &[&instance_id],
            )
            .await?;

        let row = match row {
            Some(r) => ContractRow {
                contract_instance_id: r.get(0),
                contract_key_bytes: r.get(1),
                contract_type: ContractType::from_str(r.get::<_, &str>(2))
                    .unwrap_or(ContractType::Directory),
                parameters_bytes: r.get(3),
                state_bytes: r.get(4),
                state_json: r.get(5),
                wasm_code: r.get(6),
                code_hash: r.get(7),
            },
            None => anyhow::bail!("contract not found: {instance_id}"),
        };

        let new_state_bytes = merge_fn(&row)?;
        let new_state_json = serde_json::from_slice::<serde_json::Value>(&new_state_bytes)
            .unwrap_or(serde_json::Value::Null);
        let ct = row.contract_type.as_str();

        tx.execute(
            "UPDATE contracts SET state_bytes = $1, state_json = $2, updated_at = NOW()
            WHERE contract_instance_id = $3",
            &[
                &new_state_bytes.as_slice(),
                &new_state_json,
                &instance_id,
            ],
        )
        .await?;

        tx.execute(
            "INSERT INTO audit_log (entity_type, entity_id, action, old_state, new_state, metadata)
            VALUES ($1, $2, 'update', $3, $4, '{}')",
            &[&ct, &instance_id, &row.state_json, &new_state_json],
        )
        .await?;

        tx.commit().await?;
        Ok((row, new_state_bytes))
    }

    pub async fn audit_as_at(
        &self,
        before: DateTime<Utc>,
        entity_type: Option<&str>,
    ) -> Result<Vec<AuditEntry>, anyhow::Error> {
        let client = self.pool.get().await?;

        let rows = if let Some(et) = entity_type {
            client
                .query(
                    "SELECT id, ts, entity_type, entity_id, action,
                        old_state, new_state, update_data, metadata
                    FROM audit_log
                    WHERE ts <= $1 AND entity_type = $2
                    ORDER BY ts ASC",
                    &[&before, &et],
                )
                .await?
        } else {
            client
                .query(
                    "SELECT id, ts, entity_type, entity_id, action,
                        old_state, new_state, update_data, metadata
                    FROM audit_log
                    WHERE ts <= $1
                    ORDER BY ts ASC",
                    &[&before],
                )
                .await?
        };

        Ok(rows
            .into_iter()
            .map(|r| AuditEntry {
                id: r.get(0),
                ts: r.get(1),
                entity_type: r.get(2),
                entity_id: r.get(3),
                action: r.get(4),
                old_state: r.get(5),
                new_state: r.get(6),
                update_data: r.get(7),
                metadata: r.get(8),
            })
            .collect())
    }
}
