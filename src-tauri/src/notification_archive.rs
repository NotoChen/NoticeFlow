use crate::notification_db::NotificationRecord;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{
    params, params_from_iter,
    types::{Type, Value as SqlValue},
    Connection, OpenFlags, OptionalExtension,
};
use serde::Serialize;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionHistoryEntry {
    pub id: String,
    pub timestamp: String,
    pub queue_id: Option<String>,
    pub rule_id: String,
    pub rule_name: String,
    pub notification_id: i64,
    pub notification_title: String,
    pub app_identifier: String,
    pub action_index: u32,
    pub action_type: String,
    pub success: bool,
    pub message: String,
    pub output: Option<String>,
    pub origin: String,
    pub duration_ms: u64,
    pub attempt_count: u32,
    pub variables_json: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveStats {
    pub path: String,
    pub size_bytes: u64,
    pub notification_count: u64,
    pub hidden_count: u64,
    pub system_deleted_count: u64,
    pub action_history_count: u64,
    pub system_delete_audit_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDeleteAuditEntry {
    pub id: String,
    pub timestamp: String,
    pub record_id: i64,
    pub app_identifier: String,
    pub app_name: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub system_rows_deleted: u64,
}

pub fn archive_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(crate::app_settings::app_data_dir()?.join("notifications.sqlite"))
}

pub fn upsert_records(records: &[NotificationRecord]) -> Result<(), Box<dyn Error>> {
    if records.is_empty() {
        return Ok(());
    }
    let mut connection = open_archive_connection()?;
    upsert_records_in_connection(&mut connection, records)
}

fn upsert_records_in_connection(
    connection: &mut Connection,
    records: &[NotificationRecord],
) -> Result<(), Box<dyn Error>> {
    if records.is_empty() {
        return Ok(());
    }
    let transaction = connection.transaction()?;
    for record in records {
        transaction.execute(
            r#"
            insert into notifications (
                record_id,
                app_identifier,
                app_name,
                delivered_at,
                title,
                subtitle,
                body,
                archived_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            on conflict(record_id) do update set
                app_identifier = excluded.app_identifier,
                app_name = excluded.app_name,
                delivered_at = excluded.delivered_at,
                title = excluded.title,
                subtitle = excluded.subtitle,
                body = excluded.body,
                hidden = case
                    when notifications.app_identifier = excluded.app_identifier
                     and notifications.delivered_at = excluded.delivered_at
                     and notifications.title = excluded.title
                     and notifications.subtitle = excluded.subtitle
                     and notifications.body = excluded.body
                    then notifications.hidden
                    else 0
                end,
                system_deleted = case
                    when notifications.app_identifier = excluded.app_identifier
                     and notifications.delivered_at = excluded.delivered_at
                     and notifications.title = excluded.title
                     and notifications.subtitle = excluded.subtitle
                     and notifications.body = excluded.body
                    then notifications.system_deleted
                    else 0
                end,
                archived_at = case
                    when notifications.app_identifier = excluded.app_identifier
                     and notifications.delivered_at = excluded.delivered_at
                     and notifications.title = excluded.title
                     and notifications.subtitle = excluded.subtitle
                     and notifications.body = excluded.body
                    then notifications.archived_at
                    else excluded.archived_at
                end
            "#,
            params![
                record.id,
                record.app_identifier,
                record.app_name,
                record.delivered_at.to_rfc3339(),
                record.title,
                record.subtitle,
                record.body,
                Utc::now().to_rfc3339()
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn recent_records_excluding(
    limit: usize,
    ignored_app_identifiers: &[String],
) -> Result<Vec<NotificationRecord>, Box<dyn Error>> {
    let ignored_app_identifiers = normalized_app_identifiers(ignored_app_identifiers);
    let connection = open_archive_connection()?;
    let mut records = if ignored_app_identifiers.is_empty() {
        let mut statement = connection.prepare(
            r#"
            select record_id, app_identifier, app_name, delivered_at, title, subtitle, body
            from notifications
            where hidden = 0 and system_deleted = 0
              and julianday(delivered_at) is not null
            order by delivered_at desc, record_id desc
            limit ?1
            "#,
        )?;
        let rows = statement
            .query_map([limit as i64], archive_record_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    } else {
        let placeholders = std::iter::repeat_n("?", ignored_app_identifiers.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            select record_id, app_identifier, app_name, delivered_at, title, subtitle, body
            from notifications
            where hidden = 0 and system_deleted = 0
              and julianday(delivered_at) is not null
              and lower(app_identifier) not in ({placeholders})
            order by delivered_at desc, record_id desc
            limit ?
            "#
        );
        let mut params = ignored_app_identifiers
            .into_iter()
            .map(SqlValue::Text)
            .collect::<Vec<_>>();
        params.push(SqlValue::Integer(limit as i64));
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(params), archive_record_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    records.reverse();
    Ok(records)
}

pub fn recent_records_including(
    limit: usize,
    app_identifiers: &[String],
) -> Result<Vec<NotificationRecord>, Box<dyn Error>> {
    let app_identifiers = normalized_app_identifiers(app_identifiers);
    if app_identifiers.is_empty() {
        return Ok(Vec::new());
    }

    let connection = open_archive_connection()?;
    let placeholders = std::iter::repeat_n("?", app_identifiers.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        select record_id, app_identifier, app_name, delivered_at, title, subtitle, body
        from notifications
        where hidden = 0 and system_deleted = 0
          and julianday(delivered_at) is not null
          and lower(app_identifier) in ({placeholders})
        order by delivered_at desc, record_id desc
        limit ?
        "#
    );
    let mut params = app_identifiers
        .into_iter()
        .map(SqlValue::Text)
        .collect::<Vec<_>>();
    params.push(SqlValue::Integer(limit as i64));
    let mut statement = connection.prepare(&sql)?;
    let mut records = statement
        .query_map(params_from_iter(params), archive_record_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    records.reverse();
    Ok(records)
}

pub fn record_by_id(record_id: i64) -> Result<Option<NotificationRecord>, Box<dyn Error>> {
    let connection = open_archive_connection()?;
    connection
        .query_row(
            r#"
            select record_id, app_identifier, app_name, delivered_at, title, subtitle, body
            from notifications
            where record_id = ?1
              and julianday(delivered_at) is not null
            limit 1
            "#,
            [record_id],
            archive_record_from_row,
        )
        .optional()
        .map_err(Into::into)
}

pub fn hide_record(record_id: i64) -> Result<usize, Box<dyn Error>> {
    let connection = open_archive_connection()?;
    let changed = connection.execute(
        "update notifications set hidden = 1 where record_id = ?1",
        [record_id],
    )?;
    Ok(changed)
}

pub fn clear_hidden_records() -> Result<(), Box<dyn Error>> {
    let connection = open_archive_connection()?;
    clear_hidden_records_in_connection(&connection)?;
    Ok(())
}

pub fn mark_record_system_deleted(record_id: i64) -> Result<usize, Box<dyn Error>> {
    let connection = open_archive_connection()?;
    let changed = connection.execute(
        "update notifications set hidden = 1, system_deleted = 1 where record_id = ?1",
        [record_id],
    )?;
    Ok(changed)
}

pub fn append_action_history(entries: &[ActionHistoryEntry]) -> Result<(), Box<dyn Error>> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut connection = open_archive_connection()?;
    let transaction = connection.transaction()?;
    for entry in entries {
        transaction.execute(
            r#"
            insert into action_history (
                id,
                timestamp,
                queue_id,
                rule_id,
                rule_name,
                notification_id,
                notification_title,
                app_identifier,
                action_index,
                action_type,
                success,
                message,
                output,
                origin,
                duration_ms,
                attempt_count,
                variables_json
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            "#,
            params![
                entry.id,
                entry.timestamp,
                entry.queue_id,
                entry.rule_id,
                entry.rule_name,
                entry.notification_id,
                entry.notification_title,
                entry.app_identifier,
                entry.action_index as i64,
                entry.action_type,
                if entry.success { 1_i64 } else { 0_i64 },
                entry.message,
                entry.output,
                entry.origin,
                entry.duration_ms as i64,
                entry.attempt_count as i64,
                entry.variables_json,
            ],
        )?;
    }
    transaction.execute(
        r#"
        delete from action_history
        where rowid in (
            select rowid
            from action_history
            order by timestamp desc, rowid desc
            limit -1 offset 500
        )
        "#,
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn recent_action_history(limit: usize) -> Result<Vec<ActionHistoryEntry>, Box<dyn Error>> {
    let connection = open_archive_connection()?;
    let mut statement = connection.prepare(
        r#"
        select id, timestamp, queue_id, rule_id, rule_name, notification_id, notification_title,
               app_identifier, action_index, action_type, success, message, duration_ms,
               attempt_count, variables_json, output, origin
        from action_history
        order by timestamp desc, rowid desc
        limit ?1
        "#,
    )?;
    let rows = statement
        .query_map([limit as i64], action_history_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn clear_action_history() -> Result<(), Box<dyn Error>> {
    let connection = open_archive_connection()?;
    connection.execute("delete from action_history", [])?;
    Ok(())
}

pub fn archive_stats() -> Result<ArchiveStats, Box<dyn Error>> {
    let path = archive_path()?;
    let connection = open_archive_connection()?;
    let size_bytes = fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(ArchiveStats {
        path: path.to_string_lossy().to_string(),
        size_bytes,
        notification_count: count_rows(&connection, "notifications", None)?,
        hidden_count: count_rows(&connection, "notifications", Some("hidden = 1"))?,
        system_deleted_count: count_rows(&connection, "notifications", Some("system_deleted = 1"))?,
        action_history_count: count_rows(&connection, "action_history", None)?,
        system_delete_audit_count: count_rows(&connection, "system_delete_audit", None)?,
    })
}

pub fn compact_archive() -> Result<(), Box<dyn Error>> {
    let connection = open_archive_connection()?;
    connection.execute_batch("vacuum;")?;
    Ok(())
}

pub fn prune_archive(notification_retention_days: u64) -> Result<(), Box<dyn Error>> {
    let cutoff = Utc::now() - Duration::days(notification_retention_days.clamp(1, 3650) as i64);
    let connection = open_archive_connection()?;
    prune_archive_in_connection(&connection, cutoff)
}

fn prune_archive_in_connection(
    connection: &Connection,
    cutoff: DateTime<Utc>,
) -> Result<(), Box<dyn Error>> {
    connection.execute(
        "delete from notifications where julianday(delivered_at) is null or julianday(delivered_at) < julianday(?1)",
        [cutoff.to_rfc3339()],
    )?;
    Ok(())
}

pub fn append_system_delete_audit(
    record: &NotificationRecord,
    system_rows_deleted: usize,
) -> Result<(), Box<dyn Error>> {
    let connection = open_archive_connection()?;
    connection.execute(
        r#"
        insert into system_delete_audit (
            id, timestamp, record_id, app_identifier, app_name, title, subtitle, body,
            system_rows_deleted
        )
        values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            Uuid::new_v4().to_string(),
            Utc::now().to_rfc3339(),
            record.id,
            record.app_identifier,
            record.app_name,
            record.title,
            record.subtitle,
            record.body,
            system_rows_deleted as i64,
        ],
    )?;
    connection.execute(
        r#"
        delete from system_delete_audit
        where rowid in (
            select rowid
            from system_delete_audit
            order by timestamp desc, rowid desc
            limit -1 offset 500
        )
        "#,
        [],
    )?;
    Ok(())
}

pub fn recent_system_delete_audit(
    limit: usize,
) -> Result<Vec<SystemDeleteAuditEntry>, Box<dyn Error>> {
    let connection = open_archive_connection()?;
    let mut statement = connection.prepare(
        r#"
        select id, timestamp, record_id, app_identifier, app_name, title, subtitle, body,
               system_rows_deleted
        from system_delete_audit
        order by timestamp desc, rowid desc
        limit ?1
        "#,
    )?;
    let rows = statement
        .query_map([limit as i64], system_delete_audit_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn open_archive_connection() -> Result<Connection, Box<dyn Error>> {
    let path = archive_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_millis(1_000))?;
    initialize_archive_schema(&connection)?;
    Ok(connection)
}

fn initialize_archive_schema(connection: &Connection) -> Result<(), Box<dyn Error>> {
    connection.execute_batch(
        r#"
        create table if not exists notifications (
            record_id integer primary key,
            app_identifier text not null,
            app_name text not null,
            delivered_at text not null,
            title text not null,
            subtitle text not null,
            body text not null,
            hidden integer not null default 0,
            system_deleted integer not null default 0,
            archived_at text not null
        );
        create index if not exists idx_notifications_app on notifications(app_identifier);
        create index if not exists idx_notifications_hidden_record on notifications(hidden, record_id);

        create table if not exists action_history (
            id text primary key,
            timestamp text not null,
            queue_id text,
            rule_id text not null,
            rule_name text not null,
            notification_id integer not null,
            notification_title text not null,
            app_identifier text not null,
            action_index integer not null default 0,
            action_type text not null,
            success integer not null,
            message text not null,
            output text,
            origin text not null default 'auto',
            duration_ms integer not null,
            attempt_count integer not null default 1,
            variables_json text
        );
        create index if not exists idx_action_history_timestamp on action_history(timestamp);

        create table if not exists system_delete_audit (
            id text primary key,
            timestamp text not null,
            record_id integer not null,
            app_identifier text not null,
            app_name text not null,
            title text not null,
            subtitle text not null,
            body text not null,
            system_rows_deleted integer not null
        );
        create index if not exists idx_system_delete_audit_timestamp on system_delete_audit(timestamp);
        "#,
    )?;
    ensure_column(
        connection,
        "notifications",
        "system_deleted",
        "integer not null default 0",
    )?;
    ensure_column(connection, "action_history", "queue_id", "text")?;
    ensure_column(
        connection,
        "action_history",
        "action_index",
        "integer not null default 0",
    )?;
    ensure_column(
        connection,
        "action_history",
        "attempt_count",
        "integer not null default 1",
    )?;
    ensure_column(connection, "action_history", "variables_json", "text")?;
    ensure_column(connection, "action_history", "output", "text")?;
    ensure_column(
        connection,
        "action_history",
        "origin",
        "text not null default 'auto'",
    )?;
    Ok(())
}

fn clear_hidden_records_in_connection(connection: &Connection) -> rusqlite::Result<usize> {
    connection.execute(
        "update notifications set hidden = 0 where hidden = 1 and system_deleted = 0",
        [],
    )
}

fn ensure_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
    column_definition: &str,
) -> Result<(), Box<dyn Error>> {
    let mut statement = connection.prepare(&format!("pragma table_info({table_name})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.iter().any(|column| column == column_name) {
        return Ok(());
    }
    connection.execute(
        &format!("alter table {table_name} add column {column_name} {column_definition}"),
        [],
    )?;
    Ok(())
}

fn archive_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NotificationRecord> {
    let delivered_at: String = row.get(3)?;
    let delivered_at = DateTime::parse_from_rfc3339(&delivered_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(error))
        })?;
    Ok(NotificationRecord {
        id: row.get(0)?,
        app_identifier: row.get(1)?,
        app_name: row.get(2)?,
        delivered_at,
        title: row.get(4)?,
        subtitle: row.get(5)?,
        body: row.get(6)?,
    })
}

fn system_delete_audit_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<SystemDeleteAuditEntry> {
    let system_rows_deleted: i64 = row.get(8)?;
    Ok(SystemDeleteAuditEntry {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        record_id: row.get(2)?,
        app_identifier: row.get(3)?,
        app_name: row.get(4)?,
        title: row.get(5)?,
        subtitle: row.get(6)?,
        body: row.get(7)?,
        system_rows_deleted: system_rows_deleted.max(0) as u64,
    })
}

fn action_history_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionHistoryEntry> {
    let duration_ms: i64 = row.get(12)?;
    let action_index: i64 = row.get(8)?;
    let attempt_count: i64 = row.get(13)?;
    Ok(ActionHistoryEntry {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        queue_id: row.get(2)?,
        rule_id: row.get(3)?,
        rule_name: row.get(4)?,
        notification_id: row.get(5)?,
        notification_title: row.get(6)?,
        app_identifier: row.get(7)?,
        action_index: action_index.max(0) as u32,
        action_type: row.get(9)?,
        success: row.get::<_, i64>(10)? != 0,
        message: row.get(11)?,
        duration_ms: duration_ms.max(0) as u64,
        attempt_count: attempt_count.max(1) as u32,
        variables_json: row.get(14)?,
        output: row.get(15)?,
        origin: row
            .get::<_, Option<String>>(16)?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "auto".to_string()),
    })
}

fn count_rows(
    connection: &Connection,
    table_name: &str,
    where_clause: Option<&str>,
) -> rusqlite::Result<u64> {
    let sql = match where_clause {
        Some(where_clause) => format!("select count(*) from {table_name} where {where_clause}"),
        None => format!("select count(*) from {table_name}"),
    };
    let count = connection.query_row(&sql, [], |row| row.get::<_, i64>(0))?;
    Ok(count.max(0) as u64)
}

fn normalized_app_identifiers(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn normalized_app_identifiers_removes_blank_values() {
        assert_eq!(
            normalized_app_identifiers(&[" com.example.App ".to_string(), "".to_string()]),
            vec!["com.example.app".to_string()]
        );
    }

    #[test]
    fn schema_migration_adds_system_deleted_column() {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        connection
            .execute_batch(
                r#"
                create table notifications (
                    record_id integer primary key,
                    app_identifier text not null,
                    app_name text not null,
                    delivered_at text not null,
                    title text not null,
                    subtitle text not null,
                    body text not null,
                    hidden integer not null default 0,
                    archived_at text not null
                );
                "#,
            )
            .expect("legacy schema should be created");

        initialize_archive_schema(&connection).expect("schema should migrate");

        let mut statement = connection
            .prepare("pragma table_info(notifications)")
            .expect("table info should be readable");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns should be listed")
            .collect::<Result<Vec<_>, _>>()
            .expect("columns should parse");
        assert!(columns.contains(&"system_deleted".to_string()));
    }

    #[test]
    fn clearing_hidden_records_keeps_system_deleted_records_hidden() {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        initialize_archive_schema(&connection).expect("schema should initialize");
        connection
            .execute(
                r#"
                insert into notifications (
                    record_id, app_identifier, app_name, delivered_at, title, subtitle, body,
                    hidden, system_deleted, archived_at
                )
                values (?1, 'com.example.app', 'Example', '2026-07-06T00:00:00Z', 'Title', '', '',
                    ?2, ?3, '2026-07-06T00:00:00Z')
                "#,
                (1_i64, 1_i64, 0_i64),
            )
            .expect("manual hidden record should insert");
        connection
            .execute(
                r#"
                insert into notifications (
                    record_id, app_identifier, app_name, delivered_at, title, subtitle, body,
                    hidden, system_deleted, archived_at
                )
                values (?1, 'com.example.app', 'Example', '2026-07-06T00:00:00Z', 'Title', '', '',
                    ?2, ?3, '2026-07-06T00:00:00Z')
                "#,
                (2_i64, 1_i64, 1_i64),
            )
            .expect("system deleted record should insert");

        clear_hidden_records_in_connection(&connection).expect("hidden records should clear");

        let manual_hidden: i64 = connection
            .query_row(
                "select hidden from notifications where record_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("manual record should exist");
        let system_deleted_hidden: i64 = connection
            .query_row(
                "select hidden from notifications where record_id = 2",
                [],
                |row| row.get(0),
            )
            .expect("system deleted record should exist");
        assert_eq!(manual_hidden, 0);
        assert_eq!(system_deleted_hidden, 1);
    }

    #[test]
    fn upsert_resets_local_flags_when_record_id_is_reused() {
        let mut connection = Connection::open_in_memory().expect("in-memory db should open");
        initialize_archive_schema(&connection).expect("schema should initialize");
        let old_record = NotificationRecord {
            id: 10,
            app_identifier: "com.example.old".to_string(),
            app_name: "Old".to_string(),
            delivered_at: Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap(),
            title: "旧通知".to_string(),
            subtitle: String::new(),
            body: "old".to_string(),
        };
        upsert_records_in_connection(&mut connection, &[old_record])
            .expect("old record should insert");
        connection
            .execute(
                "update notifications set hidden = 1, system_deleted = 1 where record_id = 10",
                [],
            )
            .expect("local flags should update");

        let new_record = NotificationRecord {
            id: 10,
            app_identifier: "com.example.new".to_string(),
            app_name: "New".to_string(),
            delivered_at: Utc.with_ymd_and_hms(2026, 7, 2, 0, 0, 0).unwrap(),
            title: "新通知".to_string(),
            subtitle: String::new(),
            body: "new".to_string(),
        };
        upsert_records_in_connection(&mut connection, &[new_record])
            .expect("reused id should update");

        let (title, hidden, system_deleted): (String, i64, i64) = connection
            .query_row(
                "select title, hidden, system_deleted from notifications where record_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("record should be readable");
        assert_eq!(title, "新通知");
        assert_eq!(hidden, 0);
        assert_eq!(system_deleted, 0);
    }

    #[test]
    fn upsert_preserves_local_flags_for_same_record() {
        let mut connection = Connection::open_in_memory().expect("in-memory db should open");
        initialize_archive_schema(&connection).expect("schema should initialize");
        let record = NotificationRecord {
            id: 10,
            app_identifier: "com.example.app".to_string(),
            app_name: "Example".to_string(),
            delivered_at: Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap(),
            title: "通知".to_string(),
            subtitle: String::new(),
            body: "body".to_string(),
        };
        upsert_records_in_connection(&mut connection, std::slice::from_ref(&record))
            .expect("record should insert");
        connection
            .execute(
                "update notifications set hidden = 1, system_deleted = 0 where record_id = 10",
                [],
            )
            .expect("local flags should update");

        upsert_records_in_connection(&mut connection, &[record])
            .expect("same record should update");

        let (hidden, system_deleted): (i64, i64) = connection
            .query_row(
                "select hidden, system_deleted from notifications where record_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("record should be readable");
        assert_eq!(hidden, 1);
        assert_eq!(system_deleted, 0);
    }

    #[test]
    fn archive_record_from_row_rejects_invalid_delivered_at() {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        initialize_archive_schema(&connection).expect("schema should initialize");
        connection
            .execute(
                r#"
                insert into notifications (
                    record_id, app_identifier, app_name, delivered_at, title, subtitle, body,
                    archived_at
                )
                values (1, 'com.example.app', 'Example', 'not-a-date', 'Broken', '', '',
                    '2026-07-06T00:00:00Z')
                "#,
                [],
            )
            .expect("corrupt row should insert");

        let result = connection.query_row(
            r#"
            select record_id, app_identifier, app_name, delivered_at, title, subtitle, body
            from notifications
            where record_id = 1
            "#,
            [],
            archive_record_from_row,
        );

        assert!(result.is_err());
    }

    #[test]
    fn pruning_archive_removes_invalid_and_old_notification_dates() {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        initialize_archive_schema(&connection).expect("schema should initialize");
        connection
            .execute_batch(
                r#"
                insert into notifications (
                    record_id, app_identifier, app_name, delivered_at, title, subtitle, body,
                    archived_at
                )
                values
                    (1, 'com.example.app', 'Example', '2026-07-01T00:00:00Z', 'Old', '', '', '2026-07-01T00:00:00Z'),
                    (2, 'com.example.app', 'Example', '2026-07-08T00:00:00Z', 'New', '', '', '2026-07-08T00:00:00Z'),
                    (3, 'com.example.app', 'Example', 'bad-date', 'Broken', '', '', '2026-07-08T00:00:00Z');
                "#,
            )
            .expect("rows should insert");

        prune_archive_in_connection(
            &connection,
            Utc.with_ymd_and_hms(2026, 7, 5, 0, 0, 0).unwrap(),
        )
        .expect("archive should prune");

        let mut statement = connection
            .prepare("select record_id from notifications order by record_id")
            .expect("statement should prepare");
        let remaining = statement
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("rows should query")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows should collect");
        assert_eq!(remaining, vec![2]);
    }
}
